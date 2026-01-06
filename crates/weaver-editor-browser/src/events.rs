//! Browser event handling for the editor.
//!
//! Provides browser-specific event extraction and input type parsing for
//! the `beforeinput` event and other DOM events.

use wasm_bindgen::prelude::*;
use weaver_editor_core::{InputType, ParagraphRender, Range};

use crate::dom_sync::dom_position_to_text_offset;
use crate::platform::Platform;

// === StaticRange binding ===
//
// Custom wasm_bindgen binding for StaticRange since web-sys doesn't expose it.
// StaticRange is returned by InputEvent.getTargetRanges() and represents
// a fixed range that doesn't update when the DOM changes.

#[wasm_bindgen]
extern "C" {
    /// The StaticRange interface represents a static range of text in the DOM.
    pub type StaticRange;

    #[wasm_bindgen(method, getter, structural)]
    pub fn startContainer(this: &StaticRange) -> web_sys::Node;

    #[wasm_bindgen(method, getter, structural)]
    pub fn startOffset(this: &StaticRange) -> u32;

    #[wasm_bindgen(method, getter, structural)]
    pub fn endContainer(this: &StaticRange) -> web_sys::Node;

    #[wasm_bindgen(method, getter, structural)]
    pub fn endOffset(this: &StaticRange) -> u32;

    #[wasm_bindgen(method, getter, structural)]
    pub fn collapsed(this: &StaticRange) -> bool;
}

// === InputType browser parsing ===

/// Parse a browser inputType string to an InputType enum.
///
/// This handles the W3C Input Events inputType values as returned by
/// `InputEvent.inputType` in browsers.
pub fn parse_browser_input_type(s: &str) -> InputType {
    match s {
        // Insertion
        "insertText" => InputType::InsertText,
        "insertCompositionText" => InputType::InsertCompositionText,
        "insertLineBreak" => InputType::InsertLineBreak,
        "insertParagraph" => InputType::InsertParagraph,
        "insertFromPaste" => InputType::InsertFromPaste,
        "insertFromDrop" => InputType::InsertFromDrop,
        "insertReplacementText" => InputType::InsertReplacementText,
        "insertFromYank" => InputType::InsertFromYank,
        "insertHorizontalRule" => InputType::InsertHorizontalRule,
        "insertOrderedList" => InputType::InsertOrderedList,
        "insertUnorderedList" => InputType::InsertUnorderedList,
        "insertLink" => InputType::InsertLink,

        // Deletion
        "deleteContentBackward" => InputType::DeleteContentBackward,
        "deleteContentForward" => InputType::DeleteContentForward,
        "deleteWordBackward" => InputType::DeleteWordBackward,
        "deleteWordForward" => InputType::DeleteWordForward,
        "deleteSoftLineBackward" => InputType::DeleteSoftLineBackward,
        "deleteSoftLineForward" => InputType::DeleteSoftLineForward,
        "deleteHardLineBackward" => InputType::DeleteHardLineBackward,
        "deleteHardLineForward" => InputType::DeleteHardLineForward,
        "deleteByCut" => InputType::DeleteByCut,
        "deleteByDrag" => InputType::DeleteByDrag,
        "deleteContent" => InputType::DeleteContent,
        "deleteEntireSoftLine" => InputType::DeleteSoftLineBackward,
        "deleteEntireWordBackward" => InputType::DeleteEntireWordBackward,
        "deleteEntireWordForward" => InputType::DeleteEntireWordForward,

        // History
        "historyUndo" => InputType::HistoryUndo,
        "historyRedo" => InputType::HistoryRedo,

        // Formatting
        "formatBold" => InputType::FormatBold,
        "formatItalic" => InputType::FormatItalic,
        "formatUnderline" => InputType::FormatUnderline,
        "formatStrikethrough" => InputType::FormatStrikethrough,
        "formatSuperscript" => InputType::FormatSuperscript,
        "formatSubscript" => InputType::FormatSubscript,

        // Unknown
        other => InputType::Unknown(other.to_string()),
    }
}

// === BeforeInput event handling ===

