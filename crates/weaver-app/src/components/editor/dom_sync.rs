//! DOM synchronization for the markdown editor.
//!
//! Handles syncing cursor/selection state between the browser DOM and our
//! internal document model, and updating paragraph DOM elements.

use dioxus::prelude::*;

use super::document::{EditorDocument, Selection};
use super::offset_map::{SnapDirection, find_nearest_valid_position, is_valid_cursor_position};
use super::paragraph::ParagraphRender;

/// Sync internal cursor and selection state from browser DOM selection.
///
/// The optional `direction_hint` is used when snapping cursor from invisible content.
/// Pass `SnapDirection::Backward` for left/up arrow keys, `SnapDirection::Forward` for right/down.
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub fn sync_cursor_from_dom(
    doc: &mut EditorDocument,
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
    doc: &mut EditorDocument,
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

/// Convert a DOM position (node + offset) to a rope char offset using offset maps.
///
/// The `direction_hint` is used when snapping from invisible content to determine
/// which direction to prefer.
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub fn dom_position_to_text_offset(
    dom_document: &web_sys::Document,
    editor_element: &web_sys::Element,
    node: &web_sys::Node,
    offset_in_text_node: usize,
    paragraphs: &[ParagraphRender],
    direction_hint: Option<SnapDirection>,
) -> Option<usize> {
    use wasm_bindgen::JsCast;

    // Find the containing element with a node ID (walk up from text node)
    let mut current_node = node.clone();
    let node_id = loop {
        if let Some(element) = current_node.dyn_ref::<web_sys::Element>() {
            if element == editor_element {
                // Selection is on the editor container itself (e.g., Cmd+A select all)
                // Return boundary position based on offset:
                // offset 0 = start of editor, offset == child count = end of editor
                let child_count = editor_element.child_element_count() as usize;
                if offset_in_text_node == 0 {
                    return Some(0); // Start of document
                } else if offset_in_text_node >= child_count {
                    // End of document - find last paragraph's end
                    return paragraphs.last().map(|p| p.char_range.end);
                }
                break None;
            }

            let id = element
                .get_attribute("id")
                .or_else(|| element.get_attribute("data-node-id"));

            if let Some(id) = id {
                if id.starts_with('n') && id[1..].parse::<usize>().is_ok() {
                    break Some(id);
                }
            }
        }

        current_node = current_node.parent_node()?;
    };

    let node_id = node_id?;

    let container = dom_document.get_element_by_id(&node_id).or_else(|| {
        let selector = format!("[data-node-id='{}']", node_id);
        dom_document.query_selector(&selector).ok().flatten()
    })?;

    // Calculate UTF-16 offset from start of container to the position
    let mut utf16_offset_in_container = 0;

    if let Ok(walker) = dom_document.create_tree_walker_with_what_to_show(&container, 4) {
        while let Ok(Some(text_node)) = walker.next_node() {
            if &text_node == node {
                utf16_offset_in_container += offset_in_text_node;
                break;
            }

            if let Some(text) = text_node.text_content() {
                utf16_offset_in_container += text.encode_utf16().count();
            }
        }
    }

    for para in paragraphs {
        for mapping in &para.offset_map {
            if mapping.node_id == node_id {
                let mapping_start = mapping.char_offset_in_node;
                let mapping_end = mapping.char_offset_in_node + mapping.utf16_len;

                if utf16_offset_in_container >= mapping_start
                    && utf16_offset_in_container <= mapping_end
                {
                    let offset_in_mapping = utf16_offset_in_container - mapping_start;
                    let char_offset = mapping.char_range.start + offset_in_mapping;

                    // Check if this position is valid (not on invisible content)
                    if is_valid_cursor_position(&para.offset_map, char_offset) {
                        return Some(char_offset);
                    }

                    // Position is on invisible content, snap to nearest valid
                    if let Some(snapped) =
                        find_nearest_valid_position(&para.offset_map, char_offset, direction_hint)
                    {
                        return Some(snapped.char_offset());
                    }

                    // Fallback to original if no snap target
                    return Some(char_offset);
                }
            }
        }
    }

    // No mapping found - try to find any valid position in paragraphs
    // This handles clicks on non-text elements like images
    for para in paragraphs {
        if let Some(snapped) =
            find_nearest_valid_position(&para.offset_map, para.char_range.start, direction_hint)
        {
            return Some(snapped.char_offset());
        }
    }

    None
}

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub fn sync_cursor_from_dom(
    _document: &mut EditorDocument,
    _editor_id: &str,
    _paragraphs: &[ParagraphRender],
) {
}

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub fn sync_cursor_from_dom_with_direction(
    _document: &mut EditorDocument,
    _editor_id: &str,
    _paragraphs: &[ParagraphRender],
    _direction_hint: Option<SnapDirection>,
) {
}

/// Update paragraph DOM elements incrementally.
///
/// Only modifies paragraphs that changed (by comparing source_hash).
/// Browser preserves cursor naturally in unchanged paragraphs.
///
/// Returns true if the paragraph containing the cursor was updated.
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub fn update_paragraph_dom(
    editor_id: &str,
    old_paragraphs: &[ParagraphRender],
    new_paragraphs: &[ParagraphRender],
    cursor_offset: usize,
) -> bool {
    use wasm_bindgen::JsCast;

    let window = match web_sys::window() {
        Some(w) => w,
        None => return false,
    };

    let document = match window.document() {
        Some(d) => d,
        None => return false,
    };

    let editor = match document.get_element_by_id(editor_id) {
        Some(e) => e,
        None => return false,
    };

    // Find which paragraph contains cursor
    // Use end-inclusive matching: cursor at position N belongs to paragraph (0..N)
    // This handles typing at end of paragraph, which is the common case
    // The empty paragraph at document end catches any trailing cursor positions
    let cursor_para_idx = new_paragraphs
        .iter()
        .position(|p| p.char_range.start <= cursor_offset && cursor_offset <= p.char_range.end);

    let mut cursor_para_updated = false;

    for (idx, new_para) in new_paragraphs.iter().enumerate() {
        let para_id = format!("para-{}", idx);

        if let Some(old_para) = old_paragraphs.get(idx) {
            if new_para.source_hash != old_para.source_hash {
                // Changed - clear and update innerHTML
                // We clear first to ensure any browser-added content (from IME composition,
                // contenteditable quirks, etc.) is fully removed before setting new content
                if let Some(elem) = document.get_element_by_id(&para_id) {
                    elem.set_text_content(None); // Clear completely
                    elem.set_inner_html(&new_para.html);
                }

                if Some(idx) == cursor_para_idx {
                    cursor_para_updated = true;
                }
            }
        } else {
            if let Ok(div) = document.create_element("div") {
                div.set_id(&para_id);
                div.set_inner_html(&new_para.html);
                let _ = editor.append_child(&div);
            }

            if Some(idx) == cursor_para_idx {
                cursor_para_updated = true;
            }
        }
    }

    // Remove extra paragraphs if document got shorter
    // Also mark cursor as needing restoration since structure changed
    if new_paragraphs.len() < old_paragraphs.len() {
        cursor_para_updated = true;
    }
    for idx in new_paragraphs.len()..old_paragraphs.len() {
        let para_id = format!("para-{}", idx);
        if let Some(elem) = document.get_element_by_id(&para_id) {
            let _ = elem.remove();
        }
    }

    cursor_para_updated
}

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub fn update_paragraph_dom(
    _editor_id: &str,
    _old_paragraphs: &[ParagraphRender],
    _new_paragraphs: &[ParagraphRender],
    _cursor_offset: usize,
) -> bool {
    false
}
