//! Input handling for the markdown editor.
//!
//! Keyboard events, clipboard operations, and text manipulation.

use dioxus::prelude::*;

use super::document::EditorDocument;
use super::formatting::{self, FormatAction};
use super::offset_map::SnapDirection;

/// Check if we need to intercept this key event.
/// Returns true for content-modifying operations, false for navigation.
pub fn should_intercept_key(evt: &Event<KeyboardData>) -> bool {
    use dioxus::prelude::keyboard_types::Key;

    let key = evt.key();
    let mods = evt.modifiers();

    // Handle Ctrl/Cmd shortcuts
    if mods.ctrl() || mods.meta() {
        if let Key::Character(ch) = &key {
            // Intercept our shortcuts: formatting (b/i), undo/redo (z/y), HTML export (e)
            match ch.as_str() {
                "b" | "i" | "z" | "y" => return true,
                "e" => return true, // Ctrl+E for HTML export/copy
                _ => {}
            }
        }
        // Intercept Cmd+Backspace (delete to start of line) and Cmd+Delete (delete to end)
        if matches!(key, Key::Backspace | Key::Delete) {
            return true;
        }
        // Let browser handle other Ctrl/Cmd shortcuts (paste, copy, cut, etc.)
        return false;
    }

    // Intercept content modifications
    matches!(
        key,
        Key::Character(_) | Key::Backspace | Key::Delete | Key::Enter | Key::Tab
    )
}

