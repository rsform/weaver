use crate::Route;
use crate::auth::AuthState;
use crate::components::dialog::{DialogContent, DialogDescription, DialogRoot, DialogTitle};
use crate::fetch::CachedFetcher;
use dioxus::{CapturedError, prelude::*};
use humansize::format_size;
use jacquard::api::com_atproto::repo::get_record::GetRecordOutput;
use jacquard::client::AgentError;
use jacquard::common::to_data;
use jacquard::prelude::*;
use jacquard::smol_str::ToSmolStr;
use jacquard::{
    client::AgentSessionExt,
    common::{Data, IntoStatic},
    identity::lexicon_resolver::LexiconSchemaResolver,
    types::{aturi::AtUri, cid::Cid, ident::AtIdentifier, string::Nsid},
};
use jacquard_lexicon::lexicon::LexiconDoc;
use mime_sniffer::MimeTypeSniffer;
use weaver_api::com_atproto::repo::{
    create_record::CreateRecord, delete_record::DeleteRecord, put_record::PutRecord,
};
use weaver_renderer::{code_pretty::highlight_code, css::generate_default_css};

#[derive(Clone, Copy, PartialEq)]
enum ViewMode {
    Pretty,
    Json,
    Schema,
}

#[component]
pub fn RecordIndex() -> Element {
    let navigator = use_navigator();
    let mut uri_input = use_signal(|| String::new());
    let handle_uri_submit = move || {
        let input_uri = uri_input.read().clone();
        if !input_uri.is_empty() {
            if let Ok(parsed) = AtUri::new(&input_uri) {
                let link = format!("{}/record/{}", crate::env::WEAVER_APP_DOMAIN, parsed);
                navigator.push(link);
            }
        }
    };
    rsx! {
        document::Stylesheet { href: asset!("/assets/styling/record-view.css") }
        div {
            class: "record-view-container",
            div { class: "record-header",
                h1 { "Record View" }
                div { class: "uri-input-section",
                    input {
                        r#type: "text",
                        class: "uri-input",
                        placeholder: "at://did:plc:.../collection/rkey",
                        value: "{uri_input}",
                        oninput: move |evt| uri_input.set(evt.value()),
                        onkeydown: move |evt| {
                            if evt.key() == Key::Enter {
                                handle_uri_submit();
                            }
                        },
                    }
                }
            }

            Outlet::<Route> {}
        }
    }
}

