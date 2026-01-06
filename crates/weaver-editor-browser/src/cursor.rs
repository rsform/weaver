//! Browser implementation of cursor platform operations.
//!
//! Uses the DOM Selection API to position cursors and retrieve screen coordinates.

use wasm_bindgen::JsCast;
use weaver_editor_core::{
    CursorPlatform, CursorRect, OffsetMapping, ParagraphRender, PlatformError, SelectionRect,
    SnapDirection, find_mapping_for_char, find_nearest_valid_position,
};

/// Browser-based cursor platform implementation.
///
/// Holds a reference to the editor element ID for DOM lookups.
pub struct BrowserCursor {
    editor_id: String,
}

impl BrowserCursor {
    /// Create a new browser cursor handler for the given editor element.
    pub fn new(editor_id: impl Into<String>) -> Self {
        Self {
            editor_id: editor_id.into(),
        }
    }

    /// Get the editor element ID.
    pub fn editor_id(&self) -> &str {
        &self.editor_id
    }
}

impl CursorPlatform for BrowserCursor {
    fn restore_cursor(
        &self,
        char_offset: usize,
        paragraphs: &[ParagraphRender],
        snap_direction: Option<SnapDirection>,
    ) -> Result<(), PlatformError> {
        // Find the paragraph containing this offset and use its offset map.
        let offset_map = find_offset_map_for_char(paragraphs, char_offset);
        restore_cursor_position(char_offset, offset_map, snap_direction)
    }

    fn get_cursor_rect(
        &self,
        char_offset: usize,
        paragraphs: &[ParagraphRender],
    ) -> Option<CursorRect> {
        let offset_map = find_offset_map_for_char(paragraphs, char_offset);
        get_cursor_rect_impl(char_offset, offset_map)
    }

    fn get_cursor_rect_relative(
        &self,
        char_offset: usize,
        paragraphs: &[ParagraphRender],
    ) -> Option<CursorRect> {
        let cursor_rect = self.get_cursor_rect(char_offset, paragraphs)?;

        let window = web_sys::window()?;
        let document = window.document()?;
        let editor = document.get_element_by_id(&self.editor_id)?;
        let editor_rect = editor.get_bounding_client_rect();

        Some(CursorRect::new(
            cursor_rect.x - editor_rect.x(),
            cursor_rect.y - editor_rect.y(),
            cursor_rect.height,
        ))
    }

    fn get_selection_rects_relative(
        &self,
        start: usize,
        end: usize,
        paragraphs: &[ParagraphRender],
    ) -> Vec<SelectionRect> {
        // For selection, we need all offset maps since selection can span paragraphs.
        let all_maps: Vec<_> = paragraphs
            .iter()
            .flat_map(|p| p.offset_map.iter())
            .collect();
        let borrowed: Vec<_> = all_maps.iter().map(|m| (*m).clone()).collect();
        get_selection_rects_impl(start, end, &borrowed, &self.editor_id)
    }
}

/// Find the offset map for a character offset from paragraphs.
///
/// Returns the offset map of the paragraph containing the given offset,
/// or an empty slice if no paragraph contains it.
fn find_offset_map_for_char(
    paragraphs: &[ParagraphRender],
    char_offset: usize,
) -> &[OffsetMapping] {
    for para in paragraphs {
        if para.char_range.start <= char_offset && char_offset <= para.char_range.end {
            return &para.offset_map;
        }
    }
    // Fallback: if offset is past the end, use the last paragraph.
    paragraphs
        .last()
        .map(|p| p.offset_map.as_slice())
        .unwrap_or(&[])
}