/// Handle keyboard events and update document state.
pub fn handle_keydown(evt: Event<KeyboardData>, doc: &mut EditorDocument) {
    use dioxus::prelude::keyboard_types::Key;

    let key = evt.key();
    let mods = evt.modifiers();

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
                    "z" => {
                        if mods.shift() {
                            // Ctrl+Shift+Z = redo
                            if let Ok(true) = doc.redo() {
                                let max = doc.len_chars();
                                doc.cursor.with_mut(|c| c.offset = c.offset.min(max));
                            }
                        } else {
                            // Ctrl+Z = undo
                            if let Ok(true) = doc.undo() {
                                let max = doc.len_chars();
                                doc.cursor.with_mut(|c| c.offset = c.offset.min(max));
                            }
                        }
                        doc.selection.set(None);
                        return;
                    }
                    "y" => {
                        // Ctrl+Y = redo (alternative)
                        if let Ok(true) = doc.redo() {
                            let max = doc.len_chars();
                            doc.cursor.with_mut(|c| c.offset = c.offset.min(max));
                        }
                        doc.selection.set(None);
                        return;
                    }
                    "e" => {
                        // Ctrl+E = copy as HTML (export)
                        if let Some(sel) = *doc.selection.read() {
                            let (start, end) = (sel.anchor.min(sel.head), sel.anchor.max(sel.head));
                            if start != end {
                                if let Some(markdown) = doc.slice(start, end) {
                                    let clean_md =
                                        markdown.replace('\u{200C}', "").replace('\u{200B}', "");
                                    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
                                    wasm_bindgen_futures::spawn_local(async move {
                                        if let Err(e) = copy_as_html(&clean_md).await {
                                            tracing::warn!("[COPY HTML] Failed: {:?}", e);
                                        }
                                    });
                                }
                            }
                        }
                        return;
                    }
                    _ => {}
                }
            }

            // Insert character at cursor (replacing selection if any)
            let sel = doc.selection.write().take();
            if let Some(sel) = sel {
                let (start, end) = (sel.anchor.min(sel.head), sel.anchor.max(sel.head));
                let _ = doc.replace_tracked(start, end.saturating_sub(start), &ch);
                doc.cursor.write().offset = start + ch.chars().count();
            } else {
                // Clean up any preceding zero-width chars (gap scaffolding)
                let cursor_offset = doc.cursor.read().offset;
                let mut delete_start = cursor_offset;
                while delete_start > 0 {
                    match get_char_at(doc.loro_text(), delete_start - 1) {
                        Some('\u{200C}') | Some('\u{200B}') => delete_start -= 1,
                        _ => break,
                    }
                }

                let zw_count = cursor_offset - delete_start;
                if zw_count > 0 {
                    // Splice: delete zero-width chars and insert new char in one op
                    let _ = doc.replace_tracked(delete_start, zw_count, &ch);
                    doc.cursor.write().offset = delete_start + ch.chars().count();
                } else if cursor_offset == doc.len_chars() {
                    // Fast path: append at end
                    let _ = doc.push_tracked(&ch);
                    doc.cursor.write().offset = cursor_offset + ch.chars().count();
                } else {
                    let _ = doc.insert_tracked(cursor_offset, &ch);
                    doc.cursor.write().offset = cursor_offset + ch.chars().count();
                }
            }
        }

        Key::Backspace => {
            // Snap backward after backspace (toward deleted content)
            doc.pending_snap.set(Some(SnapDirection::Backward));

            let sel = doc.selection.write().take();
            if let Some(sel) = sel {
                // Delete selection
                let (start, end) = (sel.anchor.min(sel.head), sel.anchor.max(sel.head));
                let _ = doc.remove_tracked(start, end.saturating_sub(start));
                doc.cursor.write().offset = start;
            } else if doc.cursor.read().offset > 0 {
                let cursor_offset = doc.cursor.read().offset;

                // Cmd+Backspace: delete to start of line
                if mods.meta() || mods.ctrl() {
                    let line_start = find_line_start(doc.loro_text(), cursor_offset);
                    if line_start < cursor_offset {
                        let _ = doc.remove_tracked(line_start, cursor_offset - line_start);
                        doc.cursor.write().offset = line_start;
                    }
                    return;
                }

                // Check if we're about to delete a newline
                let prev_char = get_char_at(doc.loro_text(), cursor_offset - 1);

                if prev_char == Some('\n') {
                    let newline_pos = cursor_offset - 1;
                    let mut delete_start = newline_pos;
                    let mut delete_end = cursor_offset;

                    // Check if there's another newline before this one (empty paragraph)
                    // If so, delete both newlines to merge paragraphs
                    if newline_pos > 0 {
                        let prev_prev_char = get_char_at(doc.loro_text(), newline_pos - 1);
                        if prev_prev_char == Some('\n') {
                            // Empty paragraph case: delete both newlines
                            delete_start = newline_pos - 1;
                        }
                    }

                    // Also check if there's a zero-width char after cursor (inserted by Shift+Enter)
                    if let Some(ch) = get_char_at(doc.loro_text(), delete_end) {
                        if ch == '\u{200C}' || ch == '\u{200B}' {
                            delete_end += 1;
                        }
                    }

                    // Scan backwards through whitespace before the newline(s)
                    while delete_start > 0 {
                        let ch = get_char_at(doc.loro_text(), delete_start - 1);
                        match ch {
                            Some('\u{200C}') | Some('\u{200B}') => {
                                delete_start -= 1;
                            }
                            Some('\n') => break, // stop at another newline
                            _ => break,          // stop at actual content
                        }
                    }

                    // Delete from where we stopped to end (including any trailing zero-width)
                    let _ =
                        doc.remove_tracked(delete_start, delete_end.saturating_sub(delete_start));
                    doc.cursor.write().offset = delete_start;
                } else {
                    // Normal backspace - delete one char
                    let prev = cursor_offset - 1;
                    let _ = doc.remove_tracked(prev, 1);
                    doc.cursor.write().offset = prev;
                }
            }
        }

        Key::Delete => {
            // Snap forward after delete (toward remaining content)
            doc.pending_snap.set(Some(SnapDirection::Forward));

            let sel = doc.selection.write().take();
            if let Some(sel) = sel {
                // Delete selection
                let (start, end) = (sel.anchor.min(sel.head), sel.anchor.max(sel.head));
                let _ = doc.remove_tracked(start, end.saturating_sub(start));
                doc.cursor.write().offset = start;
            } else {
                let cursor_offset = doc.cursor.read().offset;
                let doc_len = doc.len_chars();

                // Cmd+Delete: delete to end of line
                if mods.meta() || mods.ctrl() {
                    let line_end = find_line_end(doc.loro_text(), cursor_offset);
                    if cursor_offset < line_end {
                        let _ = doc.remove_tracked(cursor_offset, line_end - cursor_offset);
                    }
                    return;
                }

                if cursor_offset < doc_len {
                    // Delete next char
                    let _ = doc.remove_tracked(cursor_offset, 1);
                }
            }
        }

        // Arrow keys handled by browser, synced in onkeyup
        Key::ArrowLeft | Key::ArrowRight | Key::ArrowUp | Key::ArrowDown => {
            // Browser handles these naturally
        }

        Key::Enter => {
            // Snap forward after enter (into new paragraph/line)
            doc.pending_snap.set(Some(SnapDirection::Forward));

            let sel = doc.selection.write().take();
            if let Some(sel) = sel {
                let (start, end) = (sel.anchor.min(sel.head), sel.anchor.max(sel.head));
                let _ = doc.remove_tracked(start, end.saturating_sub(start));
                doc.cursor.write().offset = start;
            }

            let cursor_offset = doc.cursor.read().offset;
            if mods.shift() {
                // Shift+Enter: hard line break (soft break)
                let _ = doc.insert_tracked(cursor_offset, "  \n\u{200C}");
                doc.cursor.write().offset = cursor_offset + 3;
            } else if let Some(ctx) = detect_list_context(doc.loro_text(), cursor_offset) {
                // We're in a list item
                if is_list_item_empty(doc.loro_text(), cursor_offset, &ctx) {
                    // Empty item - exit list by removing marker and inserting paragraph break
                    let line_start = find_line_start(doc.loro_text(), cursor_offset);
                    let line_end = find_line_end(doc.loro_text(), cursor_offset);

                    // Delete the empty list item line INCLUDING its trailing newline
                    // line_end points to the newline, so +1 to include it
                    let delete_end = (line_end + 1).min(doc.len_chars());

                    // Use replace_tracked to atomically delete line and insert paragraph break
                    let _ = doc.replace_tracked(
                        line_start,
                        delete_end.saturating_sub(line_start),
                        "\n\n\u{200C}\n",
                    );
                    doc.cursor.write().offset = line_start + 2;
                } else {
                    // Non-empty item - continue list
                    let continuation = match ctx {
                        ListContext::Unordered { indent, marker } => {
                            format!("\n{}{} ", indent, marker)
                        }
                        ListContext::Ordered { indent, number } => {
                            format!("\n{}{}. ", indent, number + 1)
                        }
                    };
                    let len = continuation.chars().count();
                    let _ = doc.insert_tracked(cursor_offset, &continuation);
                    doc.cursor.write().offset = cursor_offset + len;
                }
            } else {
                // Not in a list - normal paragraph break
                let _ = doc.insert_tracked(cursor_offset, "\n\n");
                doc.cursor.write().offset = cursor_offset + 2;
            }
        }

        // Home/End handled by browser, synced in onkeyup
        Key::Home | Key::End => {
            // Browser handles these naturally
        }

        _ => {}
    }

    // Sync Loro cursor when edits affect paragraph boundaries
    // This ensures cursor position is tracked correctly through structural changes
    if doc
        .last_edit
        .read()
        .as_ref()
        .is_some_and(|e| e.contains_newline)
    {
        doc.sync_loro_cursor();
    }
}

