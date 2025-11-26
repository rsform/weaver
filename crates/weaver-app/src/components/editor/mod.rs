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
mod storage;
mod toolbar;
mod visibility;
mod writer;

#[cfg(test)]
mod tests;

pub use document::{Affinity, CompositionState, CursorState, EditorDocument, Selection};
pub use formatting::{FormatAction, apply_formatting, find_word_boundaries};
pub use offset_map::{OffsetMapping, RenderResult, find_mapping_for_byte};
pub use paragraph::ParagraphRender;
pub use render::{RenderCache, render_paragraphs_incremental};
pub use storage::{EditorSnapshot, clear_storage, load_from_storage, save_to_storage};
pub use toolbar::EditorToolbar;
pub use visibility::VisibilityState;
pub use writer::{SyntaxSpanInfo, SyntaxType, WriterResult};

use dioxus::prelude::*;

/// Main markdown editor component.
///
/// # Props
/// - `initial_content`: Optional initial markdown content
///
/// # Features
/// - Loro CRDT-based text storage with undo/redo support
/// - Event interception for full control over editing operations
/// - Toolbar formatting buttons
/// - LocalStorage auto-save with debouncing
/// - Keyboard shortcuts (Ctrl+B for bold, Ctrl+I for italic)
///
/// # Phase 1 Limitations (mostly resolved)
/// - Cursor jumps to end after each keystroke (acceptable for MVP)
/// - All formatting characters visible (no hiding based on cursor position) - RESOLVED
/// - No proper grapheme cluster handling
/// - No undo/redo - RESOLVED (Loro UndoManager)
/// - No selection with Shift+Arrow
/// - No mouse selection - RESOLVED
#[component]
pub fn MarkdownEditor(initial_content: Option<String>) -> Element {
    // Try to restore from localStorage (includes CRDT state for undo history)
    let mut document = use_signal(move || {
        storage::load_from_storage()
            .unwrap_or_else(|| EditorDocument::new(initial_content.clone().unwrap_or_default()))
    });
    let editor_id = "markdown-editor";

    // Cache for incremental paragraph rendering
    let mut render_cache = use_signal(|| render::RenderCache::default());

    // Render paragraphs with incremental caching
    let paragraphs = use_memo(move || {
        let doc = document();
        let cache = render_cache.peek();
        let edit = doc.last_edit.as_ref();

        let (paras, new_cache) =
            render::render_paragraphs_incremental(doc.loro_text(), Some(&cache), edit);

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

    // Flatten syntax spans from all paragraphs
    let syntax_spans = use_memo(move || {
        paragraphs()
            .iter()
            .flat_map(|p| p.syntax_spans.iter().cloned())
            .collect::<Vec<_>>()
    });

    // Cache paragraphs for change detection AND for event handlers to access
    let mut cached_paragraphs = use_signal(|| Vec::<ParagraphRender>::new());

    // Update DOM when paragraphs change (incremental rendering)
    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    use_effect(move || {
        tracing::info!("DOM update effect triggered");

        // Read document once to avoid multiple borrows
        let doc = document();

        tracing::info!(
            composition_active = doc.composition.is_some(),
            cursor = doc.cursor.offset,
            "DOM update: checking state"
        );

        // Skip DOM updates during IME composition - browser controls the preview
        if doc.composition.is_some() {
            tracing::info!("skipping DOM update during composition");
            return;
        }

        let cursor_offset = doc.cursor.offset;
        let selection = doc.selection;
        drop(doc); // Release borrow before other operations

        let new_paras = paragraphs();
        let map = offset_map();
        let spans = syntax_spans();

        // Use peek() to avoid creating reactive dependency on cached_paragraphs
        let prev = cached_paragraphs.peek().clone();

        let cursor_para_updated = update_paragraph_dom(editor_id, &prev, &new_paras, cursor_offset);

        // Only restore cursor if we actually re-rendered the paragraph it's in
        if cursor_para_updated {
            use wasm_bindgen::JsCast;
            use wasm_bindgen::prelude::*;

            // Use requestAnimationFrame to wait for browser paint
            if let Some(window) = web_sys::window() {
                let closure = Closure::once(move || {
                    if let Err(e) = cursor::restore_cursor_position(cursor_offset, &map, editor_id)
                    {
                        tracing::warn!("Cursor restoration failed: {:?}", e);
                    }
                });

                let _ = window.request_animation_frame(closure.as_ref().unchecked_ref());
                closure.forget();
            }
        }

        // Store for next comparison AND for event handlers (write-only, no reactive read)
        cached_paragraphs.set(new_paras.clone());

        // Update syntax visibility after DOM changes
        update_syntax_visibility(cursor_offset, selection.as_ref(), &spans, &new_paras);
    });

    // Track last saved frontiers to detect changes (peek-only, no subscriptions)
    let mut last_saved_frontiers: Signal<Option<loro::Frontiers>> = use_signal(|| None);

    // Auto-save with periodic check (no reactive dependency to avoid loops)
    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    use_effect(move || {
        // Check every 500ms if there are unsaved changes
        let interval = gloo_timers::callback::Interval::new(500, move || {
            // Peek both signals without creating reactive dependencies
            let current_frontiers = document.peek().state_frontiers();

            // Only save if frontiers changed (document was edited)
            let needs_save = {
                let last_frontiers = last_saved_frontiers.peek();
                match &*last_frontiers {
                    None => true, // First save
                    Some(last) => &current_frontiers != last,
                }
            }; // drop last_frontiers borrow here

            if needs_save {
                // Sync cursor and extract data for save
                let (content, cursor_offset, loro_cursor, snapshot_bytes) =
                    document.with_mut(|doc| {
                        doc.sync_loro_cursor();
                        (
                            doc.to_string(),
                            doc.cursor.offset,
                            doc.loro_cursor().cloned(),
                            doc.export_snapshot(),
                        )
                    });

                use gloo_storage::Storage as _; // bring trait into scope for LocalStorage::set
                let snapshot_b64 = if snapshot_bytes.is_empty() {
                    None
                } else {
                    Some(base64::Engine::encode(
                        &base64::engine::general_purpose::STANDARD,
                        &snapshot_bytes,
                    ))
                };
                let snapshot = storage::EditorSnapshot {
                    content,
                    snapshot: snapshot_b64,
                    cursor: loro_cursor,
                    cursor_offset,
                };
                let _ = gloo_storage::LocalStorage::set("weaver_editor_draft", &snapshot);

                // Update last saved frontiers
                last_saved_frontiers.set(Some(current_frontiers));
            }
        });
        interval.forget();
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
                        use dioxus::prelude::keyboard_types::Key;

                        // During IME composition, let browser handle everything
                        // Exception: Escape cancels composition
                        if document.peek().composition.is_some() {
                            tracing::info!(
                                key = ?evt.key(),
                                "keydown during composition - delegating to browser"
                            );
                            if evt.key() == Key::Escape {
                                tracing::info!("Escape pressed - cancelling composition");
                                document.with_mut(|doc| {
                                    doc.composition = None;
                                });
                            }
                            return;
                        }

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
                            // Update syntax visibility after cursor sync
                            let doc = document();
                            let spans = syntax_spans();
                            update_syntax_visibility(
                                doc.cursor.offset,
                                doc.selection.as_ref(),
                                &spans,
                                &paras,
                            );
                        }
                    },

                    onclick: move |_evt| {
                        // After mouse click or drag selection, sync cursor from DOM
                        // (click fires after mouseup, so this handles both cases)
                        let paras = cached_paragraphs();
                        sync_cursor_from_dom(&mut document, editor_id, &paras);
                        // Update syntax visibility after cursor sync
                        let doc = document();
                        let spans = syntax_spans();
                        update_syntax_visibility(
                            doc.cursor.offset,
                            doc.selection.as_ref(),
                            &spans,
                            &paras,
                        );
                    },

                    onpaste: move |evt| {
                        handle_paste(evt, &mut document);
                    },

                    oncut: move |evt| {
                        handle_cut(evt, &mut document);
                    },

                    oncopy: move |evt| {
                        handle_copy(evt, &document);
                    },

                    onblur: move |_| {
                        // Cancel any in-progress IME composition on focus loss
                        let had_composition = document.peek().composition.is_some();
                        if had_composition {
                            tracing::info!("onblur: clearing active composition");
                        }
                        document.with_mut(|doc| {
                            doc.composition = None;
                        });
                    },

                    oncompositionstart: move |evt: CompositionEvent| {
                        let data = evt.data().data();
                        tracing::info!(
                            data = %data,
                            "compositionstart"
                        );
                        document.with_mut(|doc| {
                            // Delete selection if present (composition replaces it)
                            if let Some(sel) = doc.selection.take() {
                                let (start, end) =
                                    (sel.anchor.min(sel.head), sel.anchor.max(sel.head));
                                tracing::info!(
                                    start,
                                    end,
                                    "compositionstart: deleting selection"
                                );
                                let _ = doc.remove_tracked(start, end.saturating_sub(start));
                                doc.cursor.offset = start;
                            }

                            tracing::info!(
                                cursor = doc.cursor.offset,
                                "compositionstart: setting composition state"
                            );
                            doc.composition = Some(CompositionState {
                                start_offset: doc.cursor.offset,
                                text: data,
                            });
                        });
                    },

                    oncompositionupdate: move |evt: CompositionEvent| {
                        let data = evt.data().data();
                        tracing::info!(
                            data = %data,
                            "compositionupdate"
                        );
                        document.with_mut(|doc| {
                            if let Some(ref mut comp) = doc.composition {
                                comp.text = data;
                            } else {
                                tracing::info!("compositionupdate without active composition state");
                            }
                        });
                    },

                    oncompositionend: move |evt: CompositionEvent| {
                        let final_text = evt.data().data();
                        tracing::info!(
                            data = %final_text,
                            "compositionend"
                        );
                        document.with_mut(|doc| {
                            if let Some(comp) = doc.composition.take() {
                                tracing::info!(
                                    start_offset = comp.start_offset,
                                    final_text = %final_text,
                                    chars = final_text.chars().count(),
                                    "compositionend: inserting text"
                                );

                                if !final_text.is_empty() {
                                    let _ = doc.insert_tracked(comp.start_offset, &final_text);
                                    doc.cursor.offset =
                                        comp.start_offset + final_text.chars().count();
                                }
                            } else {
                                tracing::info!("compositionend without active composition state");
                            }
                        });
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
            // Intercept our shortcuts: formatting (b/i), undo/redo (z/y), HTML export (e)
            match ch.as_str() {
                "b" | "i" | "z" | "y" => return true,
                "e" => return true, // Ctrl+E for HTML export/copy
                _ => {}
            }
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
                } else {
                    // Collapsed selection (just cursor)
                    doc.selection = None;
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

/// Update syntax span visibility in the DOM based on cursor position.
///
/// Toggles the "hidden" class on syntax spans based on calculated visibility.
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
fn update_syntax_visibility(
    cursor_offset: usize,
    selection: Option<&Selection>,
    syntax_spans: &[SyntaxSpanInfo],
    paragraphs: &[ParagraphRender],
) {
    let visibility =
        visibility::VisibilityState::calculate(cursor_offset, selection, syntax_spans, paragraphs);

    let Some(window) = web_sys::window() else {
        return;
    };
    let Some(document) = window.document() else {
        return;
    };

    // Update each syntax span's visibility
    for span in syntax_spans {
        let selector = format!("[data-syn-id='{}']", span.syn_id);
        if let Ok(Some(element)) = document.query_selector(&selector) {
            let class_list = element.class_list();
            if visibility.is_visible(&span.syn_id) {
                let _ = class_list.remove_1("hidden");
            } else {
                let _ = class_list.add_1("hidden");
            }
        }
    }
}

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
fn update_syntax_visibility(
    _cursor_offset: usize,
    _selection: Option<&Selection>,
    _syntax_spans: &[SyntaxSpanInfo],
    _paragraphs: &[ParagraphRender],
) {
    // No-op on non-wasm
}

/// Handle paste events and insert text at cursor
fn handle_paste(evt: Event<ClipboardData>, document: &mut Signal<EditorDocument>) {
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
                    document.with_mut(|doc| {
                        // Delete selection if present
                        if let Some(sel) = doc.selection {
                            let (start, end) = (sel.anchor.min(sel.head), sel.anchor.max(sel.head));
                            let _ = doc.remove_tracked(start, end.saturating_sub(start));
                            doc.cursor.offset = start;
                            doc.selection = None;
                        }

                        // Insert pasted text
                        let _ = doc.insert_tracked(doc.cursor.offset, &text);
                        doc.cursor.offset += text.chars().count();
                    });
                }
            }
        } else {
            tracing::warn!("[PASTE] Failed to cast to ClipboardEvent");
        }
    }
}

