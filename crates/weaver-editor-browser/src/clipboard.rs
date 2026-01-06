//! Browser clipboard implementation.
//!
//! Implements `ClipboardPlatform` for browser environments using the
//! ClipboardEvent's DataTransfer API for sync access and the async
//! Clipboard API for custom MIME types.

use weaver_editor_core::ClipboardPlatform;

/// Browser clipboard context wrapping a ClipboardEvent's DataTransfer.
///
/// Created from a clipboard event (copy, cut, paste) to provide sync
/// clipboard access. Also spawns async tasks for custom MIME types.
pub struct BrowserClipboard {
    data_transfer: Option<web_sys::DataTransfer>,
}

impl BrowserClipboard {
    /// Create from a ClipboardEvent.
    ///
    /// Call this in your copy/cut/paste event handler.
    pub fn from_event(evt: &web_sys::ClipboardEvent) -> Self {
        Self {
            data_transfer: evt.clipboard_data(),
        }
    }

    /// Create an empty clipboard context (for testing or non-event contexts).
    pub fn empty() -> Self {
        Self {
            data_transfer: None,
        }
    }
}

impl ClipboardPlatform for BrowserClipboard {
    fn write_text(&self, text: &str) {
        // Sync write via DataTransfer (immediate fallback).
        if let Some(dt) = &self.data_transfer {
            if let Err(e) = dt.set_data("text/plain", text) {
                tracing::warn!("Clipboard sync write failed: {:?}", e);
            }
        }

        // Async write for custom MIME type (enables internal paste detection).
        let text = text.to_string();
        wasm_bindgen_futures::spawn_local(async move {
            if let Err(e) = crate::events::write_clipboard_with_custom_type(&text).await {
                tracing::debug!("Clipboard async write failed: {:?}", e);
            }
        });
    }

    fn write_html(&self, html: &str, plain_text: &str) {
        // Sync write of plain text fallback.
        if let Some(dt) = &self.data_transfer {
            if let Err(e) = dt.set_data("text/plain", plain_text) {
                tracing::warn!("Clipboard sync write (plain) failed: {:?}", e);
            }
        }

        // Async write for HTML.
        let html = html.to_string();
        let plain = plain_text.to_string();
        wasm_bindgen_futures::spawn_local(async move {
            if let Err(e) = write_html_to_clipboard(&html, &plain).await {
                tracing::warn!("Clipboard HTML write failed: {:?}", e);
            }
        });
    }

    fn read_text(&self) -> Option<String> {
        let dt = self.data_transfer.as_ref()?;

        // Try our custom MIME type first (internal paste).
        if let Ok(text) = dt.get_data("text/x-weaver-md") {
            if !text.is_empty() {
                return Some(text);
            }
        }

        // Fall back to plain text.
        dt.get_data("text/plain").ok().filter(|s| !s.is_empty())
    }
}

/// Write HTML and plain text to clipboard using the async Clipboard API.
///
/// This uses the navigator.clipboard API and doesn't require a clipboard event.
/// Suitable for keyboard-triggered copy operations like CopyAsHtml.
pub async fn write_html_to_clipboard(
    html: &str,
    plain_text: &str,
) -> Result<(), wasm_bindgen::JsValue> {
    use js_sys::{Array, Object, Reflect};
    use wasm_bindgen::JsValue;
    use web_sys::{Blob, BlobPropertyBag, ClipboardItem};

    let window = web_sys::window().ok_or_else(|| JsValue::from_str("no window"))?;
    let clipboard = window.navigator().clipboard();

    // Create HTML blob.
    let html_parts = Array::new();
    html_parts.push(&JsValue::from_str(html));
    let html_opts = BlobPropertyBag::new();
    html_opts.set_type("text/html");
    let html_blob = Blob::new_with_str_sequence_and_options(&html_parts, &html_opts)?;

    // Create plain text blob.
    let text_parts = Array::new();
    text_parts.push(&JsValue::from_str(plain_text));
    let text_opts = BlobPropertyBag::new();
    text_opts.set_type("text/plain");
    let text_blob = Blob::new_with_str_sequence_and_options(&text_parts, &text_opts)?;

    // Create ClipboardItem with both types.
    let item_data = Object::new();
    Reflect::set(&item_data, &JsValue::from_str("text/html"), &html_blob)?;
    Reflect::set(&item_data, &JsValue::from_str("text/plain"), &text_blob)?;

    let clipboard_item = ClipboardItem::new_with_record_from_str_to_blob_promise(&item_data)?;
    let items = Array::new();
    items.push(&clipboard_item);

    wasm_bindgen_futures::JsFuture::from(clipboard.write(&items)).await?;
    tracing::debug!("Wrote {} bytes of HTML to clipboard", html.len());
    Ok(())
}

// === Dioxus event handlers ===

/// Handle a Dioxus paste event.
///
/// Extracts text from the clipboard event and inserts at cursor.
#[cfg(feature = "dioxus")]
pub fn handle_paste<D: weaver_editor_core::EditorDocument>(
    evt: dioxus_core::Event<dioxus_html::ClipboardData>,
    doc: &mut D,
) {
    use dioxus_web::WebEventExt;
    use wasm_bindgen::JsCast;

    evt.prevent_default();

    let base_evt = evt.as_web_event();
    if let Some(clipboard_evt) = base_evt.dyn_ref::<web_sys::ClipboardEvent>() {
        let clipboard = BrowserClipboard::from_event(clipboard_evt);
        weaver_editor_core::clipboard_paste(doc, &clipboard);
    } else {
        tracing::warn!("[PASTE] Failed to cast to ClipboardEvent");
    }
}

/// Handle a Dioxus cut event.
///
/// Copies selection to clipboard, then deletes it.
#[cfg(feature = "dioxus")]
pub fn handle_cut<D: weaver_editor_core::EditorDocument>(
    evt: dioxus_core::Event<dioxus_html::ClipboardData>,
    doc: &mut D,
) {
    use dioxus_web::WebEventExt;
    use wasm_bindgen::JsCast;

    evt.prevent_default();

    let base_evt = evt.as_web_event();
    if let Some(clipboard_evt) = base_evt.dyn_ref::<web_sys::ClipboardEvent>() {
        let clipboard = BrowserClipboard::from_event(clipboard_evt);
        weaver_editor_core::clipboard_cut(doc, &clipboard);
    }
}

/// Handle a Dioxus copy event.
///
/// Copies selection to clipboard. Only prevents default if there was a selection.
#[cfg(feature = "dioxus")]
pub fn handle_copy<D: weaver_editor_core::EditorDocument>(
    evt: dioxus_core::Event<dioxus_html::ClipboardData>,
    doc: &D,
) {
    use dioxus_web::WebEventExt;
    use wasm_bindgen::JsCast;

    let base_evt = evt.as_web_event();
    if let Some(clipboard_evt) = base_evt.dyn_ref::<web_sys::ClipboardEvent>() {
        let clipboard = BrowserClipboard::from_event(clipboard_evt);
        if weaver_editor_core::clipboard_copy(doc, &clipboard) {
            evt.prevent_default();
        }
    }
}
