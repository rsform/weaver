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

use super::actions::{EditorAction, Range, execute_action};
use super::document::EditorDocument;
use super::platform::Platform;

// Custom wasm_bindgen binding for StaticRange since web-sys doesn't expose it.
// StaticRange is returned by InputEvent.getTargetRanges() and represents
// a fixed range that doesn't update when the DOM changes.
// https://developer.mozilla.org/en-US/docs/Web/API/StaticRange
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
mod static_range {
    use wasm_bindgen::prelude::*;

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
}

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub use static_range::StaticRange;

/// Input types from the beforeinput event.
///
/// See: https://w3c.github.io/input-events/#interface-InputEvent-Attributes
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub enum InputType {
    // === Insertion ===
    /// Insert typed text.
    InsertText,
    /// Insert text from IME composition.
    InsertCompositionText,
    /// Insert a line break (`<br>`, Shift+Enter).
    InsertLineBreak,
    /// Insert a paragraph break (Enter).
    InsertParagraph,
    /// Insert from paste operation.
    InsertFromPaste,
    /// Insert from drop operation.
    InsertFromDrop,
    /// Insert replacement text (e.g., spell check correction).
    InsertReplacementText,
    /// Insert from voice input or other source.
    InsertFromYank,
    /// Insert a horizontal rule.
    InsertHorizontalRule,
    /// Insert an ordered list.
    InsertOrderedList,
    /// Insert an unordered list.
    InsertUnorderedList,
    /// Insert a link.
    InsertLink,

    // === Deletion ===
    /// Delete content backward (Backspace).
    DeleteContentBackward,
    /// Delete content forward (Delete key).
    DeleteContentForward,
    /// Delete word backward (Ctrl/Alt+Backspace).
    DeleteWordBackward,
    /// Delete word forward (Ctrl/Alt+Delete).
    DeleteWordForward,
    /// Delete to soft line boundary backward.
    DeleteSoftLineBackward,
    /// Delete to soft line boundary forward.
    DeleteSoftLineForward,
    /// Delete to hard line boundary backward (Cmd+Backspace on Mac).
    DeleteHardLineBackward,
    /// Delete to hard line boundary forward (Cmd+Delete on Mac).
    DeleteHardLineForward,
    /// Delete by cut operation.
    DeleteByCut,
    /// Delete by drag operation.
    DeleteByDrag,
    /// Generic content deletion.
    DeleteContent,
    /// Delete entire word backward (Ctrl+W on some systems).
    DeleteEntireWordBackward,
    /// Delete entire word forward.
    DeleteEntireWordForward,

    // === History ===
    /// Undo.
    HistoryUndo,
    /// Redo.
    HistoryRedo,

    // === Formatting (rarely used, most apps handle via shortcuts) ===
    FormatBold,
    FormatItalic,
    FormatUnderline,
    FormatStrikethrough,
    FormatSuperscript,
    FormatSubscript,

    // === Unknown ===
    /// Unrecognized input type.
    Unknown(String),
}