/// Handle paste events and insert text at cursor.
pub fn handle_paste(evt: Event<ClipboardData>, doc: &mut EditorDocument) {
    evt.prevent_default();

    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    {
        use dioxus::web::WebEventExt;
        use wasm_bindgen::JsCast;

        let base_evt = evt.as_web_event();
        if let Some(clipboard_evt) = base_evt.dyn_ref::<web_sys::ClipboardEvent>() {
            if let Some(data_transfer) = clipboard_evt.clipboard_data() {
                // Try our custom type first (internal paste), fall back to text/plain
                let text = data_transfer
                    .get_data("text/x-weaver-md")
                    .ok()
                    .filter(|s| !s.is_empty())
                    .or_else(|| data_transfer.get_data("text/plain").ok());

                if let Some(text) = text {
                    // Delete selection if present
                    let sel = doc.selection.write().take();
                    if let Some(sel) = sel {
                        let (start, end) = (sel.anchor.min(sel.head), sel.anchor.max(sel.head));
                        let _ = doc.remove_tracked(start, end.saturating_sub(start));
                        doc.cursor.write().offset = start;
                    }

                    // Insert pasted text
                    let cursor_offset = doc.cursor.read().offset;
                    let _ = doc.insert_tracked(cursor_offset, &text);
                    doc.cursor.write().offset = cursor_offset + text.chars().count();
                }
            }
        } else {
            tracing::warn!("[PASTE] Failed to cast to ClipboardEvent");
        }
    }
}

