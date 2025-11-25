//! Markdown editor component with Obsidian-style formatting visibility.
//!
//! This module implements a WYSIWYG-like markdown editor where formatting
//! characters are hidden contextually based on cursor position, while still
//! editing plain markdown text under the hood.

mod cursor;
mod document;
mod formatting;
mod offset_map;
mod paragraph;
mod render;
mod rope_writer;
mod storage;
mod toolbar;
mod writer;

#[cfg(test)]
mod tests;

pub use document::{Affinity, CompositionState, CursorState, EditorDocument, Selection};
pub use formatting::{FormatAction, apply_formatting, find_word_boundaries};
pub use offset_map::{OffsetMapping, RenderResult, find_mapping_for_byte};
pub use paragraph::ParagraphRender;
pub use render::{RenderCache, render_paragraphs, render_paragraphs_incremental};
pub use rope_writer::RopeWriter;
pub use storage::{EditorSnapshot, clear_storage, load_from_storage, save_to_storage};
pub use toolbar::EditorToolbar;
pub use writer::WriterResult;

use dioxus::prelude::*;

/// Main markdown editor component.
///
/// # Props
/// - `initial_content`: Optional initial markdown content
///
/// # Features
/// - JumpRope-based text storage for efficient editing
/// - Event interception for full control over editing operations
/// - Toolbar formatting buttons
/// - LocalStorage auto-save with debouncing
/// - Keyboard shortcuts (Ctrl+B for bold, Ctrl+I for italic)
///
/// # Phase 1 Limitations
/// - Cursor jumps to end after each keystroke (acceptable for MVP)
/// - All formatting characters visible (no hiding based on cursor position)
/// - No proper grapheme cluster handling
/// - No IME composition support
/// - No undo/redo
/// - No selection with Shift+Arrow
/// - No mouse selection
#[component]
pub fn MarkdownEditor(initial_content: Option<String>) -> Element {
    // Try to restore from localStorage
    let restored = use_memo(move || {
        storage::load_from_storage()
            .map(|s| s.content)
            .or_else(|| initial_content.clone())
            .unwrap_or_default()
    });

    let mut document = use_signal(|| EditorDocument::new(restored()));
    let editor_id = "markdown-editor";

    // Cache for incremental paragraph rendering
    let mut render_cache = use_signal(|| render::RenderCache::default());

    // Render paragraphs with incremental caching
    let paragraphs = use_memo(move || {
        let doc = document();
        let cache = render_cache.peek();
        let edit = doc.last_edit.as_ref();

        let (paras, new_cache) =
            render::render_paragraphs_incremental(&doc.rope, Some(&cache), edit);

        // Update cache for next render (write-only via spawn to avoid reactive loop)
        dioxus::prelude::spawn(async move {
            render_cache.set(new_cache);
        });

        paras
    });

    // Flatten offset maps from all paragraphs
    let offset_map = use_memo(move || {
        paragraphs()
            .iter()
            .flat_map(|p| p.offset_map.iter().cloned())
            .collect::<Vec<_>>()
    });

    // Cache paragraphs for change detection AND for event handlers to access
    let mut cached_paragraphs = use_signal(|| Vec::<ParagraphRender>::new());

    // Update DOM when paragraphs change (incremental rendering)
    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    use_effect(move || {
        let new_paras = paragraphs();
        let cursor_offset = document().cursor.offset;

        // Use peek() to avoid creating reactive dependency on cached_paragraphs
        let prev = cached_paragraphs.peek().clone();

        let cursor_para_updated = update_paragraph_dom(editor_id, &prev, &new_paras, cursor_offset);

        // Only restore cursor if we actually re-rendered the paragraph it's in
        if cursor_para_updated {
            use wasm_bindgen::JsCast;
            use wasm_bindgen::prelude::*;

            let rope = document().rope.clone();
            let map = offset_map();

            // Use requestAnimationFrame to wait for browser paint
            if let Some(window) = web_sys::window() {
                let closure = Closure::once(move || {
                    if let Err(e) =
                        cursor::restore_cursor_position(&rope, cursor_offset, &map, editor_id)
                    {
                        tracing::warn!("Cursor restoration failed: {:?}", e);
                    }
                });

                let _ = window.request_animation_frame(closure.as_ref().unchecked_ref());
                closure.forget();
            }
        }

        // Store for next comparison AND for event handlers (write-only, no reactive read)
        cached_paragraphs.set(new_paras);
    });

    // Auto-save with debounce
    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    use_effect(move || {
        let doc = document();

        // Save after 500ms of no typing
        let timer = gloo_timers::callback::Timeout::new(500, move || {
            let _ = storage::save_to_storage(&doc.to_string(), doc.cursor.offset);
        });
        timer.forget();
    });

    rsx! {
        Stylesheet { href: asset!("/assets/styling/editor.css") }
        div { class: "markdown-editor-container",
            div { class: "editor-content-wrapper",
                // Debug panel
                div { class: "editor-debug",
                    "Cursor: {document().cursor.offset}, "
                    "Chars: {document().len_chars()}"
                }
                div {
                    id: "{editor_id}",
                    class: "editor-content",
                    contenteditable: "true",
                    // DOM populated via web-sys in use_effect for incremental updates

                    onkeydown: move |evt| {
                        // Only prevent default for operations that modify content
                        // Let browser handle arrow keys, Home/End naturally
                        if should_intercept_key(&evt) {
                            evt.prevent_default();
                            handle_keydown(evt, &mut document);
                        }
                    },

                    onkeyup: move |evt| {
                        use dioxus::prelude::keyboard_types::Key;
                        // Only sync cursor from DOM after navigation keys
                        // Content-modifying keys update cursor directly in handle_keydown
                        let dominated = matches!(
                            evt.key(),
                            Key::ArrowLeft | Key::ArrowRight | Key::ArrowUp | Key::ArrowDown |
                            Key::Home | Key::End | Key::PageUp | Key::PageDown
                        );
                        if dominated {
                            let paras = cached_paragraphs();
                            sync_cursor_from_dom(&mut document, editor_id, &paras);
                        }
                    },

                    onclick: move |_evt| {
                        // After mouse click, sync cursor from DOM
                        let paras = cached_paragraphs();
                        sync_cursor_from_dom(&mut document, editor_id, &paras);
                    },

                    onpaste: move |evt| {
                        handle_paste(evt, &mut document);
                    },

                    oncut: move |evt| {
                        handle_cut(evt, &mut document);
                    },
                }


            }

            EditorToolbar {
                on_format: move |action| {
                    document.with_mut(|doc| {
                        formatting::apply_formatting(doc, action);
                    });
                }
            }
        }
    }
}

