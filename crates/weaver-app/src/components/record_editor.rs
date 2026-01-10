use crate::Route;
use crate::components::accordion::{Accordion, AccordionContent, AccordionItem, AccordionTrigger};
use crate::components::dialog::{DialogContent, DialogDescription, DialogRoot, DialogTitle};
use crate::components::record_view::{PathLabel, SchemaView, ViewMode};
use crate::fetch::Fetcher;
use crate::record_utils::{create_array_item_default, infer_data_from_text, try_parse_as_type};
use dioxus::prelude::{FormData, *};
use http::StatusCode;
use humansize::format_size;
use jacquard::api::com_atproto::repo::get_record::GetRecordOutput;
use jacquard::bytes::Bytes;
use jacquard::client::AgentError;
use jacquard::{atproto, prelude::*};
use jacquard::{
    client::AgentSessionExt,
    common::{Data, IntoStatic},
    types::{aturi::AtUri, ident::AtIdentifier, string::Nsid},
};
use jacquard_lexicon::lexicon::LexiconDoc;
use jacquard_lexicon::validation::ValidationResult;
use mime_sniffer::MimeTypeSniffer;
use weaver_api::com_atproto::repo::{
    create_record::CreateRecord, delete_record::DeleteRecord, put_record::PutRecord,
};
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
    let handle_input = move |evt: Event<FormData>| {
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

    let fetcher = use_context::<Fetcher>();
    let path_for_upload = path.clone();
    let handle_file = move |evt: Event<FormData>| {
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
                        let bytes = Bytes::from(bytes_data.to_vec());
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
    let handle_cid_change = move |evt: Event<FormData>| {
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
    let handle_size_change = move |evt: Event<FormData>| {
        let text = evt.value();
        size_input.set(text.clone());

        match text.parse::<usize>() {
            Ok(new_size) => {
                size_input.set(format_size(new_size, humansize::BINARY));
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
                let aspect_parent = query_match.path.rsplit_once('.').map(|(parent, _)| parent);

                // Check if they share the same parent
                if blob_parent == aspect_parent {
                    Some(query_match.path.clone())
                } else {
                    None
                }
            })
        })
    };

    // Update the aspectRatio if we found a matching field
    if let Some(aspect_path) = aspect_path_to_update {
        let aspect_obj = atproto! {{
            "width": width,
            "height": height
        }};

        root.with_mut(|record_data| {
            record_data.set_at_path(&aspect_path, aspect_obj);
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
    let handle_input = move |evt: Event<FormData>| {
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
fn parse_bytes_input(text: &str) -> Result<(Bytes, String), String> {
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
fn parse_hex_bytes(hex: &str) -> Result<Bytes, String> {
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

    Ok(Bytes::from(bytes))
}

/// Parse base64 string to bytes
fn parse_base64_bytes(b64: &str) -> Result<Bytes, String> {
    use base64::Engine;
    let engine = base64::engine::general_purpose::STANDARD;

    engine
        .decode(b64)
        .map(Bytes::from)
        .map_err(|e| format!("Invalid base64: {}", e))
}

/// Convert bytes to hex display string (with spacing every 4 chars)
fn bytes_to_hex(bytes: &Bytes) -> String {
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
    let handle_input = move |evt: Event<FormData>| {
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
            Accordion {
                id: "edit-array-{path}",
                collapsible: true,
                AccordionItem {
                    default_open: true,
                    index: 0,
                    AccordionTrigger {
                        div { class: "record-section-header",
                            div { class: "section-label",
                                {
                                    let parts: Vec<&str> = path.split('.').collect();
                                    let final_part = parts.last().unwrap_or(&"");
                                    rsx! { "{final_part}" }
                                }
                            }
                            span { class: "array-length", "[{array_len}]" }
                        }
                    }
                    AccordionContent {
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
                Accordion {
                    id: "edit-object-{path}",
                    collapsible: true,
                    AccordionItem {
                        default_open: true,
                        index: 0,
                        AccordionTrigger {
                            div { class: "record-section-header",
                                div { class: "section-label",
                                    {
                                        let parts: Vec<&str> = path.split('.').collect();
                                        let final_part = parts.last().unwrap_or(&"");
                                        rsx! { "{final_part}" }
                                    }
                                }
                                {remove_button}
                            }
                        }
                        AccordionContent {
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
                    }
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

#[component]
pub fn EditableRecordContent(
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
    let fetcher = use_context::<Fetcher>();

    // Validate edit_data whenever it changes and provide via context
    let mut validation_result = use_signal(|| None);
    use_effect(move || {
        let _ = schema(); // Track schema changes
        if let Some(nsid_str) = nsid() {
            let data = edit_data();
            let validator = jacquard_lexicon::validation::SchemaValidator::global();
            let result = validator.validate_by_nsid(&nsid_str, &data);
            validation_result.set(Some(result));
        }
    });
    use_context_provider(|| validation_result);

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
                                            if output.status() == StatusCode::OK.as_u16() {
                                                tracing::info!("Record updated successfully");
                                                edit_data.set(data.clone());
                                                edit_mode.set(false);
                                            } else {
                                                tracing::error!("Unexpected status code: {:?}", output.status());
                                            }
                                        }
                                        Err(e) => {
                                            tracing::error!("Failed to update record: {:?}", e);
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
                                                tracing::info!("Record created: {}", output.uri);
                                                let link = format!("{}/record/{}", crate::env::WEAVER_APP_HOST, output.uri);
                                                nav.push(link);
                                            }
                                        }
                                        Err(e) => {
                                            tracing::error!("Failed to create record: {:?}", e);
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
                                    // Create new record first - if this fails, user keeps their old record
                                    // If delete fails after, user has duplicates (recoverable) rather than data loss
                                    let create_req = CreateRecord::new()
                                        .repo(AtIdentifier::Did(did.clone()))
                                        .collection(new_collection)
                                        .record(data.clone())
                                        .build();

                                    match fetcher.send(create_req).await {
                                        Ok(response) => {
                                            if let Ok(create_output) = response.into_output() {
                                                // Delete old record after successful create
                                                if let (Some(old_collection_str), Some(old_rkey)) = (uri.collection(), uri.rkey()) {
                                                    let old_collection = Nsid::new(old_collection_str.as_str()).ok();
                                                    if let Some(old_collection) = old_collection {
                                                        let delete_req = DeleteRecord::new()
                                                            .repo(AtIdentifier::Did(did))
                                                            .collection(old_collection)
                                                            .rkey(old_rkey.clone())
                                                            .build();

                                                        if let Err(e) = fetcher.send(delete_req).await {
                                                            tracing::warn!("Created new record but failed to delete old: {:?}", e);
                                                        }
                                                    }
                                                }

                                                tracing::info!("Record replaced: {}", create_output.uri);
                                                let link = format!("{}/record/{}", crate::env::WEAVER_APP_HOST, create_output.uri);
                                                nav.push(link);
                                            }
                                        }
                                        Err(e) => {
                                            tracing::error!("Failed to replace record: {:?}", e);
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
                                            tracing::info!("Record deleted");
                                            nav.push(Route::Home {});
                                        }
                                        Err(e) => {
                                            tracing::error!("Failed to delete record: {:?}", e);
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
pub fn JsonEditor(
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
pub fn ActionButtons(
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
pub fn ValidationPanel(
    validation: Resource<Option<(Option<ValidationResult>, Option<String>)>>,
) -> Element {
    rsx! {
        div { class: "validation-panel",
            if let Some(Some((result_opt, parse_error_opt))) = validation.read().as_ref() {
                if let Some(parse_err) = parse_error_opt {
                    div { class: "parse-error",
                        "❌ Invalid JSON: {parse_err}"
                    }
                }

                if let Some(result) = result_opt {
                    // Structural validity
                    if result.is_structurally_valid() {
                        div { class: "validation-success", "✓ Structurally valid" }
                    } else {
                        div { class: "parse-error", "❌ Structurally invalid" }
                    }

                    // Overall validity
                    if result.is_valid() {
                        div { class: "validation-success", "✓ Fully valid" }
                    } else {
                        div { class: "validation-warning", "⚠ Has errors" }
                    }

                    // Show errors if any
                    if !result.is_valid() {
                        div { class: "validation-errors",
                            h4 { "Validation Errors:" }
                            for error in result.all_errors() {
                                div { class: "error", "{error}" }
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
