//! DOM synchronization for the markdown editor.
//!
//! Handles syncing cursor/selection state between the browser DOM and our
//! internal document model, and updating paragraph DOM elements.

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
use super::cursor::restore_cursor_position;
#[allow(unused_imports)]
use super::document::{EditorDocument, Selection};
#[allow(unused_imports)]
use super::offset_map::{SnapDirection, find_nearest_valid_position, is_valid_cursor_position};
use super::paragraph::ParagraphRender;
#[allow(unused_imports)]
use dioxus::prelude::*;

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
    let mut walked_from: Option<web_sys::Node> = None; // Track the child we walked up from
    let node_id = loop {
        let node_name = current_node.node_name();
        let node_id_attr = current_node
            .dyn_ref::<web_sys::Element>()
            .and_then(|e| e.get_attribute("id"));
        tracing::trace!(
            node_name = %node_name,
            node_id_attr = ?node_id_attr,
            "dom_position_to_text_offset: walk-up iteration"
        );

        if let Some(element) = current_node.dyn_ref::<web_sys::Element>() {
            if element == editor_element {
                // Selection is on the editor container itself
                //
                // IMPORTANT: If we WALKED UP to the editor from a descendant,
                // offset_in_text_node is the offset within that descendant, NOT the
                // child index in the editor. We need to find which paragraph contains
                // the node we walked from.
                if let Some(ref walked_node) = walked_from {
                    // We walked up from a descendant - find which paragraph it belongs to
                    tracing::debug!(
                        walked_from_node_name = %walked_node.node_name(),
                        "dom_position_to_text_offset: walked up to editor from descendant"
                    );

                    // Find paragraph containing this node by checking paragraph wrapper divs
                    for (idx, para) in paragraphs.iter().enumerate() {
                        if let Some(para_elem) = dom_document.get_element_by_id(&para.id) {
                            let para_node: &web_sys::Node = para_elem.as_ref();
                            if para_node.contains(Some(walked_node)) {
                                // Found the paragraph - return its start
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
                    // Couldn't find containing paragraph, fall through
                    tracing::warn!(
                        "dom_position_to_text_offset: walked up to editor but couldn't find containing paragraph"
                    );
                    break None;
                }

                // Selection is directly on the editor container (e.g., Cmd+A select all)
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
                // Match both old-style "n0" and paragraph-prefixed "p-2-n0" node IDs
                let is_node_id = id.starts_with('n') || id.contains("-n");
                tracing::trace!(
                    id = %id,
                    is_node_id,
                    starts_with_n = id.starts_with('n'),
                    contains_dash_n = id.contains("-n"),
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

    let node_id = node_id?;

    let container = dom_document.get_element_by_id(&node_id).or_else(|| {
        let selector = format!("[data-node-id='{}']", node_id);
        dom_document.query_selector(&selector).ok().flatten()
    })?;

    // Calculate UTF-16 offset from start of container to the position
    // Skip text nodes inside contenteditable="false" elements (like embeds)
    let mut utf16_offset_in_container = 0;

    // Check if the node IS the container element itself (not a text node descendant)
    // In this case, offset_in_text_node is actually a child index, not a character offset
    let node_is_container = node
        .dyn_ref::<web_sys::Element>()
        .map(|e| e == &container)
        .unwrap_or(false);

    if node_is_container {
        // offset_in_text_node is a child index - count text content up to that child
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

        tracing::debug!(
            child_index,
            utf16_offset = utf16_offset_in_container,
            "dom_position_to_text_offset: node is container, using child index"
        );
    } else {
        // Normal case: node is a text node, walk to find it
        // Use SHOW_ALL (0xFFFFFFFF) to see element boundaries for tracking non-editable regions
        if let Ok(walker) =
            dom_document.create_tree_walker_with_what_to_show(&container, 0xFFFFFFFF)
        {
            // Track the non-editable element we're inside (if any)
            let mut skip_until_exit: Option<web_sys::Element> = None;

            while let Ok(Some(dom_node)) = walker.next_node() {
                // Check if we've exited the non-editable subtree
                if let Some(ref skip_elem) = skip_until_exit {
                    if !skip_elem.contains(Some(&dom_node)) {
                        skip_until_exit = None;
                    }
                }

                // Check if entering a non-editable element
                if skip_until_exit.is_none() {
                    if let Some(element) = dom_node.dyn_ref::<web_sys::Element>() {
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

    // Log what we're looking for
    tracing::trace!(
        node_id = %node_id,
        utf16_offset = utf16_offset_in_container,
        num_paragraphs = paragraphs.len(),
        "dom_position_to_text_offset: looking up mapping"
    );

    for para in paragraphs {
        for mapping in &para.offset_map {
            if mapping.node_id == node_id {
                let mapping_start = mapping.char_offset_in_node;
                let mapping_end = mapping.char_offset_in_node + mapping.utf16_len;

                tracing::trace!(
                    mapping_node_id = %mapping.node_id,
                    mapping_start,
                    mapping_end,
                    char_range_start = mapping.char_range.start,
                    char_range_end = mapping.char_range.end,
                    "dom_position_to_text_offset: found matching node_id"
                );

                if utf16_offset_in_container >= mapping_start
                    && utf16_offset_in_container <= mapping_end
                {
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

/// Update paragraph DOM elements incrementally using pool-based surgical diffing.
///
/// Uses stable content-based paragraph IDs for efficient DOM reconciliation:
/// - Unchanged paragraphs (same ID + hash) are not touched
/// - Changed paragraphs (same ID, different hash) get innerHTML updated
/// - New paragraphs get created and inserted at correct position
/// - Removed paragraphs get deleted
///
/// Returns true if the paragraph containing the cursor was updated.
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub fn update_paragraph_dom(
    editor_id: &str,
    old_paragraphs: &[ParagraphRender],
    new_paragraphs: &[ParagraphRender],
    cursor_offset: usize,
    force: bool,
) -> bool {
    use std::collections::HashMap;
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

    let mut cursor_para_updated = false;

    // Build lookup for old paragraphs by ID (for syntax span comparison)
    let old_para_map: HashMap<&str, &ParagraphRender> =
        old_paragraphs.iter().map(|p| (p.id.as_str(), p)).collect();

    // Build pool of existing DOM elements by ID
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

    // Track position for insertBefore - starts at first element child
    // (use first_element_child to skip any stray text nodes)
    let mut cursor_node: Option<web_sys::Node> = editor.first_element_child().map(|e| e.into());

    // Single pass through new paragraphs
    for new_para in new_paragraphs.iter() {
        let para_id = &new_para.id;
        let new_hash = format!("{:x}", new_para.source_hash);
        let is_cursor_para =
            new_para.char_range.start <= cursor_offset && cursor_offset <= new_para.char_range.end;

        if let Some(existing_elem) = old_elements.remove(para_id) {
            // Element exists - check if it needs updating
            let old_hash = existing_elem.get_attribute("data-hash").unwrap_or_default();
            let needs_update = force || old_hash != new_hash;

            // Check if element is at correct position (compare as nodes)
            let existing_as_node: &web_sys::Node = existing_elem.as_ref();
            let at_correct_position = cursor_node
                .as_ref()
                .map(|c| c == existing_as_node)
                .unwrap_or(false);

            if !at_correct_position {
                tracing::warn!(
                    para_id,
                    is_cursor_para,
                    "update_paragraph_dom: element not at correct position, moving"
                );
                let _ = editor.insert_before(existing_as_node, cursor_node.as_ref());
                if is_cursor_para {
                    cursor_para_updated = true;
                }
            } else {
                // Use next_element_sibling to skip any stray text nodes
                cursor_node = existing_elem.next_element_sibling().map(|e| e.into());
            }

            if needs_update {
                // TESTING: Force innerHTML update to measure timing cost
                // TODO: Remove this flag after benchmarking
                const FORCE_INNERHTML_UPDATE: bool = true;

                // For cursor paragraph: only update if syntax/formatting changed
                // This prevents destroying browser selection during fast typing
                //
                // HOWEVER: we must verify browser actually updated the DOM.
                // PassThrough assumes browser handles edit, but sometimes it doesn't.
                let should_skip_cursor_update =
                    !FORCE_INNERHTML_UPDATE && is_cursor_para && !force && {
                        let old_para = old_para_map.get(para_id.as_str());
                        let syntax_unchanged = old_para
                            .map(|old| old.syntax_spans == new_para.syntax_spans)
                            .unwrap_or(false);

                        // Verify DOM content length matches expected - if not, browser didn't handle it
                        // NOTE: Get inner element (the <p>) not outer div, to avoid counting
                        // the newline from </p>\n in the HTML
                        let dom_matches_expected = if syntax_unchanged {
                            let inner_elem = existing_elem.first_element_child();
                            let dom_text = inner_elem
                                .as_ref()
                                .and_then(|e| e.text_content())
                                .unwrap_or_default();
                            let expected_len = new_para.byte_range.end - new_para.byte_range.start;
                            let dom_len = dom_text.len();
                            let matches = dom_len == expected_len;
                            // Always log for debugging
                            tracing::debug!(
                                para_id = %para_id,
                                dom_len,
                                expected_len,
                                matches,
                                dom_text = %dom_text,
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
                        para_id,
                        "update_paragraph_dom: skipping cursor para innerHTML (syntax unchanged, DOM verified)"
                    );
                    // Update hash - browser native editing has the correct content
                    let _ = existing_elem.set_attribute("data-hash", &new_hash);
                } else {
                    // Timing instrumentation for innerHTML update cost
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
                            tracing::debug!(
                                para_id,
                                is_cursor_para,
                                elapsed_ms,
                                html_len = new_para.html.len(),
                                old_hash = %old_hash,
                                new_hash = %new_hash,
                                "update_paragraph_dom: innerHTML update timing"
                            );
                        }
                    }

                    if is_cursor_para {
                        // Restore cursor synchronously - don't wait for rAF
                        // This prevents race conditions with fast typing
                        if let Err(e) = restore_cursor_position(
                            cursor_offset,
                            &new_para.offset_map,
                            editor_id,
                            None,
                        ) {
                            tracing::warn!("Synchronous cursor restore failed: {:?}", e);
                        }
                        cursor_para_updated = true;
                    }
                }
            }
        } else {
            // New element - create and insert at current position
            if let Ok(div) = document.create_element("div") {
                div.set_id(para_id);
                div.set_inner_html(&new_para.html);
                let _ = div.set_attribute("data-hash", &new_hash);
                let div_node: &web_sys::Node = div.as_ref();
                let _ = editor.insert_before(div_node, cursor_node.as_ref());
            }

            if is_cursor_para {
                cursor_para_updated = true;
            }
        }
    }

    // Remove stale elements (still in pool = not in new paragraphs)
    for (_, elem) in old_elements {
        let _ = elem.remove();
        cursor_para_updated = true; // Structure changed, cursor may need restoration
    }

    cursor_para_updated
}