/// Handle cut events - extract text, write to clipboard, then delete.
pub fn handle_cut(evt: Event<ClipboardData>, doc: &mut EditorDocument) {
    evt.prevent_default();

    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    {
        use dioxus::web::WebEventExt;
        use wasm_bindgen::JsCast;

        let base_evt = evt.as_web_event();
        if let Some(clipboard_evt) = base_evt.dyn_ref::<web_sys::ClipboardEvent>() {
            let cut_text = {
                let sel = doc.selection.write().take();
                if let Some(sel) = sel {
                    let (start, end) = (sel.anchor.min(sel.head), sel.anchor.max(sel.head));
                    if start != end {
                        // Extract text and strip zero-width chars
                        let selected_text = doc.slice(start, end).unwrap_or_default();
                        let clean_text = selected_text
                            .replace('\u{200C}', "")
                            .replace('\u{200B}', "");

                        // Write to clipboard BEFORE deleting (sync fallback)
                        if let Some(data_transfer) = clipboard_evt.clipboard_data() {
                            if let Err(e) = data_transfer.set_data("text/plain", &clean_text) {
                                tracing::warn!("[CUT] Failed to set clipboard data: {:?}", e);
                            }
                        }

                        // Now delete
                        let _ = doc.remove_tracked(start, end.saturating_sub(start));
                        doc.cursor.write().offset = start;

                        Some(clean_text)
                    } else {
                        None
                    }
                } else {
                    None
                }
            };

            // Async: also write custom MIME type for internal paste detection
            if let Some(text) = cut_text {
                wasm_bindgen_futures::spawn_local(async move {
                    if let Err(e) = write_clipboard_with_custom_type(&text).await {
                        tracing::debug!("[CUT] Async clipboard write failed: {:?}", e);
                    }
                });
            }
        }
    }

    #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
    {
        let _ = evt; // suppress unused warning
    }
}

/// Handle copy events - extract text, clean it up, write to clipboard.
pub fn handle_copy(evt: Event<ClipboardData>, doc: &EditorDocument) {
    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    {
        use dioxus::web::WebEventExt;
        use wasm_bindgen::JsCast;

        let base_evt = evt.as_web_event();
        if let Some(clipboard_evt) = base_evt.dyn_ref::<web_sys::ClipboardEvent>() {
            let sel = *doc.selection.read();
            if let Some(sel) = sel {
                let (start, end) = (sel.anchor.min(sel.head), sel.anchor.max(sel.head));
                if start != end {
                    // Extract text
                    let selected_text = doc.slice(start, end).unwrap_or_default();

                    // Strip zero-width chars used for gap handling
                    let clean_text = selected_text
                        .replace('\u{200C}', "")
                        .replace('\u{200B}', "");

                    // Sync fallback: write text/plain via DataTransfer
                    if let Some(data_transfer) = clipboard_evt.clipboard_data() {
                        if let Err(e) = data_transfer.set_data("text/plain", &clean_text) {
                            tracing::warn!("[COPY] Failed to set clipboard data: {:?}", e);
                        }
                    }

                    // Async: also write custom MIME type for internal paste detection
                    let text_for_async = clean_text.clone();
                    wasm_bindgen_futures::spawn_local(async move {
                        if let Err(e) = write_clipboard_with_custom_type(&text_for_async).await {
                            tracing::debug!("[COPY] Async clipboard write failed: {:?}", e);
                        }
                    });

                    // Prevent browser's default copy (which would copy rendered HTML)
                    evt.prevent_default();
                }
            }
        }
    }

    #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
    {
        let _ = (evt, doc); // suppress unused warnings
    }
}

