//! Cursor position restoration in the DOM.
//!
//! After re-rendering HTML, we need to restore the cursor to its original
//! position in the source text. This involves:
//! 1. Finding the offset mapping for the cursor's char position
//! 2. Getting the DOM element by node ID
//! 3. Walking text nodes to find the UTF-16 offset within the element
//! 4. Setting cursor with web_sys Selection API

use weaver_editor_core::OffsetMapping;
pub use weaver_editor_core::{CursorRect, SelectionRect};
#[cfg(all(target_family = "wasm", target_os = "unknown"))]
use weaver_editor_core::{SnapDirection, find_mapping_for_char, find_nearest_valid_position};

#[cfg(all(target_family = "wasm", target_os = "unknown"))]
use wasm_bindgen::JsCast;

/// Restore cursor position in the DOM after re-render.
#[cfg(all(target_family = "wasm", target_os = "unknown"))]
pub fn restore_cursor_position(
    char_offset: usize,
    offset_map: &[OffsetMapping],
    editor_id: &str,
    snap_direction: Option<SnapDirection>,
) -> Result<(), wasm_bindgen::JsValue> {
    // Empty document - no cursor to restore
    if offset_map.is_empty() {
        return Ok(());
    }

    // Bounds check using offset map
    let max_offset = offset_map
        .iter()
        .map(|m| m.char_range.end)
        .max()
        .unwrap_or(0);
    if char_offset > max_offset {
        tracing::warn!(
            "cursor offset {} > max mapping offset {}",
            char_offset,
            max_offset
        );
        // Don't error, just skip restoration - this can happen during edits
        return Ok(());
    }

    // Find mapping for this cursor position, snapping if needed
    let (mapping, char_offset) = match find_mapping_for_char(offset_map, char_offset) {
        Some((m, false)) => (m, char_offset), // Valid position, use as-is
        Some((m, true)) => {
            // Position is on invisible content, snap to nearest valid
            if let Some(snapped) =
                find_nearest_valid_position(offset_map, char_offset, snap_direction)
            {
                tracing::trace!(
                    target: "weaver::cursor",
                    original_offset = char_offset,
                    snapped_offset = snapped.char_offset(),
                    direction = ?snapped.snapped,
                    "snapping cursor from invisible content"
                );
                (snapped.mapping, snapped.char_offset())
            } else {
                // Fallback to original mapping if no valid snap target
                (m, char_offset)
            }
        }
        None => return Err("no mapping found for cursor offset".into()),
    };

    tracing::trace!(
        target: "weaver::cursor",
        char_offset,
        node_id = %mapping.node_id,
        mapping_range = ?mapping.char_range,
        child_index = ?mapping.child_index,
        "restoring cursor position"
    );

    // Get window and document
    let window = web_sys::window().ok_or("no window")?;
    let document = window.document().ok_or("no document")?;

    // Get the container element by node ID (try id attribute first, then data-node-id)
    let container = document
        .get_element_by_id(&mapping.node_id)
        .or_else(|| {
            let selector = format!("[data-node-id='{}']", mapping.node_id);
            document.query_selector(&selector).ok().flatten()
        })
        .ok_or_else(|| format!("element not found: {}", mapping.node_id))?;

    // Set selection using Range API
    let selection = window.get_selection()?.ok_or("no selection object")?;
    let range = document.create_range()?;

    // Check if this is an element-based position (e.g., after <br />)
    if let Some(child_index) = mapping.child_index {
        // Position cursor at child index in the element
        range.set_start(&container, child_index as u32)?;
    } else {
        // Position cursor in text content
        let container_element = container.dyn_into::<web_sys::HtmlElement>()?;
        let offset_in_range = char_offset - mapping.char_range.start;
        let target_utf16_offset = mapping.char_offset_in_node + offset_in_range;
        let (text_node, node_offset) =
            find_text_node_at_offset(&container_element, target_utf16_offset)?;
        range.set_start(&text_node, node_offset as u32)?;
    }

    range.collapse_with_to_start(true);

    selection.remove_all_ranges()?;
    selection.add_range(&range)?;

    Ok(())
}

