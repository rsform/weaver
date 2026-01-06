//! DOM synchronization for the markdown editor.
//!
//! Handles syncing cursor/selection state between the browser DOM and our
//! internal document model, and updating paragraph DOM elements.
//!
//! The core DOM position conversion is provided by `weaver_editor_browser`.

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
use super::cursor::restore_cursor_position;
#[allow(unused_imports)]
use super::document::Selection;
#[allow(unused_imports)]
use super::document::SignalEditorDocument;
use super::paragraph::ParagraphRender;
#[allow(unused_imports)]
use dioxus::prelude::*;
#[allow(unused_imports)]
use weaver_editor_core::SnapDirection;

// Re-export the DOM position conversion from browser crate.
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub use weaver_editor_browser::dom_position_to_text_offset;

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

        if let Some(existing_elem) = old_elements.remove(para_id.as_str()) {
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
                    para_id = %para_id,
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
                use super::FORCE_INNERHTML_UPDATE;

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
                        para_id = %para_id,
                        "update_paragraph_dom: skipping cursor para innerHTML (syntax unchanged, DOM verified)"
                    );
                    // Update hash - browser native editing has the correct content
                    let _ = existing_elem.set_attribute("data-hash", &new_hash);
                } else {
                    // Log old innerHTML before replacement to see what browser did
                    if tracing::enabled!(tracing::Level::TRACE) {
                        let old_inner = existing_elem.inner_html();
                        tracing::trace!(
                            para_id = %para_id,
                            old_inner = %old_inner.escape_debug(),
                            new_html = %new_para.html.escape_debug(),
                            "update_paragraph_dom: replacing innerHTML"
                        );
                    }

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
                                para_id = %para_id,
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
