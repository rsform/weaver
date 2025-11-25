//! Cursor position restoration in the DOM.
//!
//! After re-rendering HTML, we need to restore the cursor to its original
//! position in the source text. This involves:
//! 1. Finding the offset mapping for the cursor's char position
//! 2. Getting the DOM element by node ID
//! 3. Walking text nodes to find the UTF-16 offset within the element
//! 4. Setting cursor with web_sys Selection API

use super::offset_map::{find_mapping_for_char, OffsetMapping};

#[cfg(all(target_family = "wasm", target_os = "unknown"))]
use wasm_bindgen::JsCast;

/// Restore cursor position in the DOM after re-render.
///
/// # Arguments
/// - `char_offset`: Cursor position as char offset in document
/// - `offset_map`: Mappings from source to DOM positions
/// - `editor_id`: DOM ID of the contenteditable element
///
/// # Algorithm
/// 1. Find offset mapping containing char_offset
/// 2. Get DOM node by mapping.node_id
/// 3. Walk text nodes to find UTF-16 position
/// 4. Set cursor with Selection API
#[cfg(all(target_family = "wasm", target_os = "unknown"))]
pub fn restore_cursor_position(
    char_offset: usize,
    offset_map: &[OffsetMapping],
    editor_id: &str,
) -> Result<(), wasm_bindgen::JsValue> {
    // Empty document - no cursor to restore
    if offset_map.is_empty() {
        return Ok(());
    }

    // Bounds check using offset map
    let max_offset = offset_map.iter().map(|m| m.char_range.end).max().unwrap_or(0);
    if char_offset > max_offset {
        tracing::warn!("cursor offset {} > max mapping offset {}", char_offset, max_offset);
        // Don't error, just skip restoration - this can happen during edits
        return Ok(());
    }

    // Find mapping for this cursor position
    let (mapping, _should_snap) = find_mapping_for_char(offset_map, char_offset)
        .ok_or("no mapping found for cursor offset")?;

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
    let selection = window
        .get_selection()?
        .ok_or("no selection object")?;
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
        let (text_node, node_offset) = find_text_node_at_offset(&container_element, target_utf16_offset)?;
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

    // Create tree walker to find text nodes
    // SHOW_TEXT = 4 (from DOM spec)
    let walker = document.create_tree_walker_with_what_to_show(
        container,
        4,
    )?;

    let mut accumulated_utf16 = 0;
    let mut last_node: Option<web_sys::Node> = None;

    while let Some(node) = walker.next_node()? {
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

/// Non-WASM stub for testing
#[cfg(not(all(target_family = "wasm", target_os = "unknown")))]
pub fn restore_cursor_position(
    _char_offset: usize,
    _offset_map: &[OffsetMapping],
    _editor_id: &str,
) -> Result<(), String> {
    Ok(())
}