#[allow(dead_code)]
impl InputType {
    /// Parse from the browser's inputType string.
    pub fn from_str(s: &str) -> Self {
        match s {
            // Insertion
            "insertText" => Self::InsertText,
            "insertCompositionText" => Self::InsertCompositionText,
            "insertLineBreak" => Self::InsertLineBreak,
            "insertParagraph" => Self::InsertParagraph,
            "insertFromPaste" => Self::InsertFromPaste,
            "insertFromDrop" => Self::InsertFromDrop,
            "insertReplacementText" => Self::InsertReplacementText,
            "insertFromYank" => Self::InsertFromYank,
            "insertHorizontalRule" => Self::InsertHorizontalRule,
            "insertOrderedList" => Self::InsertOrderedList,
            "insertUnorderedList" => Self::InsertUnorderedList,
            "insertLink" => Self::InsertLink,

            // Deletion
            "deleteContentBackward" => Self::DeleteContentBackward,
            "deleteContentForward" => Self::DeleteContentForward,
            "deleteWordBackward" => Self::DeleteWordBackward,
            "deleteWordForward" => Self::DeleteWordForward,
            "deleteSoftLineBackward" => Self::DeleteSoftLineBackward,
            "deleteSoftLineForward" => Self::DeleteSoftLineForward,
            "deleteHardLineBackward" => Self::DeleteHardLineBackward,
            "deleteHardLineForward" => Self::DeleteHardLineForward,
            "deleteByCut" => Self::DeleteByCut,
            "deleteByDrag" => Self::DeleteByDrag,
            "deleteContent" => Self::DeleteContent,
            "deleteEntireSoftLine" => Self::DeleteSoftLineBackward, // Treat as soft line
            "deleteEntireWordBackward" => Self::DeleteEntireWordBackward,
            "deleteEntireWordForward" => Self::DeleteEntireWordForward,

            // History
            "historyUndo" => Self::HistoryUndo,
            "historyRedo" => Self::HistoryRedo,

            // Formatting
            "formatBold" => Self::FormatBold,
            "formatItalic" => Self::FormatItalic,
            "formatUnderline" => Self::FormatUnderline,
            "formatStrikethrough" => Self::FormatStrikethrough,
            "formatSuperscript" => Self::FormatSuperscript,
            "formatSubscript" => Self::FormatSubscript,

            // Unknown
            other => Self::Unknown(other.to_string()),
        }
    }

    /// Whether this input type is a deletion operation.
    pub fn is_deletion(&self) -> bool {
        matches!(
            self,
            Self::DeleteContentBackward
                | Self::DeleteContentForward
                | Self::DeleteWordBackward
                | Self::DeleteWordForward
                | Self::DeleteSoftLineBackward
                | Self::DeleteSoftLineForward
                | Self::DeleteHardLineBackward
                | Self::DeleteHardLineForward
                | Self::DeleteByCut
                | Self::DeleteByDrag
                | Self::DeleteContent
                | Self::DeleteEntireWordBackward
                | Self::DeleteEntireWordForward
        )
    }

    /// Whether this input type is an insertion operation.
    pub fn is_insertion(&self) -> bool {
        matches!(
            self,
            Self::InsertText
                | Self::InsertCompositionText
                | Self::InsertLineBreak
                | Self::InsertParagraph
                | Self::InsertFromPaste
                | Self::InsertFromDrop
                | Self::InsertReplacementText
                | Self::InsertFromYank
        )
    }
}

/// Result of handling a beforeinput event.
#[derive(Debug, Clone)]
#[allow(dead_code)]
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
        fallback_action: EditorAction,
    },
}

/// Context for beforeinput handling.
#[allow(dead_code)]
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

/// Handle a beforeinput event.
///
/// This is the main entry point for beforeinput-based input handling.
/// Returns whether the event was handled and default should be prevented.
#[allow(dead_code)]
pub fn handle_beforeinput(
    doc: &mut EditorDocument,
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
                // Simple text insert - update model, let browser handle DOM
                // This mirrors the simple delete handling: we track in model,
                // browser handles visual update, DOM sync skips innerHTML for
                // cursor paragraph when syntax is unchanged
                let action = EditorAction::Insert {
                    text: text.clone(),
                    range,
                };
                execute_action(doc, &action);
                tracing::trace!(
                    text_len = text.len(),
                    range_start = range.start,
                    range_end = range.end,
                    cursor_after = doc.cursor.read().offset,
                    "insertText: updated model, PassThrough to browser"
                );
                BeforeInputResult::PassThrough
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
                // Complex delete - we handle everything, prevent browser default
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
                BeforeInputResult::PassThrough
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
                let action = EditorAction::DeleteForward { range };
                execute_action(doc, &action);
                BeforeInputResult::Handled
            } else {
                // Simple single-char delete - track in model, let browser handle DOM
                if range.start < doc.len_chars() {
                    let _ = doc.remove_tracked(range.start, 1);
                    doc.selection.set(None);
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
fn get_current_range(doc: &EditorDocument) -> Range {
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