/// Result of handling a beforeinput event.
#[derive(Debug, Clone)]
pub enum BeforeInputResult {
    /// Event was handled, prevent default browser behavior.
    Handled,
    /// Event should be handled by browser (e.g., during composition).
    PassThrough,
    /// Event was handled but requires async follow-up (e.g., paste).
    HandledAsync,
    /// Android backspace workaround: defer and check if browser handled it.
    DeferredCheck {
        /// The action to execute if browser didn't handle it.
        fallback_action: weaver_editor_core::EditorAction,
    },
}

/// Context for beforeinput handling.
pub struct BeforeInputContext<'a> {
    /// The input type.
    pub input_type: InputType,
    /// The data (text to insert, if any).
    pub data: Option<String>,
    /// Target range from getTargetRanges(), if available.
    /// This is the range the browser wants to modify.
    pub target_range: Option<Range>,
    /// Whether the event is part of an IME composition.
    pub is_composing: bool,
    /// Platform info for quirks handling.
    pub platform: &'a Platform,
}

/// Extract target range from a beforeinput event.
///
/// Uses getTargetRanges() to get the browser's intended range for this operation.
pub fn get_target_range_from_event(
    event: &web_sys::InputEvent,
    editor_id: &str,
    paragraphs: &[ParagraphRender],
) -> Option<Range> {
    use wasm_bindgen::JsCast;

    let ranges = event.get_target_ranges();
    if ranges.length() == 0 {
        return None;
    }

    let static_range: StaticRange = ranges.get(0).unchecked_into();

    let window = web_sys::window()?;
    let dom_document = window.document()?;
    let editor_element = dom_document.get_element_by_id(editor_id)?;

    let start_container = static_range.startContainer();
    let start_offset = static_range.startOffset() as usize;
    let end_container = static_range.endContainer();
    let end_offset = static_range.endOffset() as usize;

    let start = dom_position_to_text_offset(
        &dom_document,
        &editor_element,
        &start_container,
        start_offset,
        paragraphs,
        None,
    )?;

    let end = dom_position_to_text_offset(
        &dom_document,
        &editor_element,
        &end_container,
        end_offset,
        paragraphs,
        None,
    )?;

    Some(Range::new(start, end))
}

/// Get data from a beforeinput event, handling different sources.
pub fn get_data_from_event(event: &web_sys::InputEvent) -> Option<String> {
    // First try the data property.
    if let Some(data) = event.data() {
        if !data.is_empty() {
            return Some(data);
        }
    }

    // For paste/drop, try dataTransfer.
    if let Some(data_transfer) = event.data_transfer() {
        if let Ok(text) = data_transfer.get_data("text/plain") {
            if !text.is_empty() {
                return Some(text);
            }
        }
    }

    None
}

/// Get input type from a beforeinput event.
pub fn get_input_type_from_event(event: &web_sys::InputEvent) -> InputType {
    parse_browser_input_type(&event.input_type())
}

/// Check if the beforeinput event is during IME composition.
pub fn is_composing(event: &web_sys::InputEvent) -> bool {
    event.is_composing()
}

// === Clipboard helpers ===

/// Write text to clipboard with both text/plain and custom MIME type.
pub async fn write_clipboard_with_custom_type(text: &str) -> Result<(), JsValue> {
    use js_sys::{Array, Object, Reflect};
    use web_sys::{Blob, BlobPropertyBag, ClipboardItem};

    let window = web_sys::window().ok_or_else(|| JsValue::from_str("no window"))?;
    let navigator = window.navigator();
    let clipboard = navigator.clipboard();

    let text_parts = Array::new();
    text_parts.push(&JsValue::from_str(text));

    let text_opts = BlobPropertyBag::new();
    text_opts.set_type("text/plain");
    let text_blob = Blob::new_with_str_sequence_and_options(&text_parts, &text_opts)?;

    let custom_opts = BlobPropertyBag::new();
    custom_opts.set_type("text/markdown");
    let custom_blob = Blob::new_with_str_sequence_and_options(&text_parts, &custom_opts)?;

    let item_data = Object::new();
    Reflect::set(&item_data, &JsValue::from_str("text/plain"), &text_blob)?;
    Reflect::set(
        &item_data,
        &JsValue::from_str("text/markdown"),
        &custom_blob,
    )?;

    let clipboard_item = ClipboardItem::new_with_record_from_str_to_blob_promise(&item_data)?;
    let items = Array::new();
    items.push(&clipboard_item);

    let promise = clipboard.write(&items);
    wasm_bindgen_futures::JsFuture::from(promise).await?;

    Ok(())
}

