use crate::fetch::CachedFetcher;
use dioxus::prelude::*;
use humansize::format_size;
use jacquard::{
    client::AgentSessionExt,
    common::{Data, IntoStatic},
    smol_str::SmolStr,
    types::aturi::AtUri,
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
    let record = use_resource(move || {
        let client = fetcher.get_client();

        async move { client.fetch_record_slingshot(&uri()).await }
    });
    if let Some(Ok(record)) = &*record.read_unchecked() {
        let record_value = record.value.clone().into_static();
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
                }
                div {
                    class: "tab-content",
                    match view_mode() {
                        ViewMode::Pretty => rsx! {
                            PrettyRecordView { record: record_value.clone(), uri: uri().clone() }
                        },
                        ViewMode::Json => rsx! {
                            CodeView {
                                code: use_signal(|| json.clone()),
                                lang: Some("json".to_string()),
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
