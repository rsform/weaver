use crate::components::accordion::{Accordion, AccordionContent, AccordionItem, AccordionTrigger};
use crate::record_utils::{get_errors_at_exact_path, get_expected_string_format, get_hex_rep};
use dioxus::prelude::*;
use humansize::format_size;
use jacquard::to_data;
use jacquard::types::string::AtprotoStr;
use jacquard::{
    common::{Data, IntoStatic},
    types::{aturi::AtUri, cid::Cid},
};
use jacquard_lexicon::lexicon::LexiconDoc;
use jacquard_lexicon::validation::ValidationResult;
use weaver_renderer::{code_pretty::highlight_code, css::generate_default_css};

#[derive(Clone, Copy, PartialEq)]
pub enum ViewMode {
    Pretty,
    Json,
    Schema,
}

/// Layout component for record view - handles header, metadata, and wraps children
#[component]
pub fn RecordViewLayout(
    uri: AtUri<'static>,
    cid: Option<Cid<'static>>,
    schema: ReadSignal<Option<LexiconDoc<'static>>>,
    record_value: Data<'static>,
    children: Element,
) -> Element {
    // Validate the record if schema is available
    let validation_status = use_memo(move || {
        let _schema_doc = schema()?;
        let nsid_str = record_value.type_discriminator()?;

        let validator = jacquard_lexicon::validation::SchemaValidator::global();
        let result = validator.validate_by_nsid(nsid_str, &record_value);

        Some(result.is_valid())
    });

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
            if let Some(is_valid) = validation_status() {
                div { class: "metadata-row",
                    span { class: "metadata-label", "Schema" }
                    span {
                        class: if is_valid { "metadata-value schema-valid" } else { "metadata-value schema-invalid" },
                        if is_valid { "Valid" } else { "Invalid" }
                    }
                }
            }
        }

        {children}

    }
}