#[component]
pub fn RecordView(uri: ReadSignal<Vec<String>>) -> Element {
    let fetcher = use_context::<CachedFetcher>();
    info!("Uri:{:?}", uri().join("/"));
    let at_uri = AtUri::new_owned(&*uri.read().join("/"));
    if at_uri.is_err() {
        return rsx! {};
    }
    let uri = use_signal(move || AtUri::new_owned(&*uri.read().join("/")).unwrap());
    let mut view_mode = use_signal(|| ViewMode::Pretty);
    let mut edit_mode = use_signal(|| false);

    let client = fetcher.get_client();
    let record_resource = use_resource(move || {
        let client = client.clone();
        async move { client.fetch_record_slingshot(&*uri.read()).await }
    });

    // Fetch schema for the record
    let schema_resource = use_resource(move || {
        let fetcher = fetcher.clone();
        async move {
            let record_read = record_resource.read();
            let record = record_read.as_ref()?.as_ref().ok()?;

            let validator = jacquard_lexicon::validation::SchemaValidator::global();
            let main_type = record.value.type_discriminator();
            let mut main_schema = None;

            // Find and resolve all schemas (including main and nested)
            for type_val in record.value.query("...$type").values() {
                if let Some(type_str) = type_val.as_str() {
                    // Skip non-NSID types (like "blob")
                    if !type_str.contains('.') {
                        continue;
                    }

                    if let Ok(nsid) = Nsid::new(type_str) {
                        // Fetch and register schema
                        if let Ok(schema) = fetcher.resolve_lexicon_schema(&nsid).await {
                            validator
                                .registry()
                                .insert(nsid.to_smolstr(), schema.doc.clone());

                            // Keep the main record schema
                            if Some(type_str) == main_type {
                                main_schema = Some(schema.doc);
                            }
                        }
                    }
                }
            }

            main_schema
        }
    });

    let schema_signal = use_memo(move || schema_resource.read().clone().flatten());

    // Check ownership for edit access
    let auth_state = use_context::<Signal<AuthState>>();
    let is_owner = use_memo(move || {
        let auth = auth_state();
        if !auth.is_authenticated() {
            return false;
        }

        // authority() returns &AtIdentifier which can be Did or Handle
        match &*uri.read().authority() {
            AtIdentifier::Did(record_did) => auth.did.as_ref() == Some(record_did),
            AtIdentifier::Handle(_) => {
                // Can't easily check ownership for handles without async resolution
                false
            }
        }
    });
    if let Some(Ok(record)) = &*record_resource.read() {
        let record_value = record.value.clone().into_static();
        let record = record.clone();

        rsx! {
            Fragment {  key: "{uri()}",
                RecordViewLayout {
                    uri: uri().clone(),
                    cid: record.cid.clone(),
                    if edit_mode() {

                        EditableRecordContent {
                            record_value: record_value,
                            uri: uri,
                            view_mode: view_mode,
                            edit_mode: edit_mode,
                            record_resource: record_resource,
                            schema: schema_signal,
                        }
                    } else {
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
                            button {
                                class: if view_mode() == ViewMode::Schema { "tab-button active" } else { "tab-button" },
                                onclick: move |_| view_mode.set(ViewMode::Schema),
                                "Schema"
                            }
                            if is_owner() {
                                button {
                                    class: "tab-button edit-button",
                                    onclick: move |_| edit_mode.set(true),
                                    "Edit"
                                }
                            }
                        }
                        div {
                            class: "tab-content",
                            match view_mode() {
                                ViewMode::Pretty => rsx! {
                                    PrettyRecordView { record: record_value, uri: uri().clone() }
                                },
                                ViewMode::Json => {
                                    let json = use_memo(use_reactive!(|record| serde_json::to_string_pretty(
                                        &record.value
                                    )
                                    .unwrap_or_default()));
                                    rsx! {
                                        CodeView {
                                            code: json,
                                            lang: Some("json".to_string()),
                                        }
                                    }
                                },
                                ViewMode::Schema => rsx! {
                                    SchemaView { schema: schema_signal }
                                },
                            }
                        }
                    }
                }
            }
        }
    } else {
        rsx! {}
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

#[component]
fn SchemaView(schema: ReadSignal<Option<LexiconDoc<'static>>>) -> Element {
    if let Some(schema_doc) = schema() {
        // Convert LexiconDoc to Data for display
        let schema_data = use_memo(move || to_data(&schema_doc).ok().map(|d| d.into_static()));

        if let Some(data) = schema_data() {
            rsx! {
                div {
                    class: "pretty-record",
                    DataView { data: data, path: String::new(), did: String::new() }
                }
            }
        } else {
            rsx! {
                div { class: "schema-error", "Failed to convert schema to displayable format" }
            }
        }
    } else {
        rsx! {
            div { class: "schema-loading", "Loading schema..." }
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
        Data::Array(arr) => {
            let label = path.split('.').last().unwrap_or(&path);
            rsx! {
                div { class: "record-section",
                    div { class: "section-label", "{label}" span { class: "array-len", "[{arr.len()}] " } }

                    div { class: "section-content",
                        for (idx, item) in arr.iter().enumerate() {
                            {
                                let item_path = format!("{}[{}]", label, idx);
                                let is_object = matches!(item, Data::Object(_));

                                if is_object {
                                    rsx! {

                                        div {
                                            class: "array-item",
                                        div { class: "record-section",
                                            div { class: "section-label", "{item_path}" }
                                            div { class: "section-content",
                                                DataView {
                                                    data: item.clone(),
                                                    path: item_path.clone(),
                                                    did: did.clone()
                                                }
                                            }
                                        }
                                        }
                                    }
                                } else {

                                    rsx! {

                                        div {
                                            class: "array-item",
                                        DataView {
                                            data: item.clone(),
                                            path: item_path,
                                            did: did.clone()
                                        }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        Data::Object(obj) => {
            let is_root = path.is_empty();
            let is_array_item = path.split('.').last().unwrap_or(&path).contains('[');

            if is_root || is_array_item {
                // Root object or array item: just render children (array items already wrapped)
                rsx! {
                    div { class: if !is_root { "record-section" } else {""},
                        for (key, value) in obj.iter() {
                            {
                                let new_path = if is_root {
                                    key.to_string()
                                } else {
                                    format!("{}.{}", path, key)
                                };
                                let did_clone = did.clone();
                                rsx! {
                                    DataView { data: value.clone(), path: new_path, did: did_clone }
                                }
                            }
                        }
                    }
                }
            } else {
                // Nested object (not array item): wrap in section
                let label = path.split('.').last().unwrap_or(&path);
                rsx! {

                    div { class: "section-label", "{label}" }
                    div { class: "record-section",
                        div { class: "section-content",
                            for (key, value) in obj.iter() {
                                {
                                    let new_path = format!("{}.{}", path, key);
                                    let did_clone = did.clone();
                                    rsx! {
                                        DataView { data: value.clone(), path: new_path, did: did_clone }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
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
    let link = format!("{}/record/{}", crate::env::WEAVER_APP_DOMAIN, s);

    if let Some(rest) = s.strip_prefix("at://") {
        let parts: Vec<&str> = rest.splitn(3, '/').collect();
        return rsx! {
            a {
                href: link,
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

    rsx! { a { class: "string-at-uri", href: s } }
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
fn JsonEditor(
    data: Signal<Data<'static>>,
    nsid: ReadSignal<Option<String>>,
    schema: ReadSignal<Option<LexiconDoc<'static>>>,
) -> Element {
    let mut json_text =
        use_signal(|| serde_json::to_string_pretty(&*data.read()).unwrap_or_default());

    let height = use_memo(move || {
        let line_count = json_text().lines().count();
        let min_lines = 10;
        let lines = line_count.max(min_lines);
        // line-height is 1.5, font-size is 0.9rem (approx 14.4px), so each line is ~21.6px
        // Add padding (1rem top + 1rem bottom = 2rem = 32px)
        format!("{}px", lines * 22 + 32)
    });

    let validation = use_resource(move || {
        let text = json_text();
        let nsid_val = nsid();
        let _ = schema(); // Track schema changes

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

            // Use global validator (schema already registered)
            let validator = jacquard_lexicon::validation::SchemaValidator::global();
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

// ============================================================================
// Pretty Editor: Helper Functions
// ============================================================================

/// Infer Data type from text input
fn infer_data_from_text(text: &str) -> Result<Data<'static>, String> {
    let trimmed = text.trim();

    if trimmed == "true" || trimmed == "false" {
        Ok(Data::Boolean(trimmed == "true"))
    } else if trimmed == "{}" {
        use jacquard::types::value::Object;
        use std::collections::BTreeMap;
        Ok(Data::Object(Object(BTreeMap::new())))
    } else if trimmed == "[]" {
        use jacquard::types::value::Array;
        Ok(Data::Array(Array(Vec::new())))
    } else if trimmed == "null" {
        Ok(Data::Null)
    } else if let Ok(num) = trimmed.parse::<i64>() {
        Ok(Data::Integer(num))
    } else {
        // Smart string parsing
        use jacquard::types::value::parsing;
        Ok(Data::String(parsing::parse_string(trimmed).into_static()))
    }
}

/// Parse text as specific AtprotoStr type, preserving type information
fn try_parse_as_type(
    text: &str,
    string_type: jacquard::types::LexiconStringType,
) -> Result<jacquard::types::string::AtprotoStr<'static>, String> {
    use jacquard::types::LexiconStringType;
    use jacquard::types::string::*;
    use std::str::FromStr;

    match string_type {
        LexiconStringType::Datetime => Datetime::from_str(text)
            .map(AtprotoStr::Datetime)
            .map_err(|e| format!("Invalid datetime: {}", e)),
        LexiconStringType::Did => Did::new(text)
            .map(|v| AtprotoStr::Did(v.into_static()))
            .map_err(|e| format!("Invalid DID: {}", e)),
        LexiconStringType::Handle => Handle::new(text)
            .map(|v| AtprotoStr::Handle(v.into_static()))
            .map_err(|e| format!("Invalid handle: {}", e)),
        LexiconStringType::AtUri => AtUri::new(text)
            .map(|v| AtprotoStr::AtUri(v.into_static()))
            .map_err(|e| format!("Invalid AT-URI: {}", e)),
        LexiconStringType::AtIdentifier => AtIdentifier::new(text)
            .map(|v| AtprotoStr::AtIdentifier(v.into_static()))
            .map_err(|e| format!("Invalid identifier: {}", e)),
        LexiconStringType::Nsid => Nsid::new(text)
            .map(|v| AtprotoStr::Nsid(v.into_static()))
            .map_err(|e| format!("Invalid NSID: {}", e)),
        LexiconStringType::Tid => Tid::new(text)
            .map(|v| AtprotoStr::Tid(v.into_static()))
            .map_err(|e| format!("Invalid TID: {}", e)),
        LexiconStringType::RecordKey => Rkey::new(text)
            .map(|rk| AtprotoStr::RecordKey(RecordKey::from(rk)))
            .map_err(|e| format!("Invalid record key: {}", e)),
        LexiconStringType::Cid => Cid::new(text.as_bytes())
            .map(|v| AtprotoStr::Cid(v.into_static()))
            .map_err(|_| "Invalid CID".to_string()),
        LexiconStringType::Language => Language::new(text)
            .map(AtprotoStr::Language)
            .map_err(|e| format!("Invalid language: {}", e)),
        LexiconStringType::Uri(_) => Uri::new(text)
            .map(|u| AtprotoStr::Uri(u.into_static()))
            .map_err(|e| format!("Invalid URI: {}", e)),
        LexiconStringType::String => {
            // Plain strings: use smart inference
            use jacquard::types::value::parsing;
            Ok(parsing::parse_string(text).into_static())
        }
    }
}

/// Create default value for new array item by cloning structure of existing items
fn create_array_item_default(arr: &jacquard::types::value::Array) -> Data<'static> {
    if let Some(existing) = arr.0.first() {
        clone_structure(existing)
    } else {
        // Empty array, default to null (user can change type)
        Data::Null
    }
}

/// Clone structure of Data, setting sensible defaults for leaf values
fn clone_structure(data: &Data) -> Data<'static> {
    use jacquard::types::string::*;
    use jacquard::types::value::{Array, Object};
    use jacquard::types::{LexiconStringType, blob::*};
    use std::collections::BTreeMap;

    match data {
        Data::Object(obj) => {
            let mut new_obj = BTreeMap::new();
            for (key, value) in obj.0.iter() {
                new_obj.insert(key.clone(), clone_structure(value));
            }
            Data::Object(Object(new_obj))
        }

        Data::Array(_) => Data::Array(Array(Vec::new())),

        Data::String(s) => match s.string_type() {
            LexiconStringType::Datetime => {
                // Sensible default: now
                Data::String(AtprotoStr::Datetime(Datetime::now()))
            }
            _ => {
                // Empty string, type inference will handle it
                Data::String(AtprotoStr::String("".into()))
            }
        },

        Data::Integer(_) => Data::Integer(0),
        Data::Boolean(_) => Data::Boolean(false),

        Data::Blob(blob) => {
            // Placeholder blob
            Data::Blob(
                Blob {
                    r#ref: CidLink::str(
                        "bafkreiaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                    ),
                    mime_type: blob.mime_type.clone(),
                    size: 0,
                }
                .into_static(),
            )
        }

        Data::Bytes(_) | Data::CidLink(_) | Data::Null => Data::Null,
    }
}

// ============================================================================
// Pretty Editor: Component Hierarchy
// ============================================================================

/// Main dispatcher - routes to specific field editors based on Data type
#[component]
fn EditableDataView(
    root: Signal<Data<'static>>,
    path: String,
    did: String,
    #[props(default)] remove_button: Option<Element>,
) -> Element {
    let path_for_memo = path.clone();
    let root_read = root.read();

    match root_read
        .get_at_path(&path_for_memo)
        .map(|d| d.clone().into_static())
    {
        Some(Data::Object(_)) => {
            rsx! { EditableObjectField { root, path: path.clone(), did, remove_button } }
        }
        Some(Data::Array(_)) => rsx! { EditableArrayField { root, path: path.clone(), did } },
        Some(Data::String(_)) => {
            rsx! { EditableStringField { root, path: path.clone(), remove_button } }
        }
        Some(Data::Integer(_)) => {
            rsx! { EditableIntegerField { root, path: path.clone(), remove_button } }
        }
        Some(Data::Boolean(_)) => {
            rsx! { EditableBooleanField { root, path: path.clone(), remove_button } }
        }
        Some(Data::Null) => rsx! { EditableNullField { root, path: path.clone(), remove_button } },
        Some(Data::Blob(_)) => {
            rsx! { EditableBlobField { root, path: path.clone(), did, remove_button } }
        }
        Some(Data::Bytes(_)) => {
            rsx! { EditableBytesField { root, path: path.clone(), remove_button } }
        }
        Some(Data::CidLink(_)) => {
            rsx! { EditableCidLinkField { root, path: path.clone(), remove_button } }
        }

        None => rsx! { div { class: "field-error", "❌ Path not found: {path}" } },
    }
}

// ============================================================================
// Primitive Field Editors
// ============================================================================

/// String field with type preservation
#[component]
fn EditableStringField(
    root: Signal<Data<'static>>,
    path: String,
    #[props(default)] remove_button: Option<Element>,
) -> Element {
    use jacquard::types::LexiconStringType;

    let path_for_text = path.clone();
    let path_for_type = path.clone();

    // Get current string value
    let current_text = use_memo(move || {
        root.read()
            .get_at_path(&path_for_text)
            .and_then(|d| d.as_str())
            .map(|s| s.to_string())
            .unwrap_or_default()
    });

    // Get string type (Copy, cheap to store)
    let string_type = use_memo(move || {
        root.read()
            .get_at_path(&path_for_type)
            .and_then(|d| match d {
                Data::String(s) => Some(s.string_type()),
                _ => None,
            })
            .unwrap_or(LexiconStringType::String)
    });

    // Local state for invalid input
    let mut input_text = use_signal(|| current_text());
    let mut parse_error = use_signal(|| None::<String>);

    // Sync input when current changes
    use_effect(move || {
        input_text.set(current_text());
    });

    let path_for_mutation = path.clone();
    let handle_input = move |evt: dioxus::prelude::Event<dioxus::prelude::FormData>| {
        let new_text = evt.value();
        input_text.set(new_text.clone());

        match try_parse_as_type(&new_text, string_type()) {
            Ok(new_atproto_str) => {
                parse_error.set(None);
                let mut new_data = root.read().clone();
                new_data.set_at_path(&path_for_mutation, Data::String(new_atproto_str));
                root.set(new_data);
            }
            Err(e) => {
                parse_error.set(Some(e));
            }
        }
    };

    let type_label = format!("{:?}", string_type()).to_lowercase();
    let is_plain_string = string_type() == LexiconStringType::String;

    // Dynamic width based on content length
    let input_width = use_memo(move || {
        let len = input_text().len();
        let min_width = match string_type() {
            LexiconStringType::Cid => 60,
            LexiconStringType::Nsid => 40,
            LexiconStringType::Did => 50,
            LexiconStringType::AtUri => 50,
            _ => 20,
        };
        format!("{}ch", len.max(min_width))
    });

    rsx! {
        div { class: "record-field",
            div { class: "field-header",
                PathLabel { path: path.clone() }
                if type_label != "string" {
                    span { class: "string-type-tag", " [{type_label}]" }
                }
                {remove_button}
            }
            if is_plain_string {
                textarea {
                    value: "{input_text}",
                    oninput: handle_input,
                    class: if parse_error().is_some() { "invalid" } else { "" },
                    rows: "1",
                }
            } else {
                input {
                    r#type: "text",
                    value: "{input_text}",
                    style: "width: {input_width}",
                    oninput: handle_input,
                    class: if parse_error().is_some() { "invalid" } else { "" },
                }
            }
            if let Some(err) = parse_error() {
                span { class: "field-error", " ❌ {err}" }
            }
        }
    }
}

/// Integer field with validation
#[component]
fn EditableIntegerField(
    root: Signal<Data<'static>>,
    path: String,
    #[props(default)] remove_button: Option<Element>,
) -> Element {
    let path_for_memo = path.clone();
    let current_value = use_memo(move || {
        root.read()
            .get_at_path(&path_for_memo)
            .and_then(|d| d.as_integer())
            .unwrap_or(0)
    });

    let mut input_text = use_signal(|| current_value().to_string());
    let mut parse_error = use_signal(|| None::<String>);

    use_effect(move || {
        input_text.set(current_value().to_string());
    });

    let path_for_mutation = path.clone();

    rsx! {
        div { class: "record-field",
            div { class: "field-header",
                PathLabel { path: path.clone() }
                {remove_button}
            }
            input {
                r#type: "number",
                value: "{input_text}",
                oninput: move |evt| {
                    let text = evt.value();
                    input_text.set(text.clone());

                    match text.parse::<i64>() {
                        Ok(num) => {
                            parse_error.set(None);
                            let mut data_edit = root.write_unchecked();
                             data_edit.set_at_path(&path_for_mutation, Data::Integer(num));
                        }
                        Err(_) => {
                            parse_error.set(Some("Must be a valid integer".to_string()));
                        }
                    }
                }
            }
            if let Some(err) = parse_error() {
                span { class: "field-error", " ❌ {err}" }
            }
        }
    }
}

/// Boolean field (toggle button)
#[component]
fn EditableBooleanField(
    root: Signal<Data<'static>>,
    path: String,
    #[props(default)] remove_button: Option<Element>,
) -> Element {
    let path_for_memo = path.clone();
    let current_value = use_memo(move || {
        root.read()
            .get_at_path(&path_for_memo)
            .and_then(|d| d.as_boolean())
            .unwrap_or(false)
    });

    let path_for_mutation = path.clone();
    rsx! {
        div { class: "record-field",
            div { class: "field-header",
                PathLabel { path: path.clone() }
                {remove_button}
            }
            button {
                class: if current_value() { "boolean-toggle boolean-toggle-true" } else { "boolean-toggle boolean-toggle-false" },
                onclick: move |_| {
                    root.with_mut(|data| {
                        if let Some(target) = data.get_at_path_mut(path_for_mutation.as_str()) {
                            if let Some(bool_val) = target.as_boolean() {
                                *target = Data::Boolean(!bool_val);
                            }
                        }
                    });
                },
                "{current_value()}"
            }
        }
    }
}

/// Null field with type inference
#[component]
fn EditableNullField(
    root: Signal<Data<'static>>,
    path: String,
    #[props(default)] remove_button: Option<Element>,
) -> Element {
    let mut input_text = use_signal(|| String::new());
    let mut parse_error = use_signal(|| None::<String>);

    let path_for_mutation = path.clone();
    rsx! {
        div { class: "record-field",
            div { class: "field-header",
                PathLabel { path: path.clone() }
                span { class: "field-value muted", "null" }
                {remove_button}
            }
            input {
                r#type: "text",
                placeholder: "Enter value (or {{}}, [], true, 123)...",
                value: "{input_text}",
                oninput: move |evt| {
                    input_text.set(evt.value());
                },
                onkeydown: move |evt| {
                    use dioxus::prelude::keyboard_types::Key;
                    if evt.key() == Key::Enter {
                        let text = input_text();
                        match infer_data_from_text(&text) {
                            Ok(new_value) => {
                                root.with_mut(|data| {
                                    if let Some(target) = data.get_at_path_mut(path_for_mutation.as_str()) {
                                        *target = new_value;
                                    }
                                });
                                input_text.set(String::new());
                                parse_error.set(None);
                            }
                            Err(e) => {
                                parse_error.set(Some(e));
                            }
                        }
                    }
                }
            }
            if let Some(err) = parse_error() {
                span { class: "field-error", " ❌ {err}" }
            }
        }
    }
}

/// Blob field - shows CID, size (editable), mime type (read-only), file upload
#[component]
fn EditableBlobField(
    root: Signal<Data<'static>>,
    path: String,
    did: String,
    #[props(default)] remove_button: Option<Element>,
) -> Element {
    let path_for_memo = path.clone();
    let blob_data = use_memo(move || {
        root.read()
            .get_at_path(&path_for_memo)
            .and_then(|d| match d {
                Data::Blob(blob) => Some((
                    blob.r#ref.to_string(),
                    blob.size,
                    blob.mime_type.as_str().to_string(),
                )),
                _ => None,
            })
    });

    let mut cid_input = use_signal(|| String::new());
    let mut size_input = use_signal(|| String::new());
    let mut cid_error = use_signal(|| None::<String>);
    let mut size_error = use_signal(|| None::<String>);
    let mut uploading = use_signal(|| false);
    let mut upload_error = use_signal(|| None::<String>);
    let mut preview_data_url = use_signal(|| None::<String>);

    // Sync inputs when blob data changes
    use_effect(move || {
        if let Some((cid, size, _)) = blob_data() {
            cid_input.set(cid);
            size_input.set(size.to_string());
        }
    });

    let fetcher = use_context::<CachedFetcher>();
    let path_for_upload = path.clone();
    let handle_file = move |evt: dioxus::prelude::Event<dioxus::prelude::FormData>| {
        let fetcher = fetcher.clone();
        let path_upload_clone = path_for_upload.clone();
        spawn(async move {
            uploading.set(true);
            upload_error.set(None);

            let files = evt.files();
            for file_data in files {
                match file_data.read_bytes().await {
                    Ok(bytes_data) => {
                        // Convert to jacquard Bytes and sniff MIME type
                        let bytes = jacquard::bytes::Bytes::from(bytes_data.to_vec());
                        let mime_str = bytes
                            .sniff_mime_type()
                            .unwrap_or("application/octet-stream");
                        let mime_type = jacquard::types::blob::MimeType::new_owned(mime_str);

                        // Create data URL for immediate preview if it's an image
                        if mime_str.starts_with("image/") {
                            let base64_data = base64::Engine::encode(
                                &base64::engine::general_purpose::STANDARD,
                                &bytes,
                            );
                            let data_url = format!("data:{};base64,{}", mime_str, base64_data);
                            preview_data_url.set(Some(data_url.clone()));

                            // Try to decode dimensions and populate aspectRatio field
                            #[cfg(target_arch = "wasm32")]
                            {
                                let path_clone = path_upload_clone.clone();
                                spawn(async move {
                                    if let Some((width, height)) =
                                        decode_image_dimensions(&data_url).await
                                    {
                                        populate_aspect_ratio(
                                            root,
                                            &path_clone,
                                            width as i64,
                                            height as i64,
                                        );
                                    }
                                });
                            }
                        }

                        // Upload blob
                        let client = fetcher.get_client();
                        match client.upload_blob(bytes, mime_type).await {
                            Ok(new_blob) => {
                                // Update blob in record
                                let path_ref = path_upload_clone.clone();
                                root.with_mut(|record_data| {
                                    if let Some(Data::Blob(blob)) =
                                        record_data.get_at_path_mut(&path_ref)
                                    {
                                        *blob = new_blob;
                                    }
                                });
                                upload_error.set(None);
                            }
                            Err(e) => {
                                upload_error.set(Some(format!("Upload failed: {:?}", e)));
                            }
                        }
                    }
                    Err(e) => {
                        upload_error.set(Some(format!("Failed to read file: {}", e)));
                    }
                }
            }

            uploading.set(false);
        });
    };

    let path_for_cid = path.clone();
    let handle_cid_change = move |evt: dioxus::prelude::Event<dioxus::prelude::FormData>| {
        let text = evt.value();
        cid_input.set(text.clone());

        match jacquard::types::cid::CidLink::new_owned(text.as_bytes()) {
            Ok(new_cid_link) => {
                cid_error.set(None);
                root.with_mut(|data| {
                    if let Some(Data::Blob(blob)) = data.get_at_path_mut(&path_for_cid) {
                        blob.r#ref = new_cid_link;
                    }
                });
            }
            Err(_) => {
                cid_error.set(Some("Invalid CID format".to_string()));
            }
        }
    };

    let path_for_size = path.clone();
    let handle_size_change = move |evt: dioxus::prelude::Event<dioxus::prelude::FormData>| {
        let text = evt.value();
        size_input.set(text.clone());

        match text.parse::<usize>() {
            Ok(new_size) => {
                size_error.set(None);
                root.with_mut(|data| {
                    if let Some(Data::Blob(blob)) = data.get_at_path_mut(&path_for_size) {
                        blob.size = new_size;
                    }
                });
            }
            Err(_) => {
                size_error.set(Some("Must be a non-negative integer".to_string()));
            }
        }
    };

    let placeholder_cid = "bafkreiaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let is_placeholder = blob_data()
        .map(|(cid, _, _)| cid == placeholder_cid)
        .unwrap_or(true);
    let is_image = blob_data()
        .map(|(_, _, mime)| mime.starts_with("image/"))
        .unwrap_or(false);

    // Use preview data URL if available (fresh upload), otherwise CDN
    let image_url = if let Some(data_url) = preview_data_url() {
        Some(data_url)
    } else if !is_placeholder && is_image {
        blob_data().map(|(cid, _, mime)| {
            let format = mime.strip_prefix("image/").unwrap_or("jpeg");
            format!(
                "https://cdn.bsky.app/img/feed_fullsize/plain/{}/{}@{}",
                did, cid, format
            )
        })
    } else {
        None
    };

    rsx! {
        div { class: "record-field blob-field",
            div { class: "field-header",
                PathLabel { path: path.clone() }
                span { class: "string-type-tag", " [blob]" }
                {remove_button}
            }
            div { class: "blob-fields",
                div { class: "blob-field-row blob-field-cid",
                    label { "CID:" }
                    input {
                        r#type: "text",
                        value: "{cid_input}",
                        oninput: handle_cid_change,
                        class: if cid_error().is_some() { "invalid" } else { "" },
                    }
                    if let Some(err) = cid_error() {
                        span { class: "field-error", " ❌ {err}" }
                    }
                }
                div { class: "blob-field-row",
                    label { "Size:" }
                    input {
                        r#type: "number",
                        value: "{size_input}",
                        oninput: handle_size_change,
                        class: if size_error().is_some() { "invalid" } else { "" },
                    }
                    if let Some(err) = size_error() {
                        span { class: "field-error", " ❌ {err}" }
                    }
                }
                div { class: "blob-field-row",
                    label { "MIME Type:" }
                    span { class: "readonly",
                        "{blob_data().map(|(_, _, mime)| mime).unwrap_or_default()}"
                    }
                }
                if let Some(url) = image_url {
                    img {
                        src: "{url}",
                        alt: "Blob preview",
                        class: "blob-image",
                    }
                }
                div { class: "blob-upload-section",
                    input {
                        r#type: "file",
                        accept: if is_image { "image/*" } else { "*/*" },
                        onchange: handle_file,
                        disabled: uploading(),
                    }
                    if uploading() {
                        span { class: "upload-status", "Uploading..." }
                    }
                    if let Some(err) = upload_error() {
                        div { class: "field-error", "❌ {err}" }
                    }
                }
            }
        }
    }
}

/// Decode image dimensions from data URL using browser Image API
#[cfg(target_arch = "wasm32")]
async fn decode_image_dimensions(data_url: &str) -> Option<(u32, u32)> {
    use wasm_bindgen::JsCast;
    use wasm_bindgen::prelude::*;
    use wasm_bindgen_futures::JsFuture;

    let window = web_sys::window()?;
    let document = window.document()?;

    let img = document.create_element("img").ok()?;
    let img = img.dyn_into::<web_sys::HtmlImageElement>().ok()?;

    img.set_src(data_url);

    // Wait for image to load
    let promise = js_sys::Promise::new(&mut |resolve, _reject| {
        let onload = Closure::wrap(Box::new(move || {
            resolve.call0(&JsValue::NULL).ok();
        }) as Box<dyn FnMut()>);

        img.set_onload(Some(onload.as_ref().unchecked_ref()));
        onload.forget();
    });

    JsFuture::from(promise).await.ok()?;

    Some((img.natural_width(), img.natural_height()))
}

/// Find and populate aspectRatio field for a blob
#[allow(unused)]
fn populate_aspect_ratio(
    mut root: Signal<Data<'static>>,
    blob_path: &str,
    width: i64,
    height: i64,
) {
    // Query for all aspectRatio fields and collect the path we want
    let aspect_path_to_update = {
        let data = root.read();
        let query_result = data.query("...aspectRatio");

        query_result.multiple().and_then(|matches| {
            // Find aspectRatio that's a sibling of our blob
            // e.g. blob at "embed.images[0].image" -> look for "embed.images[0].aspectRatio"
            let blob_parent = blob_path.rsplit_once('.').map(|(parent, _)| parent);

            matches.iter().find_map(|query_match| {
                let aspect_path = query_match.path.as_str();
                let aspect_parent = aspect_path.rsplit_once('.').map(|(parent, _)| parent);

                // Check if they share the same parent
                if blob_parent == aspect_parent {
                    Some(aspect_path.to_string())
                } else {
                    None
                }
            })
        })
    };

    // Update the aspectRatio if we found a matching field
    if let Some(aspect_path) = aspect_path_to_update {
        use jacquard::types::value::Object;
        use std::collections::BTreeMap;

        let mut aspect_obj = BTreeMap::new();
        aspect_obj.insert("width".into(), Data::Integer(width));
        aspect_obj.insert("height".into(), Data::Integer(height));

        root.with_mut(|record_data| {
            record_data.set_at_path(&aspect_path, Data::Object(Object(aspect_obj)));
        });
    }
}

/// Bytes field with hex/base64 auto-detection
#[component]
fn EditableBytesField(
    root: Signal<Data<'static>>,
    path: String,
    #[props(default)] remove_button: Option<Element>,
) -> Element {
    let path_for_memo = path.clone();
    let current_bytes = use_memo(move || {
        root.read()
            .get_at_path(&path_for_memo)
            .and_then(|d| match d {
                Data::Bytes(b) => Some(bytes_to_hex(b)),
                _ => None,
            })
    });

    let mut input_text = use_signal(|| String::new());
    let mut parse_error = use_signal(|| None::<String>);
    let mut detected_format = use_signal(|| None::<String>);

    // Sync input when bytes change
    use_effect(move || {
        if let Some(hex) = current_bytes() {
            input_text.set(hex);
        }
    });

    let path_for_mutation = path.clone();
    let handle_input = move |evt: dioxus::prelude::Event<dioxus::prelude::FormData>| {
        let text = evt.value();
        input_text.set(text.clone());

        match parse_bytes_input(&text) {
            Ok((bytes, format)) => {
                parse_error.set(None);
                detected_format.set(Some(format));
                root.with_mut(|data| {
                    if let Some(target) = data.get_at_path_mut(&path_for_mutation) {
                        *target = Data::Bytes(bytes);
                    }
                });
            }
            Err(e) => {
                parse_error.set(Some(e));
                detected_format.set(None);
            }
        }
    };

    let byte_count = current_bytes()
        .map(|hex| hex.chars().filter(|c| c.is_ascii_hexdigit()).count() / 2)
        .unwrap_or(0);
    let size_label = if byte_count > 128 {
        format_size(byte_count, humansize::BINARY)
    } else {
        format!("{} bytes", byte_count)
    };

    rsx! {
        div { class: "record-field bytes-field",
            div { class: "field-header",
                PathLabel { path: path.clone() }
                span { class: "string-type-tag", " [bytes: {size_label}]" }
                if let Some(format) = detected_format() {
                    span { class: "bytes-format-tag", " ({format})" }
                }
                {remove_button}
            }
            textarea {
                value: "{input_text}",
                placeholder: "Paste hex (1a2b3c...) or base64 (YWJj...)",
                oninput: handle_input,
                class: if parse_error().is_some() { "invalid" } else { "" },
                rows: "3",
            }
            if let Some(err) = parse_error() {
                span { class: "field-error", " ❌ {err}" }
            }
        }
    }
}

/// Parse bytes from hex or base64, auto-detecting format
fn parse_bytes_input(text: &str) -> Result<(jacquard::bytes::Bytes, String), String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err("Input is empty".to_string());
    }

    // Remove common whitespace/separators
    let cleaned: String = trimmed
        .chars()
        .filter(|c| !c.is_whitespace() && *c != ':' && *c != '-')
        .collect();

    // Try hex first (more restrictive)
    if cleaned.chars().all(|c| c.is_ascii_hexdigit()) {
        parse_hex_bytes(&cleaned).map(|b| (b, "hex".to_string()))
    } else {
        // Try base64
        parse_base64_bytes(&cleaned).map(|b| (b, "base64".to_string()))
    }
}

/// Parse hex string to bytes
fn parse_hex_bytes(hex: &str) -> Result<jacquard::bytes::Bytes, String> {
    if hex.len() % 2 != 0 {
        return Err("Hex string must have even length".to_string());
    }

    let mut bytes = Vec::with_capacity(hex.len() / 2);
    for chunk in hex.as_bytes().chunks(2) {
        let hex_byte = std::str::from_utf8(chunk).map_err(|e| format!("Invalid UTF-8: {}", e))?;
        let byte =
            u8::from_str_radix(hex_byte, 16).map_err(|e| format!("Invalid hex digit: {}", e))?;
        bytes.push(byte);
    }

    Ok(jacquard::bytes::Bytes::from(bytes))
}

/// Parse base64 string to bytes
fn parse_base64_bytes(b64: &str) -> Result<jacquard::bytes::Bytes, String> {
    use base64::Engine;
    let engine = base64::engine::general_purpose::STANDARD;

    engine
        .decode(b64)
        .map(jacquard::bytes::Bytes::from)
        .map_err(|e| format!("Invalid base64: {}", e))
}

/// Convert bytes to hex display string (with spacing every 4 chars)
fn bytes_to_hex(bytes: &jacquard::bytes::Bytes) -> String {
    bytes
        .iter()
        .enumerate()
        .map(|(i, b)| {
            let hex = format!("{:02x}", b);
            if i > 0 && i % 2 == 0 {
                format!(" {}", hex)
            } else {
                hex
            }
        })
        .collect()
}

/// CidLink field with validation
#[component]
fn EditableCidLinkField(
    root: Signal<Data<'static>>,
    path: String,
    #[props(default)] remove_button: Option<Element>,
) -> Element {
    let path_for_memo = path.clone();
    let current_cid = use_memo(move || {
        root.read()
            .get_at_path(&path_for_memo)
            .map(|d| match d {
                Data::CidLink(cid) => cid.to_string(),
                _ => String::new(),
            })
            .unwrap_or_default()
    });

    let mut input_text = use_signal(|| String::new());
    let mut parse_error = use_signal(|| None::<String>);

    use_effect(move || {
        input_text.set(current_cid());
    });

    let input_width = use_memo(move || {
        let len = input_text().len();
        format!("{}ch", len.max(60))
    });

    let path_for_mutation = path.clone();
    let handle_input = move |evt: dioxus::prelude::Event<dioxus::prelude::FormData>| {
        let text = evt.value();
        input_text.set(text.clone());

        match jacquard::types::cid::Cid::new_owned(text.as_bytes()) {
            Ok(new_cid) => {
                parse_error.set(None);
                root.with_mut(|data| {
                    if let Some(target) = data.get_at_path_mut(&path_for_mutation) {
                        *target = Data::CidLink(new_cid);
                    }
                });
            }
            Err(_) => {
                parse_error.set(Some("Invalid CID format".to_string()));
            }
        }
    };

    rsx! {
        div { class: "record-field cidlink-field",
            div { class: "field-header",
                PathLabel { path: path.clone() }
                span { class: "string-type-tag", " [cid-link]" }
                {remove_button}
            }
            input {
                r#type: "text",
                value: "{input_text}",
                style: "width: {input_width}",
                placeholder: "bafyrei...",
                oninput: handle_input,
                class: if parse_error().is_some() { "invalid" } else { "" },
            }
            if let Some(err) = parse_error() {
                span { class: "field-error", " ❌ {err}" }
            }
        }
    }
}

// ============================================================================
// Field with Remove Button Wrapper
// ============================================================================

/// Wraps a field with an optional remove button in the header
#[component]
fn FieldWithRemove(
    root: Signal<Data<'static>>,
    path: String,
    did: String,
    is_removable: bool,
    parent_path: String,
    field_key: String,
) -> Element {
    let remove_button = if is_removable {
        Some(rsx! {
            button {
                class: "field-remove-button",
                onclick: move |_| {
                    let mut new_data = root.read().clone();
                    if let Some(Data::Object(obj)) = new_data.get_at_path_mut(parent_path.as_str()) {
                        obj.0.remove(field_key.as_str());
                    }
                    root.set(new_data);
                },
                "Remove"
            }
        })
    } else {
        None
    };

    rsx! {
        EditableDataView {
            root: root,
            path: path.clone(),
            did: did.clone(),
            remove_button: remove_button,
        }
    }
}

// ============================================================================
// Array Field Editor (enables recursion)
// ============================================================================

/// Array field - iterates items and renders child EditableDataView for each
#[component]
fn EditableArrayField(root: Signal<Data<'static>>, path: String, did: String) -> Element {
    let path_for_memo = path.clone();
    let array_len = use_memo(move || {
        root.read()
            .get_at_path(&path_for_memo)
            .and_then(|d| d.as_array())
            .map(|arr| arr.0.len())
            .unwrap_or(0)
    });

    let path_for_add = path.clone();

    rsx! {
        div { class: "record-section array-section",
            div { class: "section-header",
                div { class: "section-label",
                    {
                        let parts: Vec<&str> = path.split('.').collect();
                        let final_part = parts.last().unwrap_or(&"");
                        rsx! { "{final_part}" }
                    }
                }
                span { class: "array-length", "[{array_len}]" }
            }

            div { class: "section-content",
                for idx in 0..array_len() {
                    {
                        let item_path = format!("{}[{}]", path, idx);
                        let path_for_remove = path.clone();

                        rsx! {
                            div {
                                class: "array-item",
                                key: "{item_path}",

                                EditableDataView {
                                    root: root,
                                    path: item_path.clone(),
                                    did: did.clone(),
                                    remove_button: rsx! {
                                        button {
                                            class: "field-remove-button",
                                            onclick: move |_| {
                                                root.with_mut(|data| {
                                                    if let Some(Data::Array(arr)) = data.get_at_path_mut(&path_for_remove) {
                                                        arr.0.remove(idx);
                                                    }
                                                });
                                            },
                                            "Remove"
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                div {
                    class: "array-item",
                    div {
                        class: "add-field-widget",
                        button {

                            onclick: move |_| {
                                root.with_mut(|data| {
                                    if let Some(Data::Array(arr)) = data.get_at_path_mut(&path_for_add) {
                                        let new_item = create_array_item_default(arr);
                                        arr.0.push(new_item);
                                    }
                                });
                            },
                            "+ Add Item"
                        }
                    }
                }


            }
        }
    }
}

// ============================================================================
// Object Field Editor (enables recursion)
// ============================================================================

/// Object field - iterates fields and renders child EditableDataView for each
#[component]
fn EditableObjectField(
    root: Signal<Data<'static>>,
    path: String,
    did: String,
    #[props(default)] remove_button: Option<Element>,
) -> Element {
    let path_for_memo = path.clone();
    let field_keys = use_memo(move || {
        root.read()
            .get_at_path(&path_for_memo)
            .and_then(|d| d.as_object())
            .map(|obj| obj.0.keys().cloned().collect::<Vec<_>>())
            .unwrap_or_default()
    });

    let is_root = path.is_empty();

    rsx! {
        if !is_root {
            div { class: "record-section object-section",
                div { class: "section-header",
                    div { class: "section-label",
                        {
                            let parts: Vec<&str> = path.split('.').collect();
                            let final_part = parts.last().unwrap_or(&"");
                            rsx! { "{final_part}" }
                        }
                    }
                    {remove_button}
                }
                div { class: "section-content",
                    for key in field_keys() {
                    {
                        let field_path = if path.is_empty() {
                            key.to_string()
                        } else {
                            format!("{}.{}", path, key)
                        };
                        let is_type_field = key == "$type";

                        rsx! {
                            FieldWithRemove {
                                key: "{field_path}",
                                root: root,
                                path: field_path.clone(),
                                did: did.clone(),
                                is_removable: !is_type_field,
                                parent_path: path.clone(),
                                field_key: key.clone(),
                            }
                        }
                    }
                    }

                    AddFieldWidget { root: root, path: path.clone() }
                }
            }
        } else {
            for key in field_keys() {
                {
                    let field_path = key.to_string();
                    let is_type_field = key == "$type";

                    rsx! {
                        FieldWithRemove {
                            key: "{field_path}",
                            root: root,
                            path: field_path.clone(),
                            did: did.clone(),
                            is_removable: !is_type_field,
                            parent_path: path.clone(),
                            field_key: key.clone(),
                        }
                    }
                }
            }

            AddFieldWidget { root: root, path: path.clone() }
        }
    }
}

/// Widget for adding new fields to objects
#[component]
fn AddFieldWidget(root: Signal<Data<'static>>, path: String) -> Element {
    let mut field_name = use_signal(|| String::new());
    let mut field_value = use_signal(|| String::new());
    let mut error = use_signal(|| None::<String>);
    let mut show_form = use_signal(|| false);

    let path_for_enter = path.clone();
    let path_for_button = path.clone();

    rsx! {
        div { class: "add-field-widget",
            if !show_form() {
                button {
                    class: "add-button",
                    onclick: move |_| show_form.set(true),
                    "+ Add Field"
                }
            } else {
                div { class: "add-field-form",
                    input {
                        r#type: "text",
                        placeholder: "Field name",
                        value: "{field_name}",
                        oninput: move |evt| field_name.set(evt.value()),
                    }
                    input {
                        r#type: "text",
                        placeholder: r#"Value: {{}}, [], true, 123, "text""#,
                        value: "{field_value}",
                        oninput: move |evt| field_value.set(evt.value()),
                        onkeydown: move |evt| {
                            use dioxus::prelude::keyboard_types::Key;
                            if evt.key() == Key::Enter {
                                let name = field_name();
                                let value_text = field_value();

                                if name.is_empty() {
                                    error.set(Some("Field name required".to_string()));
                                    return;
                                }

                                let new_value = match infer_data_from_text(&value_text) {
                                    Ok(data) => data,
                                    Err(e) => {
                                        error.set(Some(e));
                                        return;
                                    }
                                };

                                let mut new_data = root.read().clone();
                                if let Some(Data::Object(obj)) = new_data.get_at_path_mut(path_for_enter.as_str()) {
                                    obj.0.insert(name.into(), new_value);
                                }
                                root.set(new_data);

                                // Reset form
                                field_name.set(String::new());
                                field_value.set(String::new());
                                show_form.set(false);
                                error.set(None);
                            }
                        }
                    }
                    button {
                        class: "add-field-widget-edit",
                        onclick: move |_| {
                            let name = field_name();
                            let value_text = field_value();

                            if name.is_empty() {
                                error.set(Some("Field name required".to_string()));
                                return;
                            }

                            let new_value = match infer_data_from_text(&value_text) {
                                Ok(data) => data,
                                Err(e) => {
                                    error.set(Some(e));
                                    return;
                                }
                            };

                            let mut new_data = root.read().clone();
                            if let Some(Data::Object(obj)) = new_data.get_at_path_mut(path_for_button.as_str()) {
                                obj.0.insert(name.into(), new_value);
                            }
                            root.set(new_data);

                            // Reset form
                            field_name.set(String::new());
                            field_value.set(String::new());
                            show_form.set(false);
                            error.set(None);
                        },
                        "Add"
                    }
                    button {
                        class: "add-field-widget-edit",
                        onclick: move |_| {
                            show_form.set(false);
                            field_name.set(String::new());
                            field_value.set(String::new());
                            error.set(None);
                        },
                        "Cancel"
                    }
                    if let Some(err) = error() {
                        div { class: "field-error", "❌ {err}" }
                    }
                }
            }
        }
    }
}

/// Layout component for record view - handles header, metadata, and wraps children
#[component]
fn RecordViewLayout(uri: AtUri<'static>, cid: Option<Cid<'static>>, children: Element) -> Element {
    rsx! {
        div {
            class: "record-metadata",
            div { class: "metadata-row",
                span { class: "metadata-label", "URI" }
                span { class: "metadata-value",
                    HighlightedUri { uri: uri.clone() }
                }
            }
            if let Some(cid) = cid {
                div { class: "metadata-row",
                    span { class: "metadata-label", "CID" }
                    code { class: "metadata-value", "{cid}" }
                }
            }
        }

        {children}

    }
}

/// Render some text as markdown.
#[component]
fn EditableRecordContent(
    record_value: Data<'static>,
    uri: ReadSignal<AtUri<'static>>,
    view_mode: Signal<ViewMode>,
    edit_mode: Signal<bool>,
    record_resource: Resource<Result<GetRecordOutput<'static>, AgentError>>,
    schema: ReadSignal<Option<LexiconDoc<'static>>>,
) -> Element {
    let mut edit_data = use_signal(use_reactive!(|record_value| record_value.clone()));
    let nsid = use_memo(move || edit_data().type_discriminator().map(|s| s.to_string()));
    let navigator = use_navigator();
    let fetcher = use_context::<CachedFetcher>();

    let update_fetcher = fetcher.clone();
    let create_fetcher = fetcher.clone();
    let replace_fetcher = fetcher.clone();
    let delete_fetcher = fetcher.clone();

    rsx! {
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
            button {
                class: if view_mode() == ViewMode::Schema { "tab-button active" } else { "tab-button" },
                onclick: move |_| view_mode.set(ViewMode::Schema),
                "Schema"
            }
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
                                        .record(data.clone())
                                        .build();

                                    match fetcher.send(request).await {
                                        Ok(output) => {
                                            if output.status() == StatusCode::OK {
                                                dioxus_logger::tracing::info!("Record updated successfully");
                                                edit_data.set(data.clone());
                                                edit_mode.set(false);
                                            } else {
                                                dioxus_logger::tracing::error!("Unexpected status code: {:?}", output.status());
                                            }
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
                                                let link = format!("{}/record/{}", crate::env::WEAVER_APP_DOMAIN, output.uri);
                                                nav.push(link);
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
                                                            dioxus_logger::tracing::warn!("Created new record but failed to delete old: {:?}", e);
                                                        }
                                                    }
                                                }

                                                dioxus_logger::tracing::info!("Record replaced: {}", create_output.uri);
                                                let link = format!("{}/record/{}", crate::env::WEAVER_APP_DOMAIN, create_output.uri);
                                                nav.push(link);
                                            }
                                        }
                                        Err(e) => {
                                            dioxus_logger::tracing::error!("Failed to replace record: {:?}", e);
                                        }
                                    }
                                }
                            }
                        }
                    });
                },
                on_delete: move |_| {
                    let fetcher = delete_fetcher.clone();
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
                                            dioxus_logger::tracing::info!("Record deleted");
                                            nav.push(Route::Home {});
                                        }
                                        Err(e) => {
                                            dioxus_logger::tracing::error!("Failed to delete record: {:?}", e);
                                        }
                                    }
                                }
                            }
                        }
                    });
                },
                on_cancel: move |_| {
                    edit_data.set(record_value.clone());
                    edit_mode.set(false);
                },
            }
        }
        div {
            class: "tab-content",
            match view_mode() {
                ViewMode::Pretty => rsx! {
                    div { class: "pretty-record",
                        EditableDataView {
                            root: edit_data,
                            path: String::new(),
                            did: uri().authority().to_string(),
                        }
                    }
                },
                ViewMode::Json => rsx! {
                    JsonEditor { data: edit_data, nsid, schema }
                },
                ViewMode::Schema => rsx! {
                    SchemaView { schema }
                },
            }
        }
    }
}

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