/// Copy markdown as rendered HTML to clipboard.
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub async fn copy_as_html(markdown: &str) -> Result<(), wasm_bindgen::JsValue> {
    use js_sys::Array;
    use wasm_bindgen::JsValue;
    use web_sys::{Blob, BlobPropertyBag, ClipboardItem};

    // Render markdown to HTML using ClientWriter
    let parser = markdown_weaver::Parser::new(markdown).into_offset_iter();
    let mut html = String::new();
    weaver_renderer::atproto::ClientWriter::<_, _, ()>::new(
        parser.map(|(evt, _range)| evt),
        &mut html,
    )
    .run()
    .map_err(|e| JsValue::from_str(&format!("render error: {e}")))?;

    let window = web_sys::window().ok_or_else(|| JsValue::from_str("no window"))?;
    let clipboard = window.navigator().clipboard();

    // Create blobs for both HTML and plain text (raw HTML for inspection)
    let parts = Array::new();
    parts.push(&JsValue::from_str(&html));

    let mut html_opts = BlobPropertyBag::new();
    html_opts.type_("text/html");
    let html_blob = Blob::new_with_str_sequence_and_options(&parts, &html_opts)?;

    let mut text_opts = BlobPropertyBag::new();
    text_opts.type_("text/plain");
    let text_blob = Blob::new_with_str_sequence_and_options(&parts, &text_opts)?;

    // Create ClipboardItem with both types
    let item_data = js_sys::Object::new();
    js_sys::Reflect::set(&item_data, &JsValue::from_str("text/html"), &html_blob)?;
    js_sys::Reflect::set(&item_data, &JsValue::from_str("text/plain"), &text_blob)?;

    let clipboard_item = ClipboardItem::new_with_record_from_str_to_blob_promise(&item_data)?;
    let items = Array::new();
    items.push(&clipboard_item);

    wasm_bindgen_futures::JsFuture::from(clipboard.write(&items)).await?;
    tracing::info!("[COPY HTML] Success - {} bytes of HTML", html.len());
    Ok(())
}

/// Write text to clipboard with both text/plain and custom MIME type.
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub async fn write_clipboard_with_custom_type(text: &str) -> Result<(), wasm_bindgen::JsValue> {
    use js_sys::{Array, Object, Reflect};
    use wasm_bindgen::JsValue;
    use web_sys::{Blob, BlobPropertyBag, ClipboardItem};

    let window = web_sys::window().ok_or_else(|| JsValue::from_str("no window"))?;
    let navigator = window.navigator();
    let clipboard = navigator.clipboard();

    // Create blobs for each MIME type
    let text_parts = Array::new();
    text_parts.push(&JsValue::from_str(text));

    let mut text_opts = BlobPropertyBag::new();
    text_opts.type_("text/plain");
    let text_blob = Blob::new_with_str_sequence_and_options(&text_parts, &text_opts)?;

    let mut custom_opts = BlobPropertyBag::new();
    custom_opts.type_("text/x-weaver-md");
    let custom_blob = Blob::new_with_str_sequence_and_options(&text_parts, &custom_opts)?;

    // Create ClipboardItem with both types
    let item_data = Object::new();
    Reflect::set(&item_data, &JsValue::from_str("text/plain"), &text_blob)?;
    Reflect::set(
        &item_data,
        &JsValue::from_str("text/x-weaver-md"),
        &custom_blob,
    )?;

    let clipboard_item = ClipboardItem::new_with_record_from_str_to_blob_promise(&item_data)?;
    let items = Array::new();
    items.push(&clipboard_item);

    let promise = clipboard.write(&items);
    wasm_bindgen_futures::JsFuture::from(promise).await?;

    Ok(())
}

