//! BeforeInput event handling for the editor.
//!
//! This module provides the primary input handling via the `beforeinput` event,
//! which gives us semantic information about what the browser wants to do
//! (insert text, delete backward, etc.) rather than raw key codes.
//!
//! ## Browser Support
//!
//! `beforeinput` is well-supported in modern browsers, but has quirks:
//! - Android: `getTargetRanges()` can be unreliable during composition
//! - Safari: Some input types may not fire or have wrong data
//! - All: `isComposing` flag behavior varies
//!
//! We handle these with platform-specific workarounds inherited from the
//! battle-tested patterns in ProseMirror.

use dioxus::prelude::*;

use super::actions::{EditorAction, execute_action};
use super::document::SignalEditorDocument;
use super::platform::Platform;

// Re-export types from extracted crates.
pub use weaver_editor_browser::{BeforeInputContext, BeforeInputResult};
pub use weaver_editor_core::{InputType, Range};

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub use weaver_editor_browser::StaticRange;

/// Handle a beforeinput event.
///
/// This is the main entry point for beforeinput-based input handling.
/// Returns whether the event was handled and default should be prevented.
#[allow(dead_code)]
pub fn handle_beforeinput(
    doc: &mut SignalEditorDocument,
    ctx: BeforeInputContext<'_>,
) -> BeforeInputResult {
    // During composition, let the browser handle most things.
    // We'll commit the final text in compositionend.
    if ctx.is_composing {
        match ctx.input_type {
            // These can happen during composition but should still be handled
            InputType::HistoryUndo | InputType::HistoryRedo => {
                // Handle undo/redo even during composition
            }
            InputType::InsertCompositionText => {
                // Let browser handle composition preview
                return BeforeInputResult::PassThrough;
            }
            _ => {
                // Let browser handle
                return BeforeInputResult::PassThrough;
            }
        }
    }

    // Get the range to operate on
    let range = ctx.target_range.unwrap_or_else(|| get_current_range(doc));

    match ctx.input_type {
        // === Insertion ===
        InputType::InsertText => {
            if let Some(text) = ctx.data {
                use super::FORCE_INNERHTML_UPDATE;

                let action = EditorAction::Insert {
                    text: text.clone(),
                    range,
                };
                execute_action(doc, &action);

                // Log model content after insert to detect ZWC contamination
                if tracing::enabled!(tracing::Level::TRACE) {
                    let content = doc.content();
                    tracing::trace!(
                        text_len = text.len(),
                        range_start = range.start,
                        range_end = range.end,
                        cursor_after = doc.cursor.read().offset,
                        model_len = content.len(),
                        model_chars = content.chars().count(),
                        model_content = %content.escape_debug(),
                        force_innerhtml = FORCE_INNERHTML_UPDATE,
                        "insertText: updated model"
                    );
                }

                // When FORCE_INNERHTML_UPDATE is true, dom_sync will always replace
                // innerHTML. We must preventDefault to avoid browser's default action
                // racing with our innerHTML update and causing double-insertion.
                if FORCE_INNERHTML_UPDATE {
                    BeforeInputResult::Handled
                } else {
                    // PassThrough: browser handles DOM, we just track in model.
                    // dom_sync will skip innerHTML for cursor paragraph when syntax unchanged.
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
            // For paste, we need the data from the event or clipboard
            if let Some(text) = ctx.data {
                let action = EditorAction::Insert { text, range };
                execute_action(doc, &action);
                BeforeInputResult::Handled
            } else {
                // No data in event - need to handle via clipboard API
                BeforeInputResult::PassThrough
            }
        }

        InputType::InsertFromDrop => {
            // Let browser handle drops for now
            BeforeInputResult::PassThrough
        }

        InputType::InsertCompositionText => {
            // Should be caught by is_composing check above, but just in case
            BeforeInputResult::PassThrough
        }

        // === Deletion ===
        InputType::DeleteContentBackward => {
            // Android Chrome workaround: backspace sometimes doesn't work properly
            // after uneditable nodes. Use deferred check pattern from ProseMirror.
            // BUT only for caret deletions - selections we handle directly since
            // the browser might only delete one char instead of the whole selection.
            if ctx.platform.android && ctx.platform.chrome && range.is_caret() {
                let action = EditorAction::DeleteBackward { range };
                return BeforeInputResult::DeferredCheck {
                    fallback_action: action,
                };
            }

            // Check if this delete requires special handling (newlines, zero-width chars)
            // If not, let browser handle DOM while we just track in model
            let needs_special_handling = if !range.is_caret() {
                // Selection delete - we handle to ensure consistency
                true
            } else if range.start == 0 {
                // At start of document, nothing to delete
                false
            } else {
                // Check what char we're deleting
                let prev_char = super::input::get_char_at(doc.loro_text(), range.start - 1);
                matches!(prev_char, Some('\n') | Some('\u{200C}') | Some('\u{200B}'))
            };

            if needs_special_handling {
                // Handle fully when: complex delete OR when dom_sync will replace innerHTML
                // (FORCE_INNERHTML_UPDATE). PassThrough + innerHTML causes double-deletion.
                let action = EditorAction::DeleteBackward { range };
                execute_action(doc, &action);
                BeforeInputResult::Handled
            } else {
                // Simple single-char delete - track in model, let browser handle DOM
                tracing::debug!(
                    range_start = range.start,
                    "deleteContentBackward: simple delete, will PassThrough to browser"
                );
                if range.start > 0 {
                    let _ = doc.remove_tracked(range.start - 1, 1);
                    doc.cursor.write().offset = range.start - 1;
                    doc.selection.set(None);
                }
                tracing::debug!("deleteContentBackward: after model update, returning PassThrough");
                if super::FORCE_INNERHTML_UPDATE {
                    BeforeInputResult::Handled
                } else {
                    BeforeInputResult::PassThrough
                }
            }
        }

        InputType::DeleteContentForward => {
            // Check if this delete requires special handling
            let needs_special_handling = if !range.is_caret() {
                true
            } else if range.start >= doc.len_chars() {
                false
            } else {
                let next_char = super::input::get_char_at(doc.loro_text(), range.start);
                matches!(next_char, Some('\n') | Some('\u{200C}') | Some('\u{200B}'))
            };

            if needs_special_handling {
                // Handle fully when: complex delete OR when dom_sync will replace innerHTML
                let action = EditorAction::DeleteForward { range };
                execute_action(doc, &action);
                BeforeInputResult::Handled
            } else {
                // Simple single-char delete - track in model, let browser handle DOM
                if range.start < doc.len_chars() {
                    let _ = doc.remove_tracked(range.start, 1);
                    doc.selection.set(None);
                }
                if super::FORCE_INNERHTML_UPDATE {
                    BeforeInputResult::Handled
                } else {
                    BeforeInputResult::PassThrough
                }
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
            // Cut is handled separately via clipboard events
            // But we should delete the selection here
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

        // === Other ===
        InputType::InsertFromYank
        | InputType::InsertHorizontalRule
        | InputType::InsertOrderedList
        | InputType::InsertUnorderedList
        | InputType::InsertLink
        | InputType::FormatUnderline
        | InputType::FormatSuperscript
        | InputType::FormatSubscript
        | InputType::Unknown(_) => {
            // Not handled - let browser do its thing or ignore
            BeforeInputResult::PassThrough
        }
    }
}

/// Get the current range based on cursor and selection state.
fn get_current_range(doc: &SignalEditorDocument) -> Range {
    if let Some(sel) = *doc.selection.read() {
        let (start, end) = (sel.anchor.min(sel.head), sel.anchor.max(sel.head));
        Range::new(start, end)
    } else {
        Range::caret(doc.cursor.read().offset)
    }
}

/// Extract target range from a beforeinput event.
///
/// Uses getTargetRanges() to get the browser's intended range for this operation.
/// This requires mapping DOM positions to document character offsets via paragraphs.
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub fn get_target_range_from_event(
    event: &web_sys::InputEvent,
    editor_id: &str,
    paragraphs: &[super::paragraph::ParagraphRender],
) -> Option<Range> {
    use super::dom_sync::dom_position_to_text_offset;
    use wasm_bindgen::JsCast;

    let ranges = event.get_target_ranges();
    if ranges.length() == 0 {
        return None;
    }

    // Get the first range (there's usually only one)
    // getTargetRanges returns an array of StaticRange objects
    let static_range: StaticRange = ranges.get(0).unchecked_into();

    let window = web_sys::window()?;
    let dom_document = window.document()?;
    let editor_element = dom_document.get_element_by_id(editor_id)?;

    let start_container = static_range.startContainer();
    let start_offset = static_range.startOffset() as usize;
    let end_container = static_range.endContainer();
    let end_offset = static_range.endOffset() as usize;

    // Log raw DOM position for debugging
    let start_node_name = start_container.node_name();
    let start_text = start_container.text_content().unwrap_or_default();
    let end_node_name = end_container.node_name();
    let end_text = end_container.text_content().unwrap_or_default();

    // Check if containers are the editor element itself
    let start_is_editor = start_container
        .dyn_ref::<web_sys::Element>()
        .map(|e| e == &editor_element)
        .unwrap_or(false);
    let end_is_editor = end_container
        .dyn_ref::<web_sys::Element>()
        .map(|e| e == &editor_element)
        .unwrap_or(false);

    tracing::trace!(
        start_node_name = %start_node_name,
        start_offset,
        start_is_editor,
        start_text_preview = %start_text.chars().take(30).collect::<String>(),
        end_node_name = %end_node_name,
        end_offset,
        end_is_editor,
        end_text_preview = %end_text.chars().take(30).collect::<String>(),
        collapsed = static_range.collapsed(),
        "get_target_range_from_event: raw StaticRange from browser"
    );

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

    tracing::trace!(
        start,
        end,
        "get_target_range_from_event: computed text offsets"
    );

    Some(Range::new(start, end))
}

/// Get data from a beforeinput event, handling different sources.
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub fn get_data_from_event(event: &web_sys::InputEvent) -> Option<String> {
    // First try the data property
    if let Some(data) = event.data() {
        if !data.is_empty() {
            return Some(data);
        }
    }

    // For paste/drop, try dataTransfer
    if let Some(data_transfer) = event.data_transfer() {
        if let Ok(text) = data_transfer.get_data("text/plain") {
            if !text.is_empty() {
                return Some(text);
            }
        }
    }

    None
}
