use crate::Route;
use crate::auth::AuthState;
use crate::components::dialog::{DialogContent, DialogDescription, DialogRoot, DialogTitle};
use crate::fetch::CachedFetcher;
use dioxus::prelude::*;
use dioxus_logger::tracing::*;
use humansize::format_size;
use jacquard::prelude::*;
use jacquard::smol_str::ToSmolStr;
use jacquard::{
    client::AgentSessionExt,
    common::{Data, IntoStatic},
    identity::lexicon_resolver::LexiconSchemaResolver,
    smol_str::SmolStr,
    types::{aturi::AtUri, ident::AtIdentifier, string::Nsid},
};
use weaver_api::com_atproto::repo::{
    create_record::CreateRecord, delete_record::DeleteRecord, put_record::PutRecord,
};
use weaver_renderer::{code_pretty::highlight_code, css::generate_default_css};

#[derive(Clone, Copy, PartialEq)]
enum ViewMode {
    Pretty,
    Json,
}

#[component]
pub fn RecordView(uri: ReadSignal<SmolStr>) -> Element {
    let fetcher = use_context::<CachedFetcher>();
    let at_uri = AtUri::new_owned(uri());
    if let Err(err) = &at_uri {
        let error = format!("{:?}", err);
        return rsx! {
            div {
                h1 { "Record View" }
                p { "URI: {uri}" }
                p { "Error: {error}" }
            }
        };
    }
    let uri = use_signal(|| at_uri.unwrap());
    let mut view_mode = use_signal(|| ViewMode::Pretty);
    let mut edit_mode = use_signal(|| false);
    let navigator = use_navigator();

    let client = fetcher.get_client();
    let record = use_resource(move || {
        let client = client.clone();
        async move { client.fetch_record_slingshot(&uri()).await }
    });

    // Check ownership for edit access
    let auth_state = use_context::<Signal<AuthState>>();
    let is_owner = use_memo(move || {
        let auth = auth_state();
        if !auth.is_authenticated() {
            return false;
        }

        // authority() returns &AtIdentifier which can be Did or Handle
        match uri().authority() {
            AtIdentifier::Did(record_did) => auth.did.as_ref() == Some(record_did),
            AtIdentifier::Handle(_) => {
                // Can't easily check ownership for handles without async resolution
                false
            }
        }
    });
    if let Some(Ok(record)) = &*record.read_unchecked() {
        let record_value = record.value.clone().into_static();
        let mut edit_data = use_signal(|| record_value.clone());
        let nsid = use_memo(move || edit_data().type_discriminator().map(|s| s.to_string()));
        let json = serde_json::to_string_pretty(&record_value).unwrap();
        rsx! {
            document::Stylesheet { href: asset!("/assets/styling/record-view.css") }
            div {
                class: "record-view-container",
                div {
                    class: "record-header",
                    h1 { "Record" }
                    div {
                        class: "record-metadata",
                        div { class: "metadata-row",
                            span { class: "metadata-label", "URI" }
                            span { class: "metadata-value",
                                HighlightedUri { uri: uri().clone() }
                            }
                        }
                        if let Some(cid) = &record.cid {
                            div { class: "metadata-row",
                                span { class: "metadata-label", "CID" }
                                code { class: "metadata-value", "{cid}" }
                            }
                        }
                    }
                }
                div {
                    class: "tab-bar",
                    button {
                        class: if view_mode() == ViewMode::Pretty { "tab-button active" } else { "tab-button" },
                        onclick: move |_| view_mode.set(ViewMode::Pretty),
                        "View"
                    }
                    button {
                        class: if view_mode() == ViewMode::Json { "tab-button active" } else { "tab-button" },
                        onclick: move |_| view_mode.set(ViewMode::Json),
                        "JSON"
                    }
                    if is_owner() && !edit_mode() {
                        {
                            let record_value_clone = record_value.clone();
                            rsx! {
                                button {
                                    class: "tab-button edit-button",
                                    onclick: move |_| {
                                        edit_data.set(record_value_clone.clone());
                                        edit_mode.set(true);
                                    },
                                    "Edit"
                                }
                            }
                        }
                    }
                    if edit_mode() {
                        {
                            let record_value_clone = record_value.clone();
                            let update_fetcher = fetcher.clone();
                            let create_fetcher = fetcher.clone();
                            let replace_fetcher = fetcher.clone();
                            rsx! {
                                ActionButtons {
                                    on_update: move |_| {
                                        let fetcher = update_fetcher.clone();
                                        let uri = uri();
                                        let data = edit_data();
                                        spawn(async move {
                                            if let Some((did, _)) = fetcher.session_info().await {
                                                if let (Some(collection_str), Some(rkey)) = (uri.collection(), uri.rkey()) {
                                                    let collection = Nsid::new(collection_str.as_str()).ok();
                                                    if let Some(collection) = collection {
                                                        let request = PutRecord::new()
                                                            .repo(AtIdentifier::Did(did))
                                                            .collection(collection)
                                                            .rkey(rkey.clone())
                                                            .record(data)
                                                            .build();

                                                        match fetcher.send(request).await {
                                                            Ok(_) => {
                                                                dioxus_logger::tracing::info!("Record updated successfully");
                                                                edit_mode.set(false);
                                                            }
                                                            Err(e) => {
                                                                dioxus_logger::tracing::error!("Failed to update record: {:?}", e);
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        });
                                    },
                                    on_save_new: move |_| {
                                        let fetcher = create_fetcher.clone();
                                        let data = edit_data();
                                        let nav = navigator.clone();
                                        spawn(async move {
                                            if let Some((did, _)) = fetcher.session_info().await {
                                                if let Some(collection_str) = data.type_discriminator() {
                                                    let collection = Nsid::new(collection_str).ok();
                                                    if let Some(collection) = collection {
                                                        let request = CreateRecord::new()
                                                            .repo(AtIdentifier::Did(did))
                                                            .collection(collection)
                                                            .record(data.clone())
                                                            .build();

                                                        match fetcher.send(request).await {
                                                            Ok(response) => {
                                                                if let Ok(output) = response.into_output() {
                                                                    dioxus_logger::tracing::info!("Record created: {}", output.uri);
                                                                    nav.push(Route::RecordView { uri: output.uri.to_smolstr() });
                                                                }
                                                            }
                                                            Err(e) => {
                                                                dioxus_logger::tracing::error!("Failed to create record: {:?}", e);
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        });
                                    },
                                    on_replace: move |_| {
                                        let fetcher = replace_fetcher.clone();
                                        let uri = uri();
                                        let data = edit_data();
                                        let nav = navigator.clone();
                                        spawn(async move {
                                            if let Some((did, _)) = fetcher.session_info().await {
                                                if let Some(new_collection_str) = data.type_discriminator() {
                                                    let new_collection = Nsid::new(new_collection_str).ok();
                                                    if let Some(new_collection) = new_collection {
                                                        // Create new record
                                                        let create_req = CreateRecord::new()
                                                            .repo(AtIdentifier::Did(did.clone()))
                                                            .collection(new_collection)
                                                            .record(data.clone())
                                                            .build();

                                                        match fetcher.send(create_req).await {
                                                            Ok(response) => {
                                                                if let Ok(create_output) = response.into_output() {
                                                                    // Delete old record
                                                                    if let (Some(old_collection_str), Some(old_rkey)) = (uri.collection(), uri.rkey()) {
                                                                        let old_collection = Nsid::new(old_collection_str.as_str()).ok();
                                                                        if let Some(old_collection) = old_collection {
                                                                            let delete_req = DeleteRecord::new()
                                                                                .repo(AtIdentifier::Did(did))
                                                                                .collection(old_collection)
                                                                                .rkey(old_rkey.clone())
                                                                                .build();

                                                                            if let Err(e) = fetcher.send(delete_req).await {
                                                                                warn!("Created new record but failed to delete old: {:?}", e);
                                                                            }
                                                                        }
                                                                    }

                                                                    info!("Record replaced: {}", create_output.uri);
                                                                    nav.push(Route::RecordView { uri: create_output.uri.to_smolstr() });
                                                                }
                                                            }
                                                            Err(e) => {
                                                                error!("Failed to replace record: {:?}", e);
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        });
                                    },
                                    on_delete: move |_| {
                                        let fetcher = fetcher.clone();
                                        let uri = uri();
                                        let nav = navigator.clone();
                                        spawn(async move {
                                            if let Some((did, _)) = fetcher.session_info().await {
                                                if let (Some(collection_str), Some(rkey)) = (uri.collection(), uri.rkey()) {
                                                    let collection = Nsid::new(collection_str.as_str()).ok();
                                                    if let Some(collection) = collection {
                                                        let request = DeleteRecord::new()
                                                            .repo(AtIdentifier::Did(did))
                                                            .collection(collection)
                                                            .rkey(rkey.clone())
                                                            .build();

                                                        match fetcher.send(request).await {
                                                            Ok(_) => {
                                                                info!("Record deleted");
                                                                nav.push(Route::Home {});
                                                            }
                                                            Err(e) => {
                                                                error!("Failed to delete record: {:?}", e);
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        });
                                    },
                                    on_cancel: move |_| {
                                        edit_data.set(record_value_clone.clone());
                                        edit_mode.set(false);
                                    },
                                }
                            }
                        }
                    }
                }
                div {
                    class: "tab-content",
                    match (view_mode(), edit_mode()) {
                        (ViewMode::Pretty, false) => rsx! {
                            PrettyRecordView { record: record_value.clone(), uri: uri().clone() }
                        },
                        (ViewMode::Json, false) => rsx! {
                            CodeView {
                                code: use_signal(|| json.clone()),
                                lang: Some("json".to_string()),
                            }
                        },
                        (ViewMode::Pretty, true) => rsx! {
                            div { "Pretty editor not yet implemented" }
                        },
                        (ViewMode::Json, true) => rsx! {
                            JsonEditor {
                                data: edit_data,
                                nsid: nsid,
                            }
                        },
                    }
                }
            }
        }
    } else {
        rsx! {
            div {
                class: "record-view-container",
                h1 { "Record" }
                p { "URI: {uri}" }
                p { "Loading..." }
            }
        }
    }
}

#[component]
fn PrettyRecordView(record: Data<'static>, uri: AtUri<'static>) -> Element {
    let did = uri.authority().to_string();
    rsx! {
        div {
            class: "pretty-record",
            DataView { data: record, path: String::new(), did }
        }
    }
}
fn get_hex_rep(byte_array: &mut [u8]) -> String {
    let build_string_vec: Vec<String> = byte_array
        .chunks(2)
        .enumerate()
        .map(|(i, c)| {
            let sep = if i % 16 == 0 && i > 0 {
                "\n"
            } else if i == 0 {
                ""
            } else {
                " "
            };
            if c.len() == 2 {
                format!("{}{:02x}{:02x}", sep, c[0], c[1])
            } else {
                format!("{}{:02x}", sep, c[0])
            }
        })
        .collect();
    build_string_vec.join("")
}

#[component]
fn PathLabel(path: String) -> Element {
    if path.is_empty() {
        return rsx! {};
    }

    // Find the last separator
    let last_sep = path.rfind(|c| c == '.');

    if let Some(idx) = last_sep {
        let prefix = &path[..idx + 1];
        let final_segment = &path[idx + 1..];
        rsx! {
            span { class: "field-label",
                span { class: "path-prefix", "{prefix}" }
                span { class: "path-final", "{final_segment}" }
            }
        }
    } else {
        rsx! {
            span { class: "field-label","{path}" }
        }
    }
}

#[component]
fn DataView(data: Data<'static>, path: String, did: String) -> Element {
    match &data {
        Data::Null => rsx! {
            div { class: "record-field",
                PathLabel { path: path.clone() }
                span { class: "field-value muted", "null" }
            }
        },
        Data::Boolean(b) => rsx! {
            div { class: "record-field",
                PathLabel { path: path.clone() }
                span { class: "field-value", "{b}" }
            }
        },
        Data::Integer(i) => rsx! {
            div { class: "record-field",
                PathLabel { path: path.clone() }
                span { class: "field-value", "{i}" }
            }
        },
        Data::String(s) => {
            use jacquard::types::string::AtprotoStr;

            let type_label = match s {
                AtprotoStr::Datetime(_) => "datetime",
                AtprotoStr::Language(_) => "language",
                AtprotoStr::Tid(_) => "tid",
                AtprotoStr::Nsid(_) => "nsid",
                AtprotoStr::Did(_) => "did",
                AtprotoStr::Handle(_) => "handle",
                AtprotoStr::AtIdentifier(_) => "at-identifier",
                AtprotoStr::AtUri(_) => "at-uri",
                AtprotoStr::Uri(_) => "uri",
                AtprotoStr::Cid(_) => "cid",
                AtprotoStr::RecordKey(_) => "record-key",
                AtprotoStr::String(_) => "string",
            };

            rsx! {
                div { class: "record-field",
                    PathLabel { path: path.clone() }
                    span { class: "field-value",

                        HighlightedString { string_type: s.clone() }
                        if type_label != "string" {
                            span { class: "string-type-tag", " [{type_label}]" }
                        }
                    }
                }
            }
        }
        Data::Bytes(b) => {
            let hex_string = get_hex_rep(&mut b.to_vec());
            let byte_size = if b.len() > 128 {
                format_size(b.len(), humansize::BINARY)
            } else {
                format!("{} bytes", b.len())
            };
            rsx! {
                div { class: "record-field",
                    PathLabel { path: path.clone() }
                    pre { class: "field-value bytes", "{hex_string} [{byte_size}]" }
                }
            }
        }
        Data::CidLink(cid) => rsx! {
            div { class: "record-field",
                span { class: "field-label", "{path}" }
                span { class: "field-value", "{cid}" }
            }
        },
        Data::Array(arr) => rsx! {
            div { class: "record-section",
                div { class: "section-label", "{path}" span { class: "array-len", "[{arr.len()}] " } }

                div { class: "section-content",
                    for (idx, item) in arr.iter().enumerate() {
                        DataView {
                            data: item.clone(),
                            path: format!("{}[{}]", path, idx),
                            did: did.clone()
                        }
                    }
                }
            }
        },
        Data::Object(obj) => rsx! {
            for (key, value) in obj.iter() {
                {
                    let new_path = if path.is_empty() {
                        key.to_string()
                    } else {
                        format!("{}.{}", path, key)
                    };
                    let did_clone = did.clone();

                    match value {
                        Data::Object(_) | Data::Array(_) => rsx! {
                            div { class: "record-section",
                                div { class: "section-label", "{key}" }
                                div { class: "section-content",
                                    DataView { data: value.clone(), path: new_path, did: did_clone }
                                }
                            }
                        },
                        _ => rsx! {
                            DataView { data: value.clone(), path: new_path, did: did_clone }
                        }
                    }
                }
            }
        },
        Data::Blob(blob) => {
            let is_image = blob.mime_type.starts_with("image/");
            let format = blob.mime_type.strip_prefix("image/").unwrap_or("jpeg");
            let image_url = format!(
                "https://cdn.bsky.app/img/feed_fullsize/plain/{}/{}@{}",
                did,
                blob.cid(),
                format
            );

            let blob_size = format_size(blob.size, humansize::BINARY);
            rsx! {
                div { class: "record-field",
                    span { class: "field-label", "{path}" }
                    span { class: "field-value mime", "[mimeType: {blob.mime_type}, size: {blob_size}]" }
                    if is_image {
                        img {
                            src: "{image_url}",
                            alt: "Blob image",
                            class: "blob-image",
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn HighlightedUri(uri: AtUri<'static>) -> Element {
    let s = uri.as_str();
    let link = format!("/record#{}", s);

    if let Some(rest) = s.strip_prefix("at://") {
        let parts: Vec<&str> = rest.splitn(3, '/').collect();
        return rsx! {
            a {
                href: "{link}",
                class: "uri-link",
                span { class: "string-at-uri",
                    span { class: "aturi-scheme", "at://" }
                    span { class: "aturi-authority", "{uri.authority()}" }

                    if parts.len() > 1 {
                        span { class: "aturi-slash", "/" }
                        if let Some(collection) = uri.collection() {
                            span { class: "aturi-collection", "{collection.as_ref()}" }
                        }
                    }
                    if parts.len() > 2 {
                        span { class: "aturi-slash", "/" }
                        if let Some(rkey) = uri.rkey() {
                            span { class: "aturi-rkey", "{rkey.as_ref()}" }
                        }
                    }
                }
            }
        };
    }

    rsx! { span { class: "string-at-uri", "{s}" } }
}

#[component]
fn HighlightedString(string_type: jacquard::types::string::AtprotoStr<'static>) -> Element {
    use jacquard::types::string::AtprotoStr;

    match &string_type {
        AtprotoStr::Nsid(nsid) => {
            let parts: Vec<&str> = nsid.as_str().split('.').collect();
            rsx! {
                span { class: "string-nsid",
                    for (i, part) in parts.iter().enumerate() {
                        span { class: "nsid-segment nsid-segment-{i % 3}", "{part}" }
                        if i < parts.len() - 1 {
                            span { class: "nsid-dot", "." }
                        }
                    }
                }
            }
        }
        AtprotoStr::Did(did) => {
            let s = did.as_str();
            if let Some(rest) = s.strip_prefix("did:") {
                if let Some((method, identifier)) = rest.split_once(':') {
                    return rsx! {
                        span { class: "string-did",
                            span { class: "did-scheme", "did:" }
                            span { class: "did-method", "{method}" }
                            span { class: "did-separator", ":" }
                            span { class: "did-identifier", "{identifier}" }
                        }
                    };
                }
            }
            rsx! { span { class: "string-did", "{s}" } }
        }
        AtprotoStr::Handle(handle) => {
            let parts: Vec<&str> = handle.as_str().split('.').collect();
            rsx! {
                span { class: "string-handle",
                    for (i, part) in parts.iter().enumerate() {
                        span { class: "handle-segment handle-segment-{i % 2}", "{part}" }
                        if i < parts.len() - 1 {
                            span { class: "handle-dot", "." }
                        }
                    }
                }
            }
        }
        AtprotoStr::AtUri(uri) => {
            rsx! {
                HighlightedUri { uri: uri.clone().into_static() }
            }
        }
        AtprotoStr::Uri(uri) => {
            let s = uri.as_str();
            if let Ok(at_uri) = AtUri::new(s) {
                return rsx! {
                    HighlightedUri { uri: at_uri.into_static() }
                };
            }

            // Try to parse scheme
            if let Some((scheme, rest)) = s.split_once("://") {
                // Split authority and path
                let (authority, path) = if let Some(idx) = rest.find('/') {
                    (&rest[..idx], &rest[idx..])
                } else {
                    (rest, "")
                };

                return rsx! {
                    a {
                        href: "{s}",
                        target: "_blank",
                        rel: "noopener noreferrer",
                        class: "uri-link",
                        span { class: "string-uri",
                            span { class: "uri-scheme", "{scheme}" }
                            span { class: "uri-separator", "://" }
                            span { class: "uri-authority", "{authority}" }
                            if !path.is_empty() {
                                span { class: "uri-path", "{path}" }
                            }
                        }
                    }
                };
            }

            rsx! { span { class: "string-uri", "{s}" } }
        }
        _ => {
            let value = string_type.as_str();
            rsx! { "{value}" }
        }
    }
}

#[derive(Props, Clone, PartialEq)]
pub struct CodeViewProps {
    #[props(default)]
    id: Signal<String>,
    #[props(default)]
    class: Signal<String>,
    code: ReadSignal<String>,
    lang: Option<String>,
}

#[component]
fn JsonEditor(data: Signal<Data<'static>>, nsid: ReadSignal<Option<String>>) -> Element {
    let mut json_text =
        use_signal(|| serde_json::to_string_pretty(&*data.read()).unwrap_or_default());
    let mut parse_error = use_signal(|| None::<String>);

    let height = use_memo(move || {
        let line_count = json_text().lines().count();
        let min_lines = 10;
        let lines = line_count.max(min_lines);
        // line-height is 1.5, font-size is 0.9rem (approx 14.4px), so each line is ~21.6px
        // Add padding (1rem top + 1rem bottom = 2rem = 32px)
        format!("{}px", lines * 22 + 32)
    });

    let fetcher = use_context::<CachedFetcher>();

    let validation = use_resource(move || {
        let text = json_text();
        let nsid_val = nsid();
        let fetcher = fetcher.clone();

        async move {
            // Only validate if we have an NSID
            let nsid_str = nsid_val?;

            // Parse JSON to Data
            let parsed = match serde_json::from_str::<Data>(&text) {
                Ok(val) => val.into_static(),
                Err(e) => {
                    return Some((None, Some(e.to_string())));
                }
            };

            // Resolve lexicon if needed
            let registry = jacquard_lexicon::schema::SchemaRegistry::from_inventory();
            if registry.get(&nsid_str).is_none() {
                let nsid_str = nsid_str.split('#').next();
                if let Some(Ok(nsid_parsed)) = nsid_str.map(|s| Nsid::new(s)) {
                    if let Ok(schema) = fetcher.resolve_lexicon_schema(&nsid_parsed).await {
                        registry.insert(nsid_parsed.to_smolstr(), schema.doc);
                    }
                }
            }

            // Validate
            let validator = jacquard_lexicon::validation::SchemaValidator::from_registry(registry);
            let result = validator.validate_by_nsid(&nsid_str, &parsed);

            Some((Some(result), None))
        }
    });

    rsx! {
        div { class: "json-editor",
            textarea {
                class: "json-textarea",
                style: "height: {height};",
                value: "{json_text}",
                oninput: move |evt| {
                    json_text.set(evt.value());
                    // Update data signal on successful parse
                    if let Ok(parsed) = serde_json::from_str::<Data>(&evt.value()) {
                        data.set(parsed.into_static());
                    }
                },
            }

            ValidationPanel {
                validation: validation,
            }
        }
    }
}

#[component]
fn ActionButtons(
    on_update: EventHandler<()>,
    on_save_new: EventHandler<()>,
    on_replace: EventHandler<()>,
    on_delete: EventHandler<()>,
    on_cancel: EventHandler<()>,
) -> Element {
    let mut show_save_dropdown = use_signal(|| false);
    let mut show_replace_warning = use_signal(|| false);
    let mut show_delete_confirm = use_signal(|| false);

    rsx! {
        div { class: "action-buttons-group",
            button {
                class: "tab-button action-button",
                onclick: move |_| on_update.call(()),
                "Update"
            }

            div { class: "dropdown-wrapper",
                button {
                    class: "tab-button action-button",
                    onclick: move |_| show_save_dropdown.toggle(),
                    "Save as New ▼"
                }
                if show_save_dropdown() {
                    div { class: "dropdown-menu",
                        button {
                            onclick: move |_| {
                                show_save_dropdown.set(false);
                                on_save_new.call(());
                            },
                            "Save as New"
                        }
                        button {
                            onclick: move |_| {
                                show_save_dropdown.set(false);
                                show_replace_warning.set(true);
                            },
                            "Replace"
                        }
                    }
                }
            }

            if show_replace_warning() {
                div { class: "inline-warning",
                    "⚠️ This will delete the current record and create a new one with a different rkey. "
                    button {
                        onclick: move |_| {
                            show_replace_warning.set(false);
                            on_replace.call(());
                        },
                        "Yes"
                    }
                    button {
                        onclick: move |_| show_replace_warning.set(false),
                        "No"
                    }
                }
            }

            button {
                class: "tab-button action-button action-button-danger",
                onclick: move |_| show_delete_confirm.set(true),
                "Delete"
            }

            DialogRoot {
                open: Some(show_delete_confirm()),
                on_open_change: move |open: bool| {
                    show_delete_confirm.set(open);
                },
                DialogContent {
                    DialogTitle { "Delete Record?" }
                    DialogDescription {
                        "This action cannot be undone."
                    }
                    div { class: "dialog-actions",
                        button {
                            onclick: move |_| {
                                show_delete_confirm.set(false);
                                on_delete.call(());
                            },
                            "Delete"
                        }
                        button {
                            onclick: move |_| show_delete_confirm.set(false),
                            "Cancel"
                        }
                    }
                }
            }

            button {
                class: "tab-button action-button",
                onclick: move |_| on_cancel.call(()),
                "Cancel"
            }
        }
    }
}

#[component]
fn ValidationPanel(
    validation: Resource<
        Option<(
            Option<jacquard_lexicon::validation::ValidationResult>,
            Option<String>,
        )>,
    >,
) -> Element {
    rsx! {
        div { class: "validation-panel",
            if let Some(Some((result_opt, parse_error_opt))) = validation.read().as_ref() {
                if let Some(parse_err) = parse_error_opt {
                    div { class: "parse-error",
                        "❌ Invalid JSON: {parse_err}"
                    }
                } else {
                    div { class: "parse-success", "✓ Valid JSON syntax" }
                }

                if let Some(result) = result_opt {
                    if result.is_valid() {
                        div { class: "validation-success", "✓ Record is valid" }
                    } else {
                        div { class: "validation-errors",
                            h4 { "Validation Errors:" }
                            for error in result.all_errors() {
                                div { class: "error", "❌ {error}" }
                            }
                        }
                    }
                }
            } else {
                div { "Validating..." }
            }
        }
    }
}

/// Render some text as markdown.
#[component]
pub fn CodeView(props: CodeViewProps) -> Element {
    let code = &*props.code.read();

    let mut html_buf = String::new();
    highlight_code(props.lang.as_deref(), code, &mut html_buf).unwrap();

    rsx! {
        document::Style { {generate_default_css().unwrap()}}
        div {
            id: "{&*props.id.read()}",
            class: "{&*props.class.read()}",
            dangerous_inner_html: "{html_buf}"
        }
    }
}