/// Check if we need to intercept this key event
/// Returns true for content-modifying operations, false for navigation
fn should_intercept_key(evt: &Event<KeyboardData>) -> bool {
    use dioxus::prelude::keyboard_types::Key;

    let key = evt.key();
    let mods = evt.modifiers();

    // Handle Ctrl/Cmd shortcuts
    if mods.ctrl() || mods.meta() {
        if let Key::Character(ch) = &key {
            // Intercept our formatting shortcuts (Ctrl+B, Ctrl+I)
            return matches!(ch.as_str(), "b" | "i");
        }
        // Let browser handle other Ctrl/Cmd shortcuts (paste, copy, cut, undo, etc.)
        return false;
    }

    // Intercept content modifications
    matches!(
        key,
        Key::Character(_) | Key::Backspace | Key::Delete | Key::Enter | Key::Tab
    )
}

/// Sync internal cursor and selection state from browser DOM selection
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
fn sync_cursor_from_dom(
    document: &mut Signal<EditorDocument>,
    editor_id: &str,
    paragraphs: &[ParagraphRender],
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

    // Convert both DOM positions to rope offsets using cached paragraphs
    let anchor_rope = dom_position_to_rope_offset(
        &dom_document,
        &editor_element,
        &anchor_node,
        anchor_offset,
        paragraphs,
    );
    let focus_rope = dom_position_to_rope_offset(
        &dom_document,
        &editor_element,
        &focus_node,
        focus_offset,
        paragraphs,
    );

    document.with_mut(|doc| {
        match (anchor_rope, focus_rope) {
            (Some(anchor), Some(focus)) => {
                doc.cursor.offset = focus;
                if anchor != focus {
                    // There's an actual selection
                    doc.selection = Some(Selection {
                        anchor,
                        head: focus,
                    });
                    tracing::debug!("[SYNC] Selection {}..{}", anchor, focus);
                } else {
                    // Collapsed selection (just cursor)
                    doc.selection = None;
                    tracing::debug!("[SYNC] Cursor at {}", focus);
                }
            }
            _ => {
                tracing::warn!("Could not map DOM selection to rope offsets");
            }
        }
    });
}

