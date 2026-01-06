//! Cursor position operations.
//!
//! Re-exports from browser crate with app-specific adapters.

pub use weaver_editor_browser::restore_cursor_position;
pub use weaver_editor_core::{CursorRect, OffsetMapping, SelectionRect};

#[cfg(all(target_family = "wasm", target_os = "unknown"))]
use weaver_editor_core::{SnapDirection, find_mapping_for_char};

/// Get screen coordinates for a character offset in the editor.
///
/// Returns the bounding rect of a zero-width range at the given offset.
#[cfg(all(target_family = "wasm", target_os = "unknown"))]
pub fn get_cursor_rect(
    char_offset: usize,
    offset_map: &[OffsetMapping],
    _editor_id: &str,
) -> Option<CursorRect> {
    use wasm_bindgen::JsCast;

    if offset_map.is_empty() {
        return None;
    }

    let (mapping, char_offset) = match find_mapping_for_char(offset_map, char_offset) {
        Some((m, _)) => (m, char_offset),
        None => return None,
    };

    let window = web_sys::window()?;
    let document = window.document()?;

    let container = document.get_element_by_id(&mapping.node_id).or_else(|| {
        let selector = format!("[data-node-id='{}']", mapping.node_id);
        document.query_selector(&selector).ok().flatten()
    })?;

    let range = document.create_range().ok()?;

    if let Some(child_index) = mapping.child_index {
        range.set_start(&container, child_index as u32).ok()?;
    } else {
        let container_element = container.dyn_into::<web_sys::HtmlElement>().ok()?;
        let offset_in_range = char_offset - mapping.char_range.start;
        let target_utf16_offset = mapping.char_offset_in_node + offset_in_range;

        if let Ok((text_node, node_offset)) =
            weaver_editor_browser::find_text_node_at_offset(&container_element, target_utf16_offset)
        {
            range.set_start(&text_node, node_offset as u32).ok()?;
        } else {
            return None;
        }
    }

    range.collapse_with_to_start(true);

    let rect = range.get_bounding_client_rect();
    Some(CursorRect {
        x: rect.x(),
        y: rect.y(),
        height: rect.height().max(16.0),
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

    let Some((start_mapping, _)) = find_mapping_for_char(offset_map, start) else {
        return vec![];
    };
    let Some((end_mapping, _)) = find_mapping_for_char(offset_map, end) else {
        return vec![];
    };

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

    let Ok(range) = document.create_range() else {
        return vec![];
    };

    if let Some(child_index) = start_mapping.child_index {
        let _ = range.set_start(&start_container, child_index as u32);
    } else if let Ok(container_element) = start_container.clone().dyn_into::<web_sys::HtmlElement>()
    {
        let offset_in_range = start - start_mapping.char_range.start;
        let target_utf16_offset = start_mapping.char_offset_in_node + offset_in_range;
        if let Ok((text_node, node_offset)) =
            weaver_editor_browser::find_text_node_at_offset(&container_element, target_utf16_offset)
        {
            let _ = range.set_start(&text_node, node_offset as u32);
        }
    }

    if let Some(child_index) = end_mapping.child_index {
        let _ = range.set_end(&end_container, child_index as u32);
    } else if let Ok(container_element) = end_container.dyn_into::<web_sys::HtmlElement>() {
        let offset_in_range = end - end_mapping.char_range.start;
        let target_utf16_offset = end_mapping.char_offset_in_node + offset_in_range;
        if let Ok((text_node, node_offset)) =
            weaver_editor_browser::find_text_node_at_offset(&container_element, target_utf16_offset)
        {
            let _ = range.set_end(&text_node, node_offset as u32);
        }
    }

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