/// Read text from clipboard.
pub async fn read_clipboard_text() -> Result<Option<String>, JsValue> {
    let window = web_sys::window().ok_or_else(|| JsValue::from_str("no window"))?;
    let navigator = window.navigator();
    let clipboard = navigator.clipboard();

    let promise = clipboard.read_text();
    let result: JsValue = wasm_bindgen_futures::JsFuture::from(promise).await?;

    Ok(result.as_string())
}

/// Copy markdown as rendered HTML to clipboard.
///
/// Renders the markdown to HTML and writes both text/html and text/plain
/// representations to the clipboard.
pub async fn copy_as_html(markdown: &str) -> Result<(), JsValue> {
    use js_sys::{Array, Object, Reflect};
    use web_sys::{Blob, BlobPropertyBag, ClipboardItem};

    // Render markdown to HTML using ClientWriter.
    let parser = weaver_editor_core::markdown_weaver::Parser::new(markdown).into_offset_iter();
    let mut html = String::new();
    weaver_editor_core::weaver_renderer::atproto::ClientWriter::<_, _, ()>::new(
        parser, &mut html, markdown,
    )
    .run()
    .map_err(|e| JsValue::from_str(&format!("render error: {e}")))?;

    let window = web_sys::window().ok_or_else(|| JsValue::from_str("no window"))?;
    let clipboard = window.navigator().clipboard();

    // Create blobs for both HTML and plain text.
    let parts = Array::new();
    parts.push(&JsValue::from_str(&html));

    let html_opts = BlobPropertyBag::new();
    html_opts.set_type("text/html");
    let html_blob = Blob::new_with_str_sequence_and_options(&parts, &html_opts)?;

    let text_opts = BlobPropertyBag::new();
    text_opts.set_type("text/plain");
    let text_blob = Blob::new_with_str_sequence_and_options(&parts, &text_opts)?;

    // Create ClipboardItem with both types.
    let item_data = Object::new();
    Reflect::set(&item_data, &JsValue::from_str("text/html"), &html_blob)?;
    Reflect::set(&item_data, &JsValue::from_str("text/plain"), &text_blob)?;

    let clipboard_item = ClipboardItem::new_with_record_from_str_to_blob_promise(&item_data)?;
    let items = Array::new();
    items.push(&clipboard_item);

    wasm_bindgen_futures::JsFuture::from(clipboard.write(&items)).await?;
    tracing::info!("[COPY HTML] Success - {} bytes of HTML", html.len());
    Ok(())
}

// === BeforeInput handler ===

use weaver_editor_core::{EditorAction, EditorDocument, execute_action};