/// Convert a DOM position (node + offset) to a rope char offset using offset maps
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
fn dom_position_to_rope_offset(
    dom_document: &web_sys::Document,
    editor_element: &web_sys::Element,
    node: &web_sys::Node,
    offset_in_text_node: usize,
    paragraphs: &[ParagraphRender],
) -> Option<usize> {
    use wasm_bindgen::JsCast;

    // Find the containing element with a node ID (walk up from text node)
    let mut current_node = node.clone();
    let node_id = loop {
        if let Some(element) = current_node.dyn_ref::<web_sys::Element>() {
            if element == editor_element {
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

    // Get the container element
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

    // Look up in offset maps
    for para in paragraphs {
        for mapping in &para.offset_map {
            if mapping.node_id == node_id {
                let mapping_start = mapping.char_offset_in_node;
                let mapping_end = mapping.char_offset_in_node + mapping.utf16_len;

                if utf16_offset_in_container >= mapping_start
                    && utf16_offset_in_container <= mapping_end
                {
                    let offset_in_mapping = utf16_offset_in_container - mapping_start;
                    return Some(mapping.char_range.start + offset_in_mapping);
                }
            }
        }
    }

    None
}

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
fn sync_cursor_from_dom(
    _document: &mut Signal<EditorDocument>,
    _editor_id: &str,
    _paragraphs: &[ParagraphRender],
) {
    // No-op on non-wasm
}

/// Handle paste events and insert text at cursor
fn handle_paste(evt: Event<ClipboardData>, document: &mut Signal<EditorDocument>) {
    tracing::info!("[PASTE] handle_paste called");

    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    {
        use dioxus::web::WebEventExt;
        use wasm_bindgen::JsCast;

        let base_evt = evt.as_web_event();
        if let Some(clipboard_evt) = base_evt.dyn_ref::<web_sys::ClipboardEvent>() {
            if let Some(data_transfer) = clipboard_evt.clipboard_data() {
                if let Ok(text) = data_transfer.get_data("text/plain") {
                    tracing::info!("[PASTE] Got text: {} chars", text.len());
                    document.with_mut(|doc| {
                        // Delete selection if present
                        if let Some(sel) = doc.selection {
                            let (start, end) = (sel.anchor.min(sel.head), sel.anchor.max(sel.head));
                            doc.rope.remove(start..end);
                            doc.cursor.offset = start;
                            doc.selection = None;
                        }

                        // Insert pasted text
                        doc.rope.insert(doc.cursor.offset, &text);
                        doc.cursor.offset += text.chars().count();
                    });
                }
            }
        } else {
            tracing::warn!("[PASTE] Failed to cast to ClipboardEvent");
        }
    }
}

/// Handle cut events - browser copies selection, we delete it from rope
/// Selection is synced via onkeyup/onclick, so doc.selection should be current
fn handle_cut(_evt: Event<ClipboardData>, document: &mut Signal<EditorDocument>) {
    tracing::info!("[CUT] handle_cut called");

    document.with_mut(|doc| {
        if let Some(sel) = doc.selection {
            let (start, end) = (sel.anchor.min(sel.head), sel.anchor.max(sel.head));
            if start != end {
                tracing::info!("[CUT] Deleting selection {}..{}", start, end);
                doc.rope.remove(start..end);
                doc.cursor.offset = start;
                doc.selection = None;
            }
        }
    });
}

/// Handle keyboard events and update document state
fn handle_keydown(evt: Event<KeyboardData>, document: &mut Signal<EditorDocument>) {
    use dioxus::prelude::keyboard_types::Key;

    let key = evt.key();
    let mods = evt.modifiers();

    document.with_mut(|doc| {
        match key {
            Key::Character(ch) => {
                // Keyboard shortcuts first
                if mods.ctrl() {
                    match ch.as_str() {
                        "b" => {
                            formatting::apply_formatting(doc, FormatAction::Bold);
                            return;
                        }
                        "i" => {
                            formatting::apply_formatting(doc, FormatAction::Italic);
                            return;
                        }
                        _ => {}
                    }
                }

                // Insert character at cursor
                if doc.selection.is_some() {
                    // Delete selection first
                    let sel = doc.selection.unwrap();
                    let (start, end) = (sel.anchor.min(sel.head), sel.anchor.max(sel.head));
                    doc.rope.remove(start..end);
                    doc.cursor.offset = start;
                    doc.selection = None;
                }

                doc.rope.insert(doc.cursor.offset, &ch);
                doc.cursor.offset += ch.chars().count();
            }

            Key::Backspace => {
                if let Some(sel) = doc.selection {
                    // Delete selection
                    let (start, end) = (sel.anchor.min(sel.head), sel.anchor.max(sel.head));
                    doc.rope.remove(start..end);
                    doc.cursor.offset = start;
                    doc.selection = None;
                } else if doc.cursor.offset > 0 {
                    // Check if we're about to delete a newline
                    let prev_char = get_char_at(&doc.rope, doc.cursor.offset - 1);

                    if prev_char == Some('\n') {
                        let newline_pos = doc.cursor.offset - 1;
                        let mut delete_start = newline_pos;
                        let mut delete_end = doc.cursor.offset;

                        // Check if there's another newline before this one (empty paragraph)
                        // If so, delete both newlines to merge paragraphs
                        if newline_pos > 0 {
                            let prev_prev_char = get_char_at(&doc.rope, newline_pos - 1);
                            if prev_prev_char == Some('\n') {
                                // Empty paragraph case: delete both newlines
                                delete_start = newline_pos - 1;
                            }
                        }

                        // Also check if there's a zero-width char after cursor (inserted by Shift+Enter)
                        if let Some(ch) = get_char_at(&doc.rope, delete_end) {
                            if ch == '\u{200C}' || ch == '\u{200B}' {
                                delete_end += 1;
                            }
                        }

                        // Scan backwards through whitespace before the newline(s)
                        while delete_start > 0 {
                            let ch = get_char_at(&doc.rope, delete_start - 1);
                            match ch {
                                Some(' ') | Some('\t') | Some('\u{200C}') | Some('\u{200B}') => {
                                    delete_start -= 1;
                                }
                                Some('\n') => break, // stop at another newline
                                _ => break,          // stop at actual content
                            }
                        }

                        // Delete from where we stopped to end (including any trailing zero-width)
                        doc.rope.remove(delete_start..delete_end);
                        doc.cursor.offset = delete_start;
                    } else {
                        // Normal backspace - delete one char
                        let prev = doc.cursor.offset - 1;
                        doc.rope.remove(prev..doc.cursor.offset);
                        doc.cursor.offset = prev;
                    }
                }
            }

            Key::Delete => {
                if let Some(sel) = doc.selection {
                    // Delete selection
                    let (start, end) = (sel.anchor.min(sel.head), sel.anchor.max(sel.head));
                    doc.rope.remove(start..end);
                    doc.cursor.offset = start;
                    doc.selection = None;
                } else if doc.cursor.offset < doc.len_chars() {
                    // Delete next char
                    doc.rope.remove(doc.cursor.offset..doc.cursor.offset + 1);
                }
            }

            // Arrow keys handled by browser, synced in onkeyup
            Key::ArrowLeft | Key::ArrowRight | Key::ArrowUp | Key::ArrowDown => {
                // Browser handles these naturally
            }

            Key::Enter => {
                if doc.selection.is_some() {
                    let sel = doc.selection.unwrap();
                    let (start, end) = (sel.anchor.min(sel.head), sel.anchor.max(sel.head));
                    doc.rope.remove(start..end);
                    doc.cursor.offset = start;
                    doc.selection = None;
                }

                if mods.shift() {
                    // Shift+Enter: hard line break (soft break)
                    doc.rope.insert(doc.cursor.offset, "  \n\u{200C}");
                    doc.cursor.offset += 3;
                } else {
                    // Enter: paragraph break
                    doc.rope.insert(doc.cursor.offset, "\n\n");
                    doc.cursor.offset += 2;
                }
            }

            // Home/End handled by browser, synced in onkeyup
            Key::Home | Key::End => {
                // Browser handles these naturally
            }

            _ => {}
        }
    });
}

/// Get character at the given offset in the rope
fn get_char_at(rope: &jumprope::JumpRopeBuf, offset: usize) -> Option<char> {
    if offset >= rope.len_chars() {
        return None;
    }

    let rope = rope.borrow();
    let mut current = 0;
    for substr in rope.slice_substrings(offset..offset + 1) {
        for c in substr.chars() {
            if current == 0 {
                return Some(c);
            }
            current += 1;
        }
    }
    None
}

/// Find start of line containing offset
fn find_line_start(rope: &jumprope::JumpRopeBuf, offset: usize) -> usize {
    // Search backwards from cursor for newline
    let mut char_pos = 0;
    let mut last_newline_pos = None;

    let rope = rope.borrow();
    for substr in rope.slice_substrings(0..offset) {
        // TODO: make more efficient
        for c in substr.chars() {
            if c == '\n' {
                last_newline_pos = Some(char_pos);
            }
            char_pos += 1;
        }
    }

    last_newline_pos.map(|pos| pos + 1).unwrap_or(0)
}

/// Find end of line containing offset
fn find_line_end(rope: &jumprope::JumpRopeBuf, offset: usize) -> usize {
    // Search forwards from cursor for newline
    let mut char_pos = offset;

    let rope = rope.borrow();
    let byte_len = rope.len_bytes() - 1;
    for substr in rope.slice_substrings(offset..byte_len) {
        // TODO: make more efficient
        for c in substr.chars() {
            if c == '\n' {
                return char_pos;
            }
            char_pos += 1;
        }
    }

    rope.len_chars()
}

/// Update paragraph DOM elements incrementally.
///
/// Only modifies paragraphs that changed (by comparing source_hash).
/// Browser preserves cursor naturally in unchanged paragraphs.
///
/// Returns true if the paragraph containing the cursor was updated.
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
fn update_paragraph_dom(
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

    // Update or create paragraphs
    for (idx, new_para) in new_paragraphs.iter().enumerate() {
        let para_id = format!("para-{}", idx);

        if let Some(old_para) = old_paragraphs.get(idx) {
            // Paragraph exists - check if changed
            if new_para.source_hash != old_para.source_hash {
                // Changed - update innerHTML
                if let Some(elem) = document.get_element_by_id(&para_id) {
                    elem.set_inner_html(&new_para.html);
                }

                // Track if we updated the cursor's paragraph
                if Some(idx) == cursor_para_idx {
                    cursor_para_updated = true;
                }
            }
            // Unchanged - do nothing, browser preserves cursor
        } else {
            // New paragraph - create div
            if let Ok(div) = document.create_element("div") {
                div.set_id(&para_id);
                div.set_inner_html(&new_para.html);
                let _ = editor.append_child(&div);
            }

            // Track if we created the cursor's paragraph
            if Some(idx) == cursor_para_idx {
                cursor_para_updated = true;
            }
        }
    }

    // Remove extra paragraphs if document got shorter
    for idx in new_paragraphs.len()..old_paragraphs.len() {
        let para_id = format!("para-{}", idx);
        if let Some(elem) = document.get_element_by_id(&para_id) {
            let _ = elem.remove();
        }
    }

    cursor_para_updated
}

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
fn update_paragraph_dom(
    _editor_id: &str,
    _old_paragraphs: &[ParagraphRender],
    _new_paragraphs: &[ParagraphRender],
    _cursor_offset: usize,
) -> bool {
    false
}