/// Describes what kind of list item the cursor is in, if any.
#[derive(Debug, Clone)]
pub enum ListContext {
    /// Unordered list with the given marker char ('-' or '*') and indentation.
    Unordered { indent: String, marker: char },
    /// Ordered list with the current number and indentation.
    Ordered { indent: String, number: usize },
}

/// Detect if cursor is in a list item and return context for continuation.
///
/// Scans backwards to find start of current line, then checks for list marker.
pub fn detect_list_context(text: &loro::LoroText, cursor_offset: usize) -> Option<ListContext> {
    // Find start of current line
    let line_start = find_line_start(text, cursor_offset);

    // Get the line content from start to cursor
    let line_end = find_line_end(text, cursor_offset);
    if line_start >= line_end {
        return None;
    }

    // Extract line text
    let line = text.slice(line_start, line_end).ok()?;

    // Parse indentation
    let indent: String = line
        .chars()
        .take_while(|c| *c == ' ' || *c == '\t')
        .collect();
    let trimmed = &line[indent.len()..];

    // Check for unordered list marker: "- " or "* "
    if trimmed.starts_with("- ") {
        return Some(ListContext::Unordered {
            indent,
            marker: '-',
        });
    }
    if trimmed.starts_with("* ") {
        return Some(ListContext::Unordered {
            indent,
            marker: '*',
        });
    }

    // Check for ordered list marker: "1. ", "2. ", "123. ", etc.
    if let Some(dot_pos) = trimmed.find(". ") {
        let num_part = &trimmed[..dot_pos];
        if !num_part.is_empty() && num_part.chars().all(|c| c.is_ascii_digit()) {
            if let Ok(number) = num_part.parse::<usize>() {
                return Some(ListContext::Ordered { indent, number });
            }
        }
    }

    None
}

/// Check if the current list item is empty (just the marker, no content after cursor).
///
/// Used to determine whether Enter should continue the list or exit it.
pub fn is_list_item_empty(text: &loro::LoroText, cursor_offset: usize, ctx: &ListContext) -> bool {
    let line_start = find_line_start(text, cursor_offset);
    let line_end = find_line_end(text, cursor_offset);

    // Get line content
    let line = match text.slice(line_start, line_end) {
        Ok(s) => s,
        Err(_) => return false,
    };

    // Calculate expected marker length
    let marker_len = match ctx {
        ListContext::Unordered { indent, .. } => indent.len() + 2, // "- "
        ListContext::Ordered { indent, number } => {
            indent.len() + number.to_string().len() + 2 // "1. "
        }
    };

    // Item is empty if line length equals marker length (nothing after marker)
    line.len() <= marker_len
}

/// Get character at the given offset in LoroText.
pub fn get_char_at(text: &loro::LoroText, offset: usize) -> Option<char> {
    text.char_at(offset).ok()
}

/// Find start of line containing offset.
pub fn find_line_start(text: &loro::LoroText, offset: usize) -> usize {
    if offset == 0 {
        return 0;
    }
    // Only slice the portion before cursor
    let prefix = match text.slice(0, offset) {
        Ok(s) => s,
        Err(_) => return 0,
    };
    prefix
        .chars()
        .enumerate()
        .filter(|(_, c)| *c == '\n')
        .last()
        .map(|(pos, _)| pos + 1)
        .unwrap_or(0)
}

/// Find end of line containing offset.
pub fn find_line_end(text: &loro::LoroText, offset: usize) -> usize {
    let char_len = text.len_unicode();
    if offset >= char_len {
        return char_len;
    }
    // Only slice from cursor to end
    let suffix = match text.slice(offset, char_len) {
        Ok(s) => s,
        Err(_) => return char_len,
    };
    suffix
        .chars()
        .enumerate()
        .find(|(_, c)| *c == '\n')
        .map(|(i, _)| offset + i)
        .unwrap_or(char_len)
}
