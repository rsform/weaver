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

// === BeforeInput handler ===

use crate::FORCE_INNERHTML_UPDATE;
use weaver_editor_core::{EditorAction, EditorDocument, Selection, execute_action};

/// Get the current range (cursor or selection) from an EditorDocument.
///
/// This is a convenience helper for building `BeforeInputContext`.
pub fn get_current_range<D: EditorDocument>(doc: &D) -> Range {
    if let Some(sel) = doc.selection() {
        Range::new(sel.start(), sel.end())
    } else {
        Range::caret(doc.cursor_offset())
    }
}

/// Check if a character requires special delete handling.
///
/// Returns true for newlines and zero-width chars which need semantic handling
/// rather than simple char deletion.
fn needs_special_delete_handling(ch: Option<char>) -> bool {
    matches!(ch, Some('\n') | Some('\u{200C}') | Some('\u{200B}'))
}

/// Handle a beforeinput event, dispatching to the appropriate action.
///
/// This is the main entry point for beforeinput-based input handling.
/// The `current_range` parameter should be the current cursor/selection range
/// from the document when `ctx.target_range` is None.
///
/// Returns the handling result indicating whether default should be prevented.
///
/// # DOM Update Strategy
///
/// When [`FORCE_INNERHTML_UPDATE`] is `true`, this always returns `Handled`
/// and the caller should preventDefault. The DOM will be updated via innerHTML.
///
/// When `false`, simple operations (plain text insert, single char delete)
/// return `PassThrough` to let the browser update the DOM while we track
/// changes in the model. Complex operations still return `Handled`.
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

                // When FORCE_INNERHTML_UPDATE is false, we can let browser handle
                // DOM updates for simple text insertions while we just track in model.
                if FORCE_INNERHTML_UPDATE {
                    BeforeInputResult::Handled
                } else {
                    BeforeInputResult::PassThrough
                }
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

            // Check if this delete requires special handling.
            let needs_special = if !range.is_caret() {
                // Selection delete - always handle for consistency.
                true
            } else if range.start == 0 {
                // At start - nothing to delete.
                false
            } else {
                // Check what char we're deleting.
                needs_special_delete_handling(doc.char_at(range.start - 1))
            };

            if needs_special || FORCE_INNERHTML_UPDATE {
                // Complex delete or forced mode - use full action handler.
                let action = EditorAction::DeleteBackward { range };
                execute_action(doc, &action);
                BeforeInputResult::Handled
            } else {
                // Simple single-char delete - track in model, let browser handle DOM.
                if range.start > 0 {
                    doc.delete(range.start - 1..range.start);
                }
                BeforeInputResult::PassThrough
            }
        }

        InputType::DeleteContentForward => {
            // Check if this delete requires special handling.
            let needs_special = if !range.is_caret() {
                true
            } else if range.start >= doc.len_chars() {
                false
            } else {
                needs_special_delete_handling(doc.char_at(range.start))
            };

            if needs_special || FORCE_INNERHTML_UPDATE {
                let action = EditorAction::DeleteForward { range };
                execute_action(doc, &action);
                BeforeInputResult::Handled
            } else {
                // Simple delete forward.
                if range.start < doc.len_chars() {
                    doc.delete(range.start..range.start + 1);
                }
                BeforeInputResult::PassThrough
            }
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

// === Math click handling ===

use weaver_editor_core::{OffsetMapping, SyntaxSpanInfo};

/// Check if a click target is a math-clickable element.
///
/// Returns the target character offset if the click was on a `.math-clickable`
/// element with a valid `data-char-target` attribute, None otherwise.
pub fn get_math_click_offset(target: &web_sys::EventTarget) -> Option<usize> {
    use wasm_bindgen::JsCast;

    let element = target.dyn_ref::<web_sys::Element>()?;
    let math_el = element.closest(".math-clickable").ok()??;
    let char_target = math_el.get_attribute("data-char-target")?;
    char_target.parse().ok()
}

/// Handle a click that might be on a math element.
///
/// If the click target is a math-clickable element, this updates the cursor,
/// clears selection, updates visibility, and restores the DOM cursor position.
///
/// Returns true if the click was handled (was on a math element), false otherwise.
/// When this returns false, the caller should handle the click normally.
pub fn handle_math_click<D: EditorDocument>(
    target: &web_sys::EventTarget,
    doc: &mut D,
    syntax_spans: &[SyntaxSpanInfo],
    paragraphs: &[ParagraphRender],
    offset_map: &[OffsetMapping],
) -> bool {
    if let Some(offset) = get_math_click_offset(target) {
        tracing::debug!("math-clickable clicked, moving cursor to {}", offset);
        doc.set_cursor_offset(offset);
        doc.set_selection(None);
        crate::update_syntax_visibility(offset, None, syntax_spans, paragraphs);
        let _ = crate::restore_cursor_position(offset, offset_map, None);
        true
    } else {
        false
    }
}

// === Composition (IME) event handlers ===

/// Handle composition start event.
///
/// Clears any existing selection (composition replaces it) and sets up
/// composition state tracking.
#[cfg(feature = "dioxus")]
pub fn handle_compositionstart<D: EditorDocument>(
    evt: dioxus_core::Event<dioxus_html::CompositionData>,
    doc: &mut D,
) {
    let data = evt.data().data();
    tracing::trace!(data = %data, "compositionstart");

    // Delete selection if present (composition replaces it).
    if let Some(sel) = doc.selection() {
        let (start, end) = (sel.anchor.min(sel.head), sel.anchor.max(sel.head));
        tracing::trace!(start, end, "compositionstart: deleting selection");
        doc.delete(start..end);
        doc.set_cursor_offset(start);
        doc.set_selection(None);
    }

    let cursor_offset = doc.cursor_offset();
    tracing::trace!(cursor = cursor_offset, "compositionstart: setting composition state");
    doc.set_composition(Some(weaver_editor_core::CompositionState {
        start_offset: cursor_offset,
        text: data,
    }));
}

/// Handle composition update event.
///
/// Updates the composition text as the user types or selects IME suggestions.
#[cfg(feature = "dioxus")]
pub fn handle_compositionupdate<D: EditorDocument>(
    evt: dioxus_core::Event<dioxus_html::CompositionData>,
    doc: &mut D,
) {
    let data = evt.data().data();
    tracing::trace!(data = %data, "compositionupdate");

    if let Some(mut comp) = doc.composition() {
        comp.text = data;
        doc.set_composition(Some(comp));
    } else {
        tracing::debug!("compositionupdate without active composition state");
    }
}

/// Handle composition end event.
///
/// Finalizes the composition by inserting the final text into the document.
/// Also handles zero-width character cleanup that some IMEs leave behind.
#[cfg(feature = "dioxus")]
pub fn handle_compositionend<D: EditorDocument>(
    evt: dioxus_core::Event<dioxus_html::CompositionData>,
    doc: &mut D,
) {
    let final_text = evt.data().data();
    tracing::trace!(data = %final_text, "compositionend");

    // Record when composition ended for Safari timing workaround.
    doc.set_composition_ended_now();

    let comp = doc.composition();
    doc.set_composition(None);

    if let Some(comp) = comp {
        tracing::debug!(
            start_offset = comp.start_offset,
            final_text = %final_text,
            chars = final_text.chars().count(),
            "compositionend: inserting text"
        );

        if !final_text.is_empty() {
            // Clean up zero-width characters that IMEs sometimes leave behind.
            let mut delete_start = comp.start_offset;
            while delete_start > 0 {
                match doc.char_at(delete_start - 1) {
                    Some('\u{200C}') | Some('\u{200B}') => delete_start -= 1,
                    _ => break,
                }
            }

            let cursor_offset = doc.cursor_offset();
            let zw_count = cursor_offset - delete_start;

            if zw_count > 0 {
                // Splice: delete zero-width chars and insert new char in one op.
                doc.replace(delete_start..delete_start + zw_count, &final_text);
                doc.set_cursor_offset(delete_start + final_text.chars().count());
            } else if cursor_offset == doc.len_chars() {
                // Fast path: append at end.
                doc.push(&final_text);
                doc.set_cursor_offset(comp.start_offset + final_text.chars().count());
            } else {
                // Insert at cursor position.
                doc.insert(cursor_offset, &final_text);
                doc.set_cursor_offset(comp.start_offset + final_text.chars().count());
            }
        }
    } else {
        tracing::debug!("compositionend without active composition state");
    }
}