/// Restore cursor position in the DOM after re-render.
pub fn restore_cursor_position(
    char_offset: usize,
    offset_map: &[OffsetMapping],
    snap_direction: Option<SnapDirection>,
) -> Result<(), PlatformError> {
    if offset_map.is_empty() {
        return Ok(());
    }

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
        return Ok(());
    }

    let (mapping, char_offset) = match find_mapping_for_char(offset_map, char_offset) {
        Some((m, false)) => (m, char_offset),
        Some((m, true)) => {
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

    let window = web_sys::window().ok_or("no window")?;
    let document = window.document().ok_or("no document")?;

    let container = document
        .get_element_by_id(&mapping.node_id)
        .or_else(|| {
            let selector = format!("[data-node-id='{}']", mapping.node_id);
            document.query_selector(&selector).ok().flatten()
        })
        .ok_or_else(|| format!("element not found: {}", mapping.node_id))?;

    let selection = window
        .get_selection()
        .map_err(|e| format!("get_selection failed: {:?}", e))?
        .ok_or("no selection object")?;
    let range = document
        .create_range()
        .map_err(|e| format!("create_range failed: {:?}", e))?;

    if let Some(child_index) = mapping.child_index {
        range
            .set_start(&container, child_index as u32)
            .map_err(|e| format!("set_start failed: {:?}", e))?;
    } else {
        let container_element = container
            .dyn_into::<web_sys::HtmlElement>()
            .map_err(|_| "container is not HtmlElement")?;
        let offset_in_range = char_offset - mapping.char_range.start;
        let target_utf16_offset = mapping.char_offset_in_node + offset_in_range;
        let (text_node, node_offset) =
            find_text_node_at_offset(&container_element, target_utf16_offset)?;
        range
            .set_start(&text_node, node_offset as u32)
            .map_err(|e| format!("set_start failed: {:?}", e))?;
    }

    range.collapse_with_to_start(true);

    selection
        .remove_all_ranges()
        .map_err(|e| format!("remove_all_ranges failed: {:?}", e))?;
    selection
        .add_range(&range)
        .map_err(|e| format!("add_range failed: {:?}", e))?;

    Ok(())
}

/// Find text node at given UTF-16 offset within element.
pub fn find_text_node_at_offset(
    container: &web_sys::HtmlElement,
    target_utf16_offset: usize,
) -> Result<(web_sys::Node, usize), PlatformError> {
    let document = web_sys::window()
        .ok_or("no window")?
        .document()
        .ok_or("no document")?;

    let walker = document
        .create_tree_walker_with_what_to_show(container, 0xFFFFFFFF)
        .map_err(|e| format!("create_tree_walker failed: {:?}", e))?;

    let mut accumulated_utf16 = 0;
    let mut last_node: Option<web_sys::Node> = None;
    let mut skip_until_exit: Option<web_sys::Element> = None;

    while let Ok(Some(node)) = walker.next_node() {
        if let Some(ref skip_elem) = skip_until_exit {
            if !skip_elem.contains(Some(&node)) {
                skip_until_exit = None;
            }
        }

        if skip_until_exit.is_none() {
            if let Some(element) = node.dyn_ref::<web_sys::Element>() {
                if element.get_attribute("contenteditable").as_deref() == Some("false") {
                    skip_until_exit = Some(element.clone());
                    continue;
                }
            }
        }

        if skip_until_exit.is_some() {
            continue;
        }

        if node.node_type() != web_sys::Node::TEXT_NODE {
            continue;
        }

        last_node = Some(node.clone());

        if let Some(text) = node.text_content() {
            let text_len = text.encode_utf16().count();

            if accumulated_utf16 + text_len >= target_utf16_offset {
                let offset_in_node = target_utf16_offset - accumulated_utf16;
                return Ok((node, offset_in_node));
            }

            accumulated_utf16 += text_len;
        }
    }

    if let Some(node) = last_node {
        if let Some(text) = node.text_content() {
            let text_len = text.encode_utf16().count();
            return Ok((node, text_len));
        }
    }

    Err("no text node found in container".into())
}

/// Get screen coordinates for a cursor position (internal impl).
fn get_cursor_rect_impl(char_offset: usize, offset_map: &[OffsetMapping]) -> Option<CursorRect> {
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
            find_text_node_at_offset(&container_element, target_utf16_offset)
        {
            range.set_start(&text_node, node_offset as u32).ok()?;
        } else {
            return None;
        }
    }

    range.collapse_with_to_start(true);

    let rect = range.get_bounding_client_rect();
    Some(CursorRect::new(rect.x(), rect.y(), rect.height().max(16.0)))
}

/// Get selection rectangles relative to editor (internal impl).
fn get_selection_rects_impl(
    start: usize,
    end: usize,
    offset_map: &[OffsetMapping],
    editor_id: &str,
) -> Vec<SelectionRect> {
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

    let Some(rects) = range.get_client_rects() else {
        return vec![];
    };
    let mut result = Vec::new();

    for i in 0..rects.length() {
        if let Some(rect) = rects.get(i) {
            let rect: web_sys::DomRect = rect;
            result.push(SelectionRect::new(
                rect.x() - editor_rect.x(),
                rect.y() - editor_rect.y(),
                rect.width(),
                rect.height().max(16.0),
            ));
        }
    }

    result
}
