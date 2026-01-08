//! DOM synchronization for the markdown editor.
//!
//! Handles syncing cursor/selection state between the browser DOM and the
//! editor document model, and updating paragraph DOM elements.

use wasm_bindgen::JsCast;
use weaver_editor_core::{
    CursorSync, OffsetMapping, ParagraphRender, SnapDirection, find_nearest_valid_position,
    is_valid_cursor_position,
};

use weaver_editor_core::{EditorDocument, Selection, SyntaxSpanInfo};

use crate::cursor::restore_cursor_position;
use crate::update_syntax_visibility;

/// Result of syncing cursor from DOM.
#[derive(Debug, Clone)]
pub enum CursorSyncResult {
    /// Cursor is collapsed at this offset.
    Cursor(usize),
    /// Selection from anchor to head.
    Selection { anchor: usize, head: usize },
    /// Could not determine cursor position.
    None,
}

/// Browser-based cursor sync implementation.
///
/// Holds reference to editor element ID and provides methods to sync
/// cursor state from DOM back to the editor model.
pub struct BrowserCursorSync {
    editor_id: String,
}

impl BrowserCursorSync {
    /// Create a new browser cursor sync for the given editor element.
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

impl CursorSync for BrowserCursorSync {
    fn sync_cursor_from_platform<F, G>(
        &self,
        paragraphs: &[ParagraphRender],
        direction_hint: Option<SnapDirection>,
        on_cursor: F,
        on_selection: G,
    ) where
        F: FnOnce(usize),
        G: FnOnce(usize, usize),
    {
        if let Some(result) = sync_cursor_from_dom_impl(&self.editor_id, paragraphs, direction_hint)
        {
            match result {
                CursorSyncResult::Cursor(offset) => on_cursor(offset),
                CursorSyncResult::Selection { anchor, head } => {
                    if anchor == head {
                        on_cursor(anchor);
                    } else {
                        on_selection(anchor, head);
                    }
                }
                CursorSyncResult::None => {}
            }
        }
    }
}

/// Sync cursor state from DOM selection, returning the result.
///
/// This is the core implementation that reads the browser's selection state
/// and converts it to character offsets using paragraph offset maps.
pub fn sync_cursor_from_dom_impl(
    editor_id: &str,
    paragraphs: &[ParagraphRender],
    direction_hint: Option<SnapDirection>,
) -> Option<CursorSyncResult> {
    if paragraphs.is_empty() {
        return Some(CursorSyncResult::None);
    }

    let window = web_sys::window()?;
    let dom_document = window.document()?;
    let editor_element = dom_document.get_element_by_id(editor_id)?;

    let selection = window.get_selection().ok()??;

    let anchor_node = selection.anchor_node()?;
    let focus_node = selection.focus_node()?;
    let anchor_offset = selection.anchor_offset() as usize;
    let focus_offset = selection.focus_offset() as usize;

    tracing::trace!(
        anchor_node_name = %anchor_node.node_name(),
        anchor_offset,
        focus_node_name = %focus_node.node_name(),
        focus_offset,
        "sync_cursor_from_dom_impl: browser selection state"
    );

    let anchor_char = dom_position_to_text_offset(
        &dom_document,
        &editor_element,
        &anchor_node,
        anchor_offset,
        paragraphs,
        direction_hint,
    );
    let focus_char = dom_position_to_text_offset(
        &dom_document,
        &editor_element,
        &focus_node,
        focus_offset,
        paragraphs,
        direction_hint,
    );

    match (anchor_char, focus_char) {
        (Some(anchor), Some(head)) => {
            if anchor == head {
                Some(CursorSyncResult::Cursor(head))
            } else {
                Some(CursorSyncResult::Selection { anchor, head })
            }
        }
        _ => {
            tracing::warn!("Could not map DOM selection to text offsets");
            Some(CursorSyncResult::None)
        }
    }
}

/// Convert a DOM position (node + offset) to a text char offset.
///
/// Walks up from the node to find a container with a node ID, then uses
/// the paragraph offset maps to convert the UTF-16 offset to a character offset.
/// The `direction_hint` is used when snapping from invisible content to determine
/// which direction to prefer.
pub fn dom_position_to_text_offset(
    dom_document: &web_sys::Document,
    editor_element: &web_sys::Element,
    node: &web_sys::Node,
    offset_in_text_node: usize,
    paragraphs: &[ParagraphRender],
    direction_hint: Option<SnapDirection>,
) -> Option<usize> {
    // Find the containing element with a node ID (walk up from text node).
    let mut current_node = node.clone();
    let mut walked_from: Option<web_sys::Node> = None;

    let node_id = loop {
        let node_name = current_node.node_name();
        let node_id_attr = current_node
            .dyn_ref::<web_sys::Element>()
            .and_then(|e| e.get_attribute("id"));
        let text_content_preview = current_node
            .text_content()
            .map(|s| s.chars().take(20).collect::<String>())
            .unwrap_or_default();
        tracing::trace!(
            node_name = %node_name,
            node_id_attr = ?node_id_attr,
            text_preview = %text_content_preview.escape_debug(),
            "dom_position_to_text_offset: walk-up iteration"
        );

        if let Some(element) = current_node.dyn_ref::<web_sys::Element>() {
            if element == editor_element {
                // Selection is on the editor container itself.
                // IMPORTANT: If we WALKED UP to the editor from a descendant,
                // offset_in_text_node is the offset within that descendant, NOT the
                // child index in the editor.
                if let Some(ref walked_node) = walked_from {
                    tracing::trace!(
                        walked_from_node_name = %walked_node.node_name(),
                        "dom_position_to_text_offset: walked up to editor from descendant"
                    );

                    // Find paragraph containing this node by checking paragraph wrapper divs.
                    for (idx, para) in paragraphs.iter().enumerate() {
                        if let Some(para_elem) = dom_document.get_element_by_id(&para.id) {
                            let para_node: &web_sys::Node = para_elem.as_ref();
                            if para_node.contains(Some(walked_node)) {
                                tracing::trace!(
                                    para_id = %para.id,
                                    para_idx = idx,
                                    char_start = para.char_range.start,
                                    "dom_position_to_text_offset: found containing paragraph"
                                );
                                return Some(para.char_range.start);
                            }
                        }
                    }
                    tracing::warn!(
                        "dom_position_to_text_offset: walked up to editor but couldn't find containing paragraph"
                    );
                    break None;
                }

                // Selection is directly on the editor container (e.g., Cmd+A).
                let child_count = editor_element.child_element_count() as usize;
                tracing::trace!(
                    offset_in_text_node,
                    child_count,
                    "dom_position_to_text_offset: selection directly on editor container"
                );
                if offset_in_text_node == 0 {
                    tracing::trace!(
                        "dom_position_to_text_offset: returning 0 (editor container offset 0)"
                    );
                    return Some(0);
                } else if offset_in_text_node >= child_count {
                    let end = paragraphs.last().map(|p| p.char_range.end);
                    tracing::trace!(end = ?end, "dom_position_to_text_offset: returning end of last paragraph");
                    return end;
                }
                break None;
            }

            let id = element
                .get_attribute("id")
                .or_else(|| element.get_attribute("data-node-id"));

            if let Some(id) = id {
                // Match both old-style "n0" and paragraph-prefixed "p-2-n0" node IDs.
                let is_node_id = id.starts_with('n') || id.contains("-n");
                tracing::trace!(
                    id = %id,
                    is_node_id,
                    "dom_position_to_text_offset: checking ID pattern"
                );
                if is_node_id {
                    break Some(id);
                }
            }
        }

        walked_from = Some(current_node.clone());
        current_node = current_node.parent_node()?;
    };

    let node_id = match node_id {
        Some(id) => id,
        None => {
            tracing::trace!("dom_position_to_text_offset: no node_id found in walk-up");
            return None;
        }
    };

    tracing::trace!(node_id = %node_id, "dom_position_to_text_offset: found node_id");

    let container = dom_document.get_element_by_id(&node_id).or_else(|| {
        let selector = format!("[data-node-id='{}']", node_id);
        dom_document.query_selector(&selector).ok().flatten()
    })?;

    // Calculate UTF-16 offset from start of container to the position.
    let mut utf16_offset_in_container = 0;

    let node_is_container = node
        .dyn_ref::<web_sys::Element>()
        .map(|e| e == &container)
        .unwrap_or(false);

    if node_is_container {
        // offset_in_text_node is a child index - count text content up to that child.
        let child_index = offset_in_text_node;
        let children = container.child_nodes();
        let mut text_counted = 0usize;

        for i in 0..child_index.min(children.length() as usize) {
            if let Some(child) = children.get(i as u32) {
                if let Some(text) = child.text_content() {
                    text_counted += text.encode_utf16().count();
                }
            }
        }
        utf16_offset_in_container = text_counted;

        tracing::trace!(
            child_index,
            utf16_offset = utf16_offset_in_container,
            "dom_position_to_text_offset: node is container, using child index"
        );
    } else {
        // Normal case: node is a text node, walk to find it.
        if let Ok(walker) =
            dom_document.create_tree_walker_with_what_to_show(&container, 0xFFFFFFFF)
        {
            let mut skip_until_exit: Option<web_sys::Element> = None;

            while let Ok(Some(dom_node)) = walker.next_node() {
                if let Some(ref skip_elem) = skip_until_exit {
                    if !skip_elem.contains(Some(&dom_node)) {
                        skip_until_exit = None;
                    }
                }

                if skip_until_exit.is_none() {
                    if let Some(element) = dom_node.dyn_ref::<web_sys::Element>() {
                        if element.get_attribute("contenteditable").as_deref() == Some("false") {
                            skip_until_exit = Some(element.clone());
                            continue;
                        }
                    }
                }

                if skip_until_exit.is_some() {
                    continue;
                }

                if dom_node.node_type() == web_sys::Node::TEXT_NODE {
                    if &dom_node == node {
                        utf16_offset_in_container += offset_in_text_node;
                        break;
                    }

                    if let Some(text) = dom_node.text_content() {
                        utf16_offset_in_container += text.encode_utf16().count();
                    }
                }
            }
        }
    }

    // Log what we're looking for.
    tracing::trace!(
        node_id = %node_id,
        utf16_offset = utf16_offset_in_container,
        num_paragraphs = paragraphs.len(),
        "dom_position_to_text_offset: looking up mapping"
    );

    // Look up the offset in paragraph offset maps.
    // Track the best match for the node_id in case offset is past the end.
    let mut best_match_for_node: Option<(usize, &OffsetMapping)> = None;

    for para in paragraphs {
        for mapping in &para.offset_map {
            if mapping.node_id == node_id {
                let mapping_start = mapping.char_offset_in_node;
                let mapping_end = mapping.char_offset_in_node + mapping.utf16_len;

                tracing::trace!(
                    mapping_node_id = %mapping.node_id,
                    mapping_start,
                    mapping_end,
                    utf16_offset = utf16_offset_in_container,
                    char_range_start = mapping.char_range.start,
                    char_range_end = mapping.char_range.end,
                    "dom_position_to_text_offset: found matching node_id"
                );

                // Track the mapping with the highest end position for this node.
                if best_match_for_node.is_none() || mapping_end > best_match_for_node.unwrap().0 {
                    best_match_for_node = Some((mapping_end, mapping));
                }

                let in_range = utf16_offset_in_container >= mapping_start
                    && utf16_offset_in_container <= mapping_end;

                if in_range {
                    let offset_in_mapping = utf16_offset_in_container - mapping_start;
                    let char_offset = mapping.char_range.start + offset_in_mapping;

                    tracing::trace!(
                        node_id = %node_id,
                        utf16_offset = utf16_offset_in_container,
                        mapping_start,
                        mapping_end,
                        offset_in_mapping,
                        char_range_start = mapping.char_range.start,
                        char_offset,
                        "dom_position_to_text_offset: MATCHED mapping"
                    );

                    // Check if position is valid (not on invisible content).
                    if is_valid_cursor_position(&para.offset_map, char_offset) {
                        tracing::trace!(
                            char_offset,
                            "dom_position_to_text_offset: returning valid position from mapping"
                        );
                        return Some(char_offset);
                    }

                    // Position is on invisible content, snap to nearest valid.
                    if let Some(snapped) =
                        find_nearest_valid_position(&para.offset_map, char_offset, direction_hint)
                    {
                        tracing::trace!(
                            original = char_offset,
                            snapped = snapped.char_offset(),
                            "dom_position_to_text_offset: snapped from invisible to valid"
                        );
                        return Some(snapped.char_offset());
                    }

                    // Fallback to original if no snap target.
                    tracing::trace!(
                        char_offset,
                        "dom_position_to_text_offset: returning original (no snap target)"
                    );
                    return Some(char_offset);
                }
            }
        }
    }

    // If we found the node_id but offset was past the end, snap to the last tracked position.
    if let Some((max_end, mapping)) = best_match_for_node {
        if utf16_offset_in_container > max_end {
            // Cursor is past the end of tracked content - snap to end of last mapping.
            let char_offset = mapping.char_range.end;
            tracing::trace!(
                node_id = %node_id,
                utf16_offset = utf16_offset_in_container,
                max_tracked_end = max_end,
                snapped_to = char_offset,
                "dom_position_to_text_offset: offset past tracked content, snapping to end"
            );
            return Some(char_offset);
        }
    }

    // No mapping found - try to find a valid position in the paragraph matching the node_id.
    // Extract paragraph index from node_id format "p-{idx}-n{node}" to avoid jumping to wrong paragraph.
    let para_idx_from_node = node_id
        .strip_prefix("p-")
        .and_then(|rest| rest.split('-').next())
        .and_then(|idx_str| idx_str.parse::<usize>().ok());

    tracing::trace!(
        node_id = %node_id,
        utf16_offset = utf16_offset_in_container,
        para_idx_from_node = ?para_idx_from_node,
        num_paragraphs = paragraphs.len(),
        "dom_position_to_text_offset: NO MAPPING FOUND - falling back"
    );

    // First try the paragraph that matches the node_id prefix.
    if let Some(idx) = para_idx_from_node {
        if let Some(para) = paragraphs.get(idx) {
            if let Some(snapped) =
                find_nearest_valid_position(&para.offset_map, para.char_range.start, direction_hint)
            {
                tracing::trace!(
                    para_id = %para.id,
                    snapped_offset = snapped.char_offset(),
                    "dom_position_to_text_offset: fallback to matching paragraph"
                );
                return Some(snapped.char_offset());
            }
        }
    }

    // Last resort: try any paragraph (starting from first).
    for para in paragraphs {
        if let Some(snapped) =
            find_nearest_valid_position(&para.offset_map, para.char_range.start, direction_hint)
        {
            tracing::trace!(
                para_id = %para.id,
                snapped_offset = snapped.char_offset(),
                "dom_position_to_text_offset: fallback to first available paragraph"
            );
            return Some(snapped.char_offset());
        }
    }

    None
}

/// Sync cursor state from DOM to an EditorDocument.
///
/// This is a generic version that works with any `EditorDocument` implementation.
/// It reads the browser's selection state and updates the document's cursor and selection.
pub fn sync_cursor_from_dom<D: EditorDocument>(
    doc: &mut D,
    editor_id: &str,
    paragraphs: &[ParagraphRender],
    direction_hint: Option<SnapDirection>,
) {
    if let Some(result) = sync_cursor_from_dom_impl(editor_id, paragraphs, direction_hint) {
        match result {
            CursorSyncResult::Cursor(offset) => {
                doc.set_cursor_offset(offset);
                doc.set_selection(None);
            }
            CursorSyncResult::Selection { anchor, head } => {
                doc.set_cursor_offset(head);
                if anchor != head {
                    doc.set_selection(Some(Selection { anchor, head }));
                } else {
                    doc.set_selection(None);
                }
            }
            CursorSyncResult::None => {}
        }
    }
}

/// Sync cursor from DOM and update syntax visibility in one call.
///
/// This is the common pattern used by most event handlers: sync the cursor
/// position from the browser's selection, then update which syntax elements
/// are visible based on the new cursor position.
///
/// Use this for: onclick, onselect, onselectstart, onselectionchange, onkeyup.
pub fn sync_cursor_and_visibility<D: EditorDocument>(
    doc: &mut D,
    editor_id: &str,
    paragraphs: &[ParagraphRender],
    syntax_spans: &[SyntaxSpanInfo],
    direction_hint: Option<SnapDirection>,
) {
    sync_cursor_from_dom(doc, editor_id, paragraphs, direction_hint);
    let cursor_offset = doc.cursor_offset();
    let selection = doc.selection();
    update_syntax_visibility(cursor_offset, selection.as_ref(), syntax_spans, paragraphs);
}

/// Update paragraph DOM elements incrementally.
///
/// Uses stable content-based paragraph IDs for efficient DOM reconciliation:
/// - Unchanged paragraphs (same ID + hash) are not touched
/// - Changed paragraphs (same ID, different hash) get innerHTML updated
/// - New paragraphs get created and inserted at correct position
/// - Removed paragraphs get deleted
///
/// When `FORCE_INNERHTML_UPDATE` is false, cursor paragraph innerHTML updates
/// are skipped if only text content changed (syntax spans unchanged) and the
/// DOM content length matches expected. This allows browser-native editing
/// to proceed without disrupting the selection.
///
/// Returns true if the paragraph containing the cursor was updated.
pub fn update_paragraph_dom(
    editor_id: &str,
    old_paragraphs: &[ParagraphRender],
    new_paragraphs: &[ParagraphRender],
    cursor_offset: usize,
    force: bool,
) -> bool {
    use crate::FORCE_INNERHTML_UPDATE;
    use std::collections::HashMap;

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

    let mut cursor_para_updated = false;

    // Build lookup for old paragraphs by ID (for syntax span comparison).
    let old_para_map: HashMap<&str, &ParagraphRender> =
        old_paragraphs.iter().map(|p| (p.id.as_str(), p)).collect();

    // Build pool of existing DOM elements by ID.
    let mut old_elements: HashMap<String, web_sys::Element> = HashMap::new();
    let mut child_opt = editor.first_element_child();
    while let Some(child) = child_opt {
        if let Some(id) = child.get_attribute("id") {
            let next = child.next_element_sibling();
            old_elements.insert(id, child);
            child_opt = next;
        } else {
            child_opt = child.next_element_sibling();
        }
    }

    let mut cursor_node: Option<web_sys::Node> = editor.first_element_child().map(|e| e.into());

    for new_para in new_paragraphs.iter() {
        let para_id = &new_para.id;
        let new_hash = format!("{:x}", new_para.source_hash);
        let is_cursor_para =
            new_para.char_range.start <= cursor_offset && cursor_offset <= new_para.char_range.end;

        if let Some(existing_elem) = old_elements.remove(para_id.as_str()) {
            let old_hash = existing_elem.get_attribute("data-hash").unwrap_or_default();
            let needs_update = force || old_hash != new_hash;

            let existing_as_node: &web_sys::Node = existing_elem.as_ref();
            let at_correct_position = cursor_node
                .as_ref()
                .map(|c| c == existing_as_node)
                .unwrap_or(false);

            if !at_correct_position {
                tracing::warn!(
                    para_id = %para_id,
                    is_cursor_para,
                    "update_paragraph_dom: element not at correct position, moving"
                );
                let _ = editor.insert_before(existing_as_node, cursor_node.as_ref());
                if is_cursor_para {
                    cursor_para_updated = true;
                }
            } else {
                cursor_node = existing_elem.next_element_sibling().map(|e| e.into());
            }

            if needs_update {
                // For cursor paragraph: only update if syntax/formatting changed.
                // This prevents destroying browser selection during fast typing.
                //
                // HOWEVER: we must verify browser actually updated the DOM.
                // PassThrough assumes browser handles edit, but sometimes it doesn't.
                let should_skip_cursor_update =
                    !FORCE_INNERHTML_UPDATE && is_cursor_para && !force && {
                        let old_para = old_para_map.get(para_id.as_str());
                        let syntax_unchanged = old_para
                            .map(|old| old.syntax_spans == new_para.syntax_spans)
                            .unwrap_or(false);

                        // Verify DOM content length matches expected.
                        let dom_matches_expected = if syntax_unchanged {
                            let inner_elem = existing_elem.first_element_child();
                            let dom_text = inner_elem
                                .as_ref()
                                .and_then(|e| e.text_content())
                                .unwrap_or_default();
                            let expected_len = new_para.byte_range.end - new_para.byte_range.start;
                            let dom_len = dom_text.len();
                            let matches = dom_len == expected_len;
                            tracing::trace!(
                                para_id = %para_id,
                                dom_len,
                                expected_len,
                                matches,
                                "DOM sync check"
                            );
                            matches
                        } else {
                            false
                        };

                        syntax_unchanged && dom_matches_expected
                    };

                if should_skip_cursor_update {
                    tracing::trace!(
                        para_id = %para_id,
                        "update_paragraph_dom: skipping cursor para innerHTML (syntax unchanged, DOM verified)"
                    );
                    let _ = existing_elem.set_attribute("data-hash", &new_hash);
                } else {
                    if tracing::enabled!(tracing::Level::TRACE) {
                        let old_inner = existing_elem.inner_html();
                        tracing::trace!(
                            para_id = %para_id,
                            old_inner = %old_inner.escape_debug(),
                            new_html = %new_para.html.escape_debug(),
                            "update_paragraph_dom: replacing innerHTML"
                        );
                    }

                    // Timing instrumentation.
                    let start = web_sys::window()
                        .and_then(|w| w.performance())
                        .map(|p| p.now());

                    existing_elem.set_inner_html(&new_para.html);
                    let _ = existing_elem.set_attribute("data-hash", &new_hash);

                    if let Some(start_time) = start {
                        if let Some(end_time) = web_sys::window()
                            .and_then(|w| w.performance())
                            .map(|p| p.now())
                        {
                            let elapsed_ms = end_time - start_time;
                            tracing::trace!(
                                para_id = %para_id,
                                is_cursor_para,
                                elapsed_ms,
                                html_len = new_para.html.len(),
                                "update_paragraph_dom: innerHTML update timing"
                            );
                        }
                    }

                    if is_cursor_para {
                        if let Err(e) =
                            restore_cursor_position(cursor_offset, &new_para.offset_map, None)
                        {
                            tracing::warn!("Synchronous cursor restore failed: {:?}", e);
                        }
                        cursor_para_updated = true;
                    }
                }
            }
        } else {
            // New element - create and insert.
            if let Ok(div) = document.create_element("div") {
                div.set_id(para_id);
                div.set_inner_html(&new_para.html);
                let _ = div.set_attribute("data-hash", &new_hash);
                let div_node: &web_sys::Node = div.as_ref();
                let _ = editor.insert_before(div_node, cursor_node.as_ref());

                if is_cursor_para {
                    if let Err(e) =
                        restore_cursor_position(cursor_offset, &new_para.offset_map, None)
                    {
                        tracing::warn!("Cursor restore for new paragraph failed: {:?}", e);
                    }
                    cursor_para_updated = true;
                }
            }
        }
    }

    // Remove stale elements.
    for (_, elem) in old_elements {
        let _ = elem.remove();
        cursor_para_updated = true;
    }

    cursor_para_updated
}