/// Handle a beforeinput event, dispatching to the appropriate action.
///
/// This is the main entry point for beforeinput-based input handling.
/// The `current_range` parameter should be the current cursor/selection range
/// from the document when `ctx.target_range` is None.
///
/// Returns the handling result indicating whether default should be prevented.
pub fn handle_beforeinput<D: EditorDocument>(
    doc: &mut D,
    ctx: &BeforeInputContext<'_>,
    current_range: Range,
) -> BeforeInputResult {
    // During composition, let the browser handle most things.
    if ctx.is_composing {
        match ctx.input_type {
            InputType::HistoryUndo | InputType::HistoryRedo => {
                // Handle undo/redo even during composition.
            }
            InputType::InsertCompositionText => {
                return BeforeInputResult::PassThrough;
            }
            _ => {
                return BeforeInputResult::PassThrough;
            }
        }
    }

    // Use target range from event, or fall back to current range.
    let range = ctx.target_range.unwrap_or(current_range);

    match ctx.input_type {
        // === Insertion ===
        InputType::InsertText => {
            if let Some(ref text) = ctx.data {
                let action = EditorAction::Insert {
                    text: text.clone(),
                    range,
                };
                execute_action(doc, &action);
                BeforeInputResult::Handled
            } else {
                BeforeInputResult::PassThrough
            }
        }

        InputType::InsertLineBreak => {
            let action = EditorAction::InsertLineBreak { range };
            execute_action(doc, &action);
            BeforeInputResult::Handled
        }

        InputType::InsertParagraph => {
            let action = EditorAction::InsertParagraph { range };
            execute_action(doc, &action);
            BeforeInputResult::Handled
        }

        InputType::InsertFromPaste | InputType::InsertReplacementText => {
            if let Some(ref text) = ctx.data {
                let action = EditorAction::Insert {
                    text: text.clone(),
                    range,
                };
                execute_action(doc, &action);
                BeforeInputResult::Handled
            } else {
                BeforeInputResult::PassThrough
            }
        }

        InputType::InsertFromDrop => BeforeInputResult::PassThrough,

        InputType::InsertCompositionText => BeforeInputResult::PassThrough,

        // === Deletion ===
        InputType::DeleteContentBackward => {
            // Android Chrome workaround: backspace sometimes doesn't work properly.
            if ctx.platform.android && ctx.platform.chrome && range.is_caret() {
                let action = EditorAction::DeleteBackward { range };
                return BeforeInputResult::DeferredCheck {
                    fallback_action: action,
                };
            }

            let action = EditorAction::DeleteBackward { range };
            execute_action(doc, &action);
            BeforeInputResult::Handled
        }

        InputType::DeleteContentForward => {
            let action = EditorAction::DeleteForward { range };
            execute_action(doc, &action);
            BeforeInputResult::Handled
        }

        InputType::DeleteWordBackward | InputType::DeleteEntireWordBackward => {
            let action = EditorAction::DeleteWordBackward { range };
            execute_action(doc, &action);
            BeforeInputResult::Handled
        }

        InputType::DeleteWordForward | InputType::DeleteEntireWordForward => {
            let action = EditorAction::DeleteWordForward { range };
            execute_action(doc, &action);
            BeforeInputResult::Handled
        }

        InputType::DeleteSoftLineBackward => {
            let action = EditorAction::DeleteSoftLineBackward { range };
            execute_action(doc, &action);
            BeforeInputResult::Handled
        }

        InputType::DeleteSoftLineForward => {
            let action = EditorAction::DeleteSoftLineForward { range };
            execute_action(doc, &action);
            BeforeInputResult::Handled
        }

        InputType::DeleteHardLineBackward => {
            let action = EditorAction::DeleteToLineStart { range };
            execute_action(doc, &action);
            BeforeInputResult::Handled
        }

        InputType::DeleteHardLineForward => {
            let action = EditorAction::DeleteToLineEnd { range };
            execute_action(doc, &action);
            BeforeInputResult::Handled
        }

        InputType::DeleteByCut => {
            if !range.is_caret() {
                let action = EditorAction::DeleteBackward { range };
                execute_action(doc, &action);
            }
            BeforeInputResult::Handled
        }

        InputType::DeleteByDrag | InputType::DeleteContent => {
            if !range.is_caret() {
                let action = EditorAction::DeleteBackward { range };
                execute_action(doc, &action);
            }
            BeforeInputResult::Handled
        }

        // === History ===
        InputType::HistoryUndo => {
            execute_action(doc, &EditorAction::Undo);
            BeforeInputResult::Handled
        }

        InputType::HistoryRedo => {
            execute_action(doc, &EditorAction::Redo);
            BeforeInputResult::Handled
        }

        // === Formatting ===
        InputType::FormatBold => {
            execute_action(doc, &EditorAction::ToggleBold);
            BeforeInputResult::Handled
        }

        InputType::FormatItalic => {
            execute_action(doc, &EditorAction::ToggleItalic);
            BeforeInputResult::Handled
        }

        InputType::FormatStrikethrough => {
            execute_action(doc, &EditorAction::ToggleStrikethrough);
            BeforeInputResult::Handled
        }

        // === Not handled ===
        InputType::InsertFromYank
        | InputType::InsertHorizontalRule
        | InputType::InsertOrderedList
        | InputType::InsertUnorderedList
        | InputType::InsertLink
        | InputType::FormatUnderline
        | InputType::FormatSuperscript
        | InputType::FormatSubscript
        | InputType::Unknown(_) => BeforeInputResult::PassThrough,
    }
}
