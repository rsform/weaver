//! DOM synchronization for the markdown editor.
//!
//! Handles syncing cursor/selection state between the browser DOM and our
//! internal document model, and updating paragraph DOM elements.
//!
//! Most DOM sync logic is in `weaver_editor_browser`. This module provides
//! thin wrappers that work with `SignalEditorDocument` directly.

#[allow(unused_imports)]
use super::document::Selection;
#[allow(unused_imports)]
use super::document::SignalEditorDocument;
#[allow(unused_imports)]
use dioxus::prelude::*;
use weaver_editor_core::ParagraphRender;
#[allow(unused_imports)]
use weaver_editor_core::SnapDirection;

// Re-export from browser crate.
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub use weaver_editor_browser::{dom_position_to_text_offset, update_paragraph_dom};

/// Sync internal cursor and selection state from browser DOM selection.
///
/// The optional `direction_hint` is used when snapping cursor from invisible content.
/// Pass `SnapDirection::Backward` for left/up arrow keys, `SnapDirection::Forward` for right/down.
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub fn sync_cursor_from_dom(
    doc: &mut SignalEditorDocument,
    editor_id: &str,
    paragraphs: &[ParagraphRender],
) {
    sync_cursor_from_dom_with_direction(doc, editor_id, paragraphs, None);
}

/// Sync cursor with optional direction hint for snapping.
///
/// Use this when handling arrow keys to ensure cursor snaps in the expected direction.
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub fn sync_cursor_from_dom_with_direction(
    doc: &mut SignalEditorDocument,
    editor_id: &str,
    paragraphs: &[ParagraphRender],
    direction_hint: Option<SnapDirection>,
) {
    use wasm_bindgen::JsCast;

    // Early return if paragraphs not yet populated (first render edge case)
    if paragraphs.is_empty() {
        return;
    }

    let window = match web_sys::window() {
        Some(w) => w,
        None => return,
    };

    let dom_document = match window.document() {
        Some(d) => d,
        None => return,
    };

    let editor_element = match dom_document.get_element_by_id(editor_id) {
        Some(e) => e,
        None => return,
    };

    let selection = match window.get_selection() {
        Ok(Some(sel)) => sel,
        _ => return,
    };

    // Get both anchor (selection start) and focus (selection end) positions
    let anchor_node = match selection.anchor_node() {
        Some(node) => node,
        None => return,
    };
    let focus_node = match selection.focus_node() {
        Some(node) => node,
        None => return,
    };
    let anchor_offset = selection.anchor_offset() as usize;
    let focus_offset = selection.focus_offset() as usize;

    let anchor_rope = dom_position_to_text_offset(
        &dom_document,
        &editor_element,
        &anchor_node,
        anchor_offset,
        paragraphs,
        direction_hint,
    );
    let focus_rope = dom_position_to_text_offset(
        &dom_document,
        &editor_element,
        &focus_node,
        focus_offset,
        paragraphs,
        direction_hint,
    );

    match (anchor_rope, focus_rope) {
        (Some(anchor), Some(focus)) => {
            let old_offset = doc.cursor.read().offset;
            // Warn if cursor is jumping a large distance - likely a bug
            let jump = if focus > old_offset {
                focus - old_offset
            } else {
                old_offset - focus
            };
            if jump > 100 {
                tracing::warn!(
                    old_offset,
                    new_offset = focus,
                    jump,
                    "sync_cursor_from_dom: LARGE CURSOR JUMP detected"
                );
            }
            doc.cursor.write().offset = focus;
            if anchor != focus {
                doc.selection.set(Some(Selection {
                    anchor,
                    head: focus,
                }));
            } else {
                doc.selection.set(None);
            }
        }
        _ => {
            tracing::warn!("Could not map DOM selection to rope offsets");
        }
    }
}

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub fn sync_cursor_from_dom(
    _document: &mut SignalEditorDocument,
    _editor_id: &str,
    _paragraphs: &[ParagraphRender],
) {
}

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub fn sync_cursor_from_dom_with_direction(
    _document: &mut SignalEditorDocument,
    _editor_id: &str,
    _paragraphs: &[ParagraphRender],
    _direction_hint: Option<SnapDirection>,
) {
}