/// Find text node at given UTF-16 offset within element.
///
/// Walks all text nodes in the container, accumulating their UTF-16 lengths
/// until we find the node containing the target offset.
/// Skips text nodes inside contenteditable="false" elements (like embeds).
///
/// Returns (text_node, offset_within_node).
#[cfg(all(target_family = "wasm", target_os = "unknown"))]
fn find_text_node_at_offset(
    container: &web_sys::HtmlElement,
    target_utf16_offset: usize,
) -> Result<(web_sys::Node, usize), wasm_bindgen::JsValue> {
    let document = web_sys::window()
        .ok_or("no window")?
        .document()
        .ok_or("no document")?;

    // Use SHOW_ALL to see element boundaries for tracking non-editable regions
    let walker = document.create_tree_walker_with_what_to_show(container, 0xFFFFFFFF)?;

    let mut accumulated_utf16 = 0;
    let mut last_node: Option<web_sys::Node> = None;
    let mut skip_until_exit: Option<web_sys::Element> = None;

    while let Some(node) = walker.next_node()? {
        // Check if we've exited the non-editable subtree
        if let Some(ref skip_elem) = skip_until_exit {
            if !skip_elem.contains(Some(&node)) {
                skip_until_exit = None;
            }
        }

        // Check if entering a non-editable element
        if skip_until_exit.is_none() {
            if let Some(element) = node.dyn_ref::<web_sys::Element>() {
                if element.get_attribute("contenteditable").as_deref() == Some("false") {
                    skip_until_exit = Some(element.clone());
                    continue;
                }
            }
        }

        // Skip everything inside non-editable regions
        if skip_until_exit.is_some() {
            continue;
        }

        // Only process text nodes
        if node.node_type() != web_sys::Node::TEXT_NODE {
            continue;
        }

        last_node = Some(node.clone());

        if let Some(text) = node.text_content() {
            let text_len = text.encode_utf16().count();

            // Found the node containing target offset
            if accumulated_utf16 + text_len >= target_utf16_offset {
                let offset_in_node = target_utf16_offset - accumulated_utf16;
                return Ok((node, offset_in_node));
            }

            accumulated_utf16 += text_len;
        }
    }

    // Fallback: return last node at its end
    // This handles cursor at end of document
    if let Some(node) = last_node {
        if let Some(text) = node.text_content() {
            let text_len = text.encode_utf16().count();
            return Ok((node, text_len));
        }
    }

    Err("no text node found in container".into())
}

// CursorRect is imported from weaver_editor_core.

/// Get screen coordinates for a character offset in the editor.
///
/// Returns the bounding rect of a zero-width range at the given offset.
#[cfg(all(target_family = "wasm", target_os = "unknown"))]
pub fn get_cursor_rect(
    char_offset: usize,
    offset_map: &[OffsetMapping],
    editor_id: &str,
) -> Option<CursorRect> {
    if offset_map.is_empty() {
        return None;
    }

    // Find mapping for this position
    let (mapping, char_offset) = match find_mapping_for_char(offset_map, char_offset) {
        Some((m, _)) => (m, char_offset),
        None => return None,
    };

    let window = web_sys::window()?;
    let document = window.document()?;

    // Get container element
    let container = document.get_element_by_id(&mapping.node_id).or_else(|| {
        let selector = format!("[data-node-id='{}']", mapping.node_id);
        document.query_selector(&selector).ok().flatten()
    })?;

    let range = document.create_range().ok()?;

    // Position the range at the character offset
    if let Some(child_index) = mapping.child_index {
        range.set_start(&container, child_index as u32).ok()?;
    } else {
        let container_element = container.dyn_into::<web_sys::HtmlElement>().ok()?;
        let offset_in_range = char_offset - mapping.char_range.start;
        let target_utf16_offset = mapping.char_offset_in_node + offset_in_range;

        if let Ok((text_node, node_offset)) =
            find_text_node_at_offset(&container_element, target_utf16_offset)
        {
            range.set_start(&text_node, node_offset as u32).ok()?;
        } else {
            return None;
        }
    }

    range.collapse_with_to_start(true);

    // Get the bounding rect
    let rect = range.get_bounding_client_rect();
    Some(CursorRect {
        x: rect.x(),
        y: rect.y(),
        height: rect.height().max(16.0), // Minimum height for empty lines
    })
}