/// Handle cut events - extract text, write to clipboard, then delete
fn handle_cut(evt: Event<ClipboardData>, document: &mut Signal<EditorDocument>) {
    evt.prevent_default();

    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    {
        use dioxus::web::WebEventExt;
        use wasm_bindgen::JsCast;

        let base_evt = evt.as_web_event();
        if let Some(clipboard_evt) = base_evt.dyn_ref::<web_sys::ClipboardEvent>() {
            let cut_text = document.with_mut(|doc| {
                if let Some(sel) = doc.selection {
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
                        doc.cursor.offset = start;
                        doc.selection = None;

                        return Some(clean_text);
                    }
                }
                None
            });

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

/// Handle copy events - extract text, clean it up, write to clipboard
fn handle_copy(evt: Event<ClipboardData>, document: &Signal<EditorDocument>) {
    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    {
        use dioxus::web::WebEventExt;
        use wasm_bindgen::JsCast;

        let base_evt = evt.as_web_event();
        if let Some(clipboard_evt) = base_evt.dyn_ref::<web_sys::ClipboardEvent>() {
            let doc = document.read();
            if let Some(sel) = doc.selection {
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
        let _ = (evt, document); // suppress unused warnings
    }
}

/// Copy markdown as rendered HTML to clipboard
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
async fn copy_as_html(markdown: &str) -> Result<(), wasm_bindgen::JsValue> {
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

/// Write text to clipboard with both text/plain and custom MIME type
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
async fn write_clipboard_with_custom_type(text: &str) -> Result<(), wasm_bindgen::JsValue> {
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

/// Extract a slice of text from a string by char indices
fn extract_text_slice(text: &str, start: usize, end: usize) -> String {
    text.chars()
        .skip(start)
        .take(end.saturating_sub(start))
        .collect()
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
                        "z" => {
                            if mods.shift() {
                                // Ctrl+Shift+Z = redo
                                if let Ok(true) = doc.redo() {
                                    // Cursor position should be handled by the undo manager
                                    // but we may need to clamp it
                                    doc.cursor.offset = doc.cursor.offset.min(doc.len_chars());
                                }
                            } else {
                                // Ctrl+Z = undo
                                if let Ok(true) = doc.undo() {
                                    doc.cursor.offset = doc.cursor.offset.min(doc.len_chars());
                                }
                            }
                            doc.selection = None;
                            return;
                        }
                        "y" => {
                            // Ctrl+Y = redo (alternative)
                            if let Ok(true) = doc.redo() {
                                doc.cursor.offset = doc.cursor.offset.min(doc.len_chars());
                            }
                            doc.selection = None;
                            return;
                        }
                        "e" => {
                            // Ctrl+E = copy as HTML (export)
                            if let Some(sel) = doc.selection {
                                let (start, end) =
                                    (sel.anchor.min(sel.head), sel.anchor.max(sel.head));
                                if start != end {
                                    if let Some(markdown) = doc.slice(start, end) {
                                        let clean_md = markdown
                                            .replace('\u{200C}', "")
                                            .replace('\u{200B}', "");
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
                if let Some(sel) = doc.selection.take() {
                    let (start, end) = (sel.anchor.min(sel.head), sel.anchor.max(sel.head));
                    let _ = doc.replace_tracked(start, end.saturating_sub(start), &ch);
                    doc.cursor.offset = start + ch.chars().count();
                } else {
                    // Clean up any preceding zero-width chars (gap scaffolding)
                    let mut delete_start = doc.cursor.offset;
                    while delete_start > 0 {
                        match get_char_at(doc.loro_text(), delete_start - 1) {
                            Some('\u{200C}') | Some('\u{200B}') => delete_start -= 1,
                            _ => break,
                        }
                    }

                    let zw_count = doc.cursor.offset - delete_start;
                    if zw_count > 0 {
                        // Splice: delete zero-width chars and insert new char in one op
                        let _ = doc.replace_tracked(delete_start, zw_count, &ch);
                        doc.cursor.offset = delete_start + ch.chars().count();
                    } else if doc.cursor.offset == doc.len_chars() {
                        // Fast path: append at end
                        let _ = doc.push_tracked(&ch);
                        doc.cursor.offset += ch.chars().count();
                    } else {
                        let _ = doc.insert_tracked(doc.cursor.offset, &ch);
                        doc.cursor.offset += ch.chars().count();
                    }
                }
            }

            Key::Backspace => {
                if let Some(sel) = doc.selection {
                    // Delete selection
                    let (start, end) = (sel.anchor.min(sel.head), sel.anchor.max(sel.head));
                    let _ = doc.remove_tracked(start, end.saturating_sub(start));
                    doc.cursor.offset = start;
                    doc.selection = None;
                } else if doc.cursor.offset > 0 {
                    // Check if we're about to delete a newline
                    let prev_char = get_char_at(doc.loro_text(), doc.cursor.offset - 1);

                    if prev_char == Some('\n') {
                        let newline_pos = doc.cursor.offset - 1;
                        let mut delete_start = newline_pos;
                        let mut delete_end = doc.cursor.offset;

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
                                Some(' ') | Some('\t') | Some('\u{200C}') | Some('\u{200B}') => {
                                    delete_start -= 1;
                                }
                                Some('\n') => break, // stop at another newline
                                _ => break,          // stop at actual content
                            }
                        }

                        // Delete from where we stopped to end (including any trailing zero-width)
                        let _ = doc
                            .remove_tracked(delete_start, delete_end.saturating_sub(delete_start));
                        doc.cursor.offset = delete_start;
                    } else {
                        // Normal backspace - delete one char
                        let prev = doc.cursor.offset - 1;
                        let _ = doc.remove_tracked(prev, 1);
                        doc.cursor.offset = prev;
                    }
                }
            }

            Key::Delete => {
                if let Some(sel) = doc.selection.take() {
                    // Delete selection
                    let (start, end) = (sel.anchor.min(sel.head), sel.anchor.max(sel.head));
                    let _ = doc.remove_tracked(start, end.saturating_sub(start));
                    doc.cursor.offset = start;
                } else if doc.cursor.offset < doc.len_chars() {
                    // Delete next char
                    let _ = doc.remove_tracked(doc.cursor.offset, 1);
                }
            }

            // Arrow keys handled by browser, synced in onkeyup
            Key::ArrowLeft | Key::ArrowRight | Key::ArrowUp | Key::ArrowDown => {
                // Browser handles these naturally
            }

            Key::Enter => {
                if let Some(sel) = doc.selection.take() {
                    let (start, end) = (sel.anchor.min(sel.head), sel.anchor.max(sel.head));
                    let _ = doc.remove_tracked(start, end.saturating_sub(start));
                    doc.cursor.offset = start;
                }

                if mods.shift() {
                    // Shift+Enter: hard line break (soft break)
                    let _ = doc.insert_tracked(doc.cursor.offset, "  \n\u{200C}");
                    doc.cursor.offset += 3;
                } else if let Some(ctx) = detect_list_context(doc.loro_text(), doc.cursor.offset) {
                    // We're in a list item
                    if is_list_item_empty(doc.loro_text(), doc.cursor.offset, &ctx) {
                        // Empty item - exit list by removing marker and inserting paragraph break
                        let line_start = find_line_start(doc.loro_text(), doc.cursor.offset);
                        let line_end = find_line_end(doc.loro_text(), doc.cursor.offset);

                        // Delete the empty list item line INCLUDING its trailing newline
                        // line_end points to the newline, so +1 to include it
                        let delete_end = (line_end + 1).min(doc.len_chars());

                        // Use replace_tracked to atomically delete line and insert paragraph break
                        let _ = doc.replace_tracked(
                            line_start,
                            delete_end.saturating_sub(line_start),
                            "\n\n\u{200C}\n",
                        );
                        doc.cursor.offset = line_start + 2;
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
                        let _ = doc.insert_tracked(doc.cursor.offset, &continuation);
                        doc.cursor.offset += len;
                    }
                } else {
                    // Not in a list - normal paragraph break
                    let _ = doc.insert_tracked(doc.cursor.offset, "\n\n");
                    doc.cursor.offset += 2;
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
        if doc.last_edit.as_ref().is_some_and(|e| e.contains_newline) {
            doc.sync_loro_cursor();
        }
    });
}

/// Describes what kind of list item the cursor is in, if any
#[derive(Debug, Clone)]
enum ListContext {
    /// Unordered list with the given marker char ('-' or '*') and indentation
    Unordered { indent: String, marker: char },
    /// Ordered list with the current number and indentation
    Ordered { indent: String, number: usize },
}

/// Detect if cursor is in a list item and return context for continuation.
///
/// Scans backwards to find start of current line, then checks for list marker.
fn detect_list_context(text: &loro::LoroText, cursor_offset: usize) -> Option<ListContext> {
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
fn is_list_item_empty(text: &loro::LoroText, cursor_offset: usize, ctx: &ListContext) -> bool {
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

/// Get character at the given offset in LoroText
fn get_char_at(text: &loro::LoroText, offset: usize) -> Option<char> {
    text.char_at(offset).ok()
}

/// Find start of line containing offset
fn find_line_start(text: &loro::LoroText, offset: usize) -> usize {
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

/// Find end of line containing offset
fn find_line_end(text: &loro::LoroText, offset: usize) -> usize {
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