#[component]
pub fn SchemaView(schema: ReadSignal<Option<LexiconDoc<'static>>>) -> Element {
    if let Some(schema_doc) = schema() {
        // Convert LexiconDoc to Data for display
        let schema_data = to_data(&schema_doc).ok().map(|d| d.into_static());

        if let Some(data) = schema_data {
            rsx! {
                div {
                    class: "pretty-record",
                    DataView { data: data.clone(), root_data: data, path: String::new(), did: String::new() }
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

#[component]
pub fn DataView(
    data: Data<'static>,
    root_data: ReadSignal<Data<'static>>,
    path: String,
    did: String,
) -> Element {
    // Try to get validation result from context and get errors exactly at this path
    let validation_result = try_use_context::<Signal<Option<ValidationResult>>>();

    let errors = if let Some(vr_signal) = validation_result {
        get_errors_at_exact_path(&*vr_signal.read(), &path)
    } else {
        Vec::new()
    };

    let has_errors = !errors.is_empty();

    match &data {
        Data::Null => rsx! {
            div { class: if has_errors { "record-field field-error" } else { "record-field" },
                PathLabel { path: path.clone() }
                span { class: "field-value muted", "null" }
                if has_errors {
                    for error in &errors {
                        div { class: "field-error-message", "{error}" }
                    }
                }
            }
        },
        Data::Boolean(b) => rsx! {
            div { class: if has_errors { "record-field field-error" } else { "record-field" },
                PathLabel { path: path.clone() }
                span { class: "field-value", "{b}" }
                if has_errors {
                    for error in &errors {
                        div { class: "field-error-message", "{error}" }
                    }
                }
            }
        },
        Data::Integer(i) => rsx! {
            div { class: if has_errors { "record-field field-error" } else { "record-field" },
                PathLabel { path: path.clone() }
                span { class: "field-value", "{i}" }
                if has_errors {
                    for error in &errors {
                        div { class: "field-error-message", "{error}" }
                    }
                }
            }
        },
        Data::String(s) => {
            use jacquard::types::string::AtprotoStr;
            use jacquard_lexicon::lexicon::LexStringFormat;

            // Get expected format from schema
            let expected_format = get_expected_string_format(&*root_data.read(), &path);

            // Get actual type from data
            let actual_type_label = match s {
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

            // Prefer schema format if available, otherwise use actual type
            let type_label = if let Some(fmt) = expected_format {
                match fmt {
                    LexStringFormat::Datetime => "datetime",
                    LexStringFormat::Uri => "uri",
                    LexStringFormat::AtUri => "at-uri",
                    LexStringFormat::Did => "did",
                    LexStringFormat::Handle => "handle",
                    LexStringFormat::AtIdentifier => "at-identifier",
                    LexStringFormat::Nsid => "nsid",
                    LexStringFormat::Cid => "cid",
                    LexStringFormat::Language => "language",
                    LexStringFormat::Tid => "tid",
                    LexStringFormat::RecordKey => "record-key",
                }
            } else {
                actual_type_label
            };

            rsx! {
                div { class: if has_errors { "record-field field-error" } else { "record-field" },
                    PathLabel { path: path.clone() }
                    span { class: "field-value",

                        HighlightedString { string_type: s.clone() }
                        if type_label != "string" {
                            span { class: "string-type-tag", " [{type_label}]" }
                        }
                    }
                    if has_errors {
                        for error in &errors {
                            div { class: "field-error-message", "{error}" }
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
                div { class: if has_errors { "record-field field-error" } else { "record-field" },
                    PathLabel { path: path.clone() }
                    pre { class: "field-value bytes", "{hex_string} [{byte_size}]" }
                    if has_errors {
                        for error in &errors {
                            div { class: "field-error-message", "{error}" }
                        }
                    }
                }
            }
        }
        Data::CidLink(cid) => rsx! {
            div { class: if has_errors { "record-field field-error" } else { "record-field" },
                span { class: "field-label", "{path}" }
                span { class: "field-value", "{cid}" }
                if has_errors {
                    for error in &errors {
                        div { class: "field-error-message", "{error}" }
                    }
                }
            }
        },
        Data::Array(arr) => {
            let label = path.split('.').last().unwrap_or(&path);
            rsx! {
                div { class: "record-section",
                    Accordion {
                        id: "array-{path}",
                        collapsible: true,
                        AccordionItem {
                            default_open: true,
                            index: 0,
                            AccordionTrigger {
                                div { class: "section-label", "{label}" span { class: "array-len", "[{arr.len()}]" } }
                            }
                            AccordionContent {
                                if has_errors {
                                    for error in &errors {
                                        div { class: "field-error-message", "{error}" }
                                    }
                                }
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
                                                                root_data,
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
                                                        root_data,
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
                        if has_errors {
                            for error in &errors {
                                div { class: "field-error-message", "{error}" }
                            }
                        }
                        for (key, value) in obj.iter() {
                            {
                                let new_path = if is_root {
                                    key.to_string()
                                } else {
                                    format!("{}.{}", path, key)
                                };
                                let did_clone = did.clone();
                                rsx! {
                                    DataView { data: value.clone(), root_data, path: new_path, did: did_clone }
                                }
                            }
                        }
                    }
                }
            } else {
                // Nested object (not array item): wrap in section
                let label = path.split('.').last().unwrap_or(&path);
                rsx! {
                    div { class: "record-section",
                        Accordion {
                            id: "object-{path}",
                            collapsible: true,
                            AccordionItem {
                                default_open: true,
                                index: 0,
                                AccordionTrigger {
                                    div { class: "section-label", "{label}" }
                                }
                                AccordionContent {
                                    if has_errors {
                                        for error in &errors {
                                            div { class: "field-error-message", "{error}" }
                                        }
                                    }
                                    div { class: "section-content",
                                        for (key, value) in obj.iter() {
                                            {
                                                let new_path = format!("{}.{}", path, key);
                                                let did_clone = did.clone();
                                                rsx! {
                                                    DataView { data: value.clone(), root_data, path: new_path, did: did_clone }
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
pub fn HighlightedUri(uri: AtUri<'static>) -> Element {
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
pub fn HighlightedString(string_type: AtprotoStr<'static>) -> Element {
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
pub fn PrettyRecordView(
    record: Data<'static>,
    uri: AtUri<'static>,
    schema: ReadSignal<Option<LexiconDoc<'static>>>,
) -> Element {
    let did = uri.authority().to_string();
    let root_data = use_signal(|| record.clone());

    // Validate the record and provide via context - only after schema is loaded
    let mut validation_result = use_signal(|| None);
    use_effect(move || {
        // Wait for schema to be loaded
        if schema().is_some() {
            if let Some(nsid_str) = root_data.read().type_discriminator() {
                let validator = jacquard_lexicon::validation::SchemaValidator::global();
                let result = validator.validate_by_nsid(nsid_str, &*root_data.read());
                validation_result.set(Some(result));
            }
        }
    });
    use_context_provider(|| validation_result);

    rsx! {
        div {
            class: "pretty-record",
            DataView { data: record, root_data, path: String::new(), did }
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

#[component]
pub fn PathLabel(path: String) -> Element {
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