/// Get screen coordinates relative to the editor container.
#[cfg(all(target_family = "wasm", target_os = "unknown"))]
pub fn get_cursor_rect_relative(
    char_offset: usize,
    offset_map: &[OffsetMapping],
    editor_id: &str,
) -> Option<CursorRect> {
    let cursor_rect = get_cursor_rect(char_offset, offset_map, editor_id)?;

    let window = web_sys::window()?;
    let document = window.document()?;
    let editor = document.get_element_by_id(editor_id)?;
    let editor_rect = editor.get_bounding_client_rect();

    Some(CursorRect {
        x: cursor_rect.x - editor_rect.x(),
        y: cursor_rect.y - editor_rect.y(),
        height: cursor_rect.height,
    })
}

#[cfg(not(all(target_family = "wasm", target_os = "unknown")))]
pub fn get_cursor_rect_relative(
    _char_offset: usize,
    _offset_map: &[OffsetMapping],
    _editor_id: &str,
) -> Option<CursorRect> {
    None
}

// SelectionRect is imported from weaver_editor_core.

/// Get screen rectangles for a selection range, relative to editor.
///
/// Returns multiple rects if selection spans multiple lines.
#[cfg(all(target_family = "wasm", target_os = "unknown"))]
pub fn get_selection_rects_relative(
    start: usize,
    end: usize,
    offset_map: &[OffsetMapping],
    editor_id: &str,
) -> Vec<SelectionRect> {
    use wasm_bindgen::JsCast;

    if offset_map.is_empty() || start >= end {
        return vec![];
    }

    let Some(window) = web_sys::window() else {
        return vec![];
    };
    let Some(document) = window.document() else {
        return vec![];
    };
    let Some(editor) = document.get_element_by_id(editor_id) else {
        return vec![];
    };
    let editor_rect = editor.get_bounding_client_rect();

    // Find mappings for start and end
    let Some((start_mapping, _)) = find_mapping_for_char(offset_map, start) else {
        return vec![];
    };
    let Some((end_mapping, _)) = find_mapping_for_char(offset_map, end) else {
        return vec![];
    };

    // Get containers
    let start_container = document
        .get_element_by_id(&start_mapping.node_id)
        .or_else(|| {
            let selector = format!("[data-node-id='{}']", start_mapping.node_id);
            document.query_selector(&selector).ok().flatten()
        });
    let end_container = document
        .get_element_by_id(&end_mapping.node_id)
        .or_else(|| {
            let selector = format!("[data-node-id='{}']", end_mapping.node_id);
            document.query_selector(&selector).ok().flatten()
        });

    let (Some(start_container), Some(end_container)) = (start_container, end_container) else {
        return vec![];
    };

    // Create range
    let Ok(range) = document.create_range() else {
        return vec![];
    };

    // Set start
    if let Some(child_index) = start_mapping.child_index {
        let _ = range.set_start(&start_container, child_index as u32);
    } else if let Ok(container_element) = start_container.clone().dyn_into::<web_sys::HtmlElement>()
    {
        let offset_in_range = start - start_mapping.char_range.start;
        let target_utf16_offset = start_mapping.char_offset_in_node + offset_in_range;
        if let Ok((text_node, node_offset)) =
            find_text_node_at_offset(&container_element, target_utf16_offset)
        {
            let _ = range.set_start(&text_node, node_offset as u32);
        }
    }

    // Set end
    if let Some(child_index) = end_mapping.child_index {
        let _ = range.set_end(&end_container, child_index as u32);
    } else if let Ok(container_element) = end_container.dyn_into::<web_sys::HtmlElement>() {
        let offset_in_range = end - end_mapping.char_range.start;
        let target_utf16_offset = end_mapping.char_offset_in_node + offset_in_range;
        if let Ok((text_node, node_offset)) =
            find_text_node_at_offset(&container_element, target_utf16_offset)
        {
            let _ = range.set_end(&text_node, node_offset as u32);
        }
    }

    // Get all rects (one per line)
    let Some(rects) = range.get_client_rects() else {
        return vec![];
    };
    let mut result = Vec::new();

    for i in 0..rects.length() {
        if let Some(rect) = rects.get(i) {
            let rect: web_sys::DomRect = rect;
            result.push(SelectionRect {
                x: rect.x() - editor_rect.x(),
                y: rect.y() - editor_rect.y(),
                width: rect.width(),
                height: rect.height().max(16.0),
            });
        }
    }

    result
}

#[cfg(not(all(target_family = "wasm", target_os = "unknown")))]
pub fn get_selection_rects_relative(
    _start: usize,
    _end: usize,
    _offset_map: &[OffsetMapping],
    _editor_id: &str,
) -> Vec<SelectionRect> {
    vec![]
}
