//! Markdown editor component with Obsidian-style formatting visibility.
//!
//! This module implements a WYSIWYG-like markdown editor where formatting
//! characters are hidden contextually based on cursor position, while still
//! editing plain markdown text under the hood.

mod cursor;
mod document;
mod formatting;
mod offset_map;
mod offsets;
mod render;
mod rope_writer;
mod storage;
mod toolbar;
mod writer;

pub use document::{Affinity, CompositionState, CursorState, EditorDocument, Selection};
pub use formatting::{FormatAction, apply_formatting, find_word_boundaries};
pub use offset_map::{OffsetMapping, RenderResult, find_mapping_for_byte};
pub use render::render_markdown_simple;
pub use rope_writer::RopeWriter;
pub use storage::{EditorSnapshot, clear_storage, load_from_storage, save_to_storage};
pub use toolbar::EditorToolbar;

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

    // Render markdown to HTML with offset mappings
    let render_result = use_memo(move || render::render_markdown_simple(&document().to_string()));
    let rendered_html = use_memo(move || render_result.read().html.clone());
    let offset_map = use_memo(move || render_result.read().offset_map.clone());

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

    // Restore cursor after re-render
    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    use_effect(move || {
        use wasm_bindgen::prelude::*;
        use wasm_bindgen::JsCast;

        let cursor_offset = document().cursor.offset;
        let rope = document().rope.clone();
        let map = offset_map.read().clone();

        // Use requestAnimationFrame to wait for browser paint
        let window = web_sys::window().expect("no window");

        let closure = Closure::once(move || {
            if let Err(e) = cursor::restore_cursor_position(&rope, cursor_offset, &map, editor_id) {
                tracing::warn!("Cursor restoration failed: {:?}", e);
            }
        });

        let _ = window.request_animation_frame(closure.as_ref().unchecked_ref());
        closure.forget();
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
                    dangerous_inner_html: "{rendered_html}",

                    onkeydown: move |evt| {
                        evt.prevent_default();
                        handle_keydown(evt, &mut document);
                    },

                    onpaste: move |evt| {
                        evt.prevent_default();
                        handle_paste(evt, &mut document);
                    },

                    // Phase 1: Accept that cursor position will jump
                    // Phase 2: Restore cursor properly
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

/// Handle paste events and insert text at cursor
fn handle_paste(evt: Event<ClipboardData>, document: &mut Signal<EditorDocument>) {
    // Downcast to web_sys event to get clipboard data
    #[cfg(target_arch = "wasm32")]
    if let Some(web_evt) = evt.data().downcast::<web_sys::ClipboardEvent>() {
        if let Some(data_transfer) = web_evt.clipboard_data() {
            if let Ok(text) = data_transfer.get_data("text/plain") {
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
    }
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
                    // Delete previous char
                    let prev = doc.cursor.offset - 1;
                    doc.rope.remove(prev..doc.cursor.offset);
                    doc.cursor.offset = prev;
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

            Key::ArrowLeft => {
                if mods.ctrl() {
                    // Word boundary (implement later)
                    if doc.cursor.offset > 0 {
                        doc.cursor.offset -= 1;
                    }
                } else if doc.cursor.offset > 0 {
                    doc.cursor.offset -= 1;
                }
                doc.selection = None;
            }

            Key::ArrowRight => {
                if mods.ctrl() {
                    // Word boundary (implement later)
                    if doc.cursor.offset < doc.len_chars() {
                        doc.cursor.offset += 1;
                    }
                } else if doc.cursor.offset < doc.len_chars() {
                    doc.cursor.offset += 1;
                }
                doc.selection = None;
            }

            Key::Enter => {
                if doc.selection.is_some() {
                    let sel = doc.selection.unwrap();
                    let (start, end) = (sel.anchor.min(sel.head), sel.anchor.max(sel.head));
                    doc.rope.remove(start..end);
                    doc.cursor.offset = start;
                    doc.selection = None;
                }
                // Insert two spaces + newline for hard line break
                doc.rope.insert(doc.cursor.offset, "  \n");
                doc.cursor.offset += 3;
            }

            Key::Home => {
                let line_start = find_line_start(&doc.rope, doc.cursor.offset);
                doc.cursor.offset = line_start;
                doc.selection = None;
            }

            Key::End => {
                let line_end = find_line_end(&doc.rope, doc.cursor.offset);
                doc.cursor.offset = line_end;
                doc.selection = None;
            }

            _ => {}
        }
    });
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
