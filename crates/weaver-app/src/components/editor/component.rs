//! The main MarkdownEditor component.

use dioxus::prelude::*;

use crate::components::editor::ReportButton;

use super::document::{CompositionState, EditorDocument};
use super::dom_sync::{sync_cursor_from_dom, update_paragraph_dom};
use super::formatting;
use super::input::{
    get_char_at, handle_copy, handle_cut, handle_keydown, handle_paste, should_intercept_key,
};
use super::paragraph::ParagraphRender;
use super::platform;
use super::publish::PublishButton;
use super::render;
use super::storage;
use super::toolbar::EditorToolbar;
use super::visibility::update_syntax_visibility;
use super::writer::SyntaxSpanInfo;

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
#[component]
pub fn MarkdownEditor(initial_content: Option<String>) -> Element {
    // Try to restore from localStorage (includes CRDT state for undo history)
    // Use "current" as the default draft key for now
    let draft_key = "current";
    let mut document = use_signal(move || {
        storage::load_from_storage(draft_key)
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
        tracing::debug!("DOM update effect triggered");

        // Read document once to avoid multiple borrows
        let doc = document();

        tracing::debug!(
            composition_active = doc.composition.is_some(),
            cursor = doc.cursor.offset,
            "DOM update: checking state"
        );

        // Skip DOM updates during IME composition - browser controls the preview
        if doc.composition.is_some() {
            tracing::debug!("skipping DOM update during composition");
            return;
        }

        tracing::debug!(
            cursor = doc.cursor.offset,
            len = doc.len_chars(),
            "DOM update proceeding (not in composition)"
        );

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
                    if let Err(e) =
                        super::cursor::restore_cursor_position(cursor_offset, &map, editor_id)
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
                document.with_mut(|doc| {
                    doc.sync_loro_cursor();
                    let _ = storage::save_to_storage(doc, draft_key, None);
                });

                // Update last saved frontiers
                last_saved_frontiers.set(Some(current_frontiers));
            }
        });
        interval.forget();
    });

    // Set up beforeinput listener for iOS/Android virtual keyboard quirks
    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    use_effect(move || {
        use wasm_bindgen::JsCast;
        use wasm_bindgen::prelude::*;

        let plat = platform::platform();

        // Only needed on mobile
        if !plat.mobile {
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
        let editor = match dom_document.get_element_by_id(editor_id) {
            Some(e) => e,
            None => return,
        };

        let mut document_signal = document;
        let cached_paras = cached_paragraphs;

        let closure = Closure::wrap(Box::new(move |evt: web_sys::InputEvent| {
            let input_type = evt.input_type();
            tracing::debug!(input_type = %input_type, "beforeinput");

            let plat = platform::platform();

            // iOS workaround: Virtual keyboard sends insertParagraph/insertLineBreak
            // without proper keydown events. Handle them here.
            if plat.ios && (input_type == "insertParagraph" || input_type == "insertLineBreak") {
                tracing::debug!("iOS: intercepting {} via beforeinput", input_type);
                evt.prevent_default();

                // Handle as Enter key
                document_signal.with_mut(|doc| {
                    if let Some(sel) = doc.selection.take() {
                        let (start, end) = (sel.anchor.min(sel.head), sel.anchor.max(sel.head));
                        let _ = doc.remove_tracked(start, end.saturating_sub(start));
                        doc.cursor.offset = start;
                    }

                    if input_type == "insertLineBreak" {
                        // Soft break (like Shift+Enter)
                        let _ = doc.insert_tracked(doc.cursor.offset, "  \n\u{200C}");
                        doc.cursor.offset += 3;
                    } else {
                        // Paragraph break
                        let _ = doc.insert_tracked(doc.cursor.offset, "\n\n");
                        doc.cursor.offset += 2;
                    }
                });
            }

            // Android workaround: When swipe keyboard picks a suggestion,
            // DOM mutations fire before selection moves. We detect this pattern
            // and defer cursor sync.
            if plat.android && input_type == "insertText" {
                // Check if this might be a suggestion pick (has data that looks like a word)
                if let Some(data) = evt.data() {
                    if data.contains(' ') || data.len() > 3 {
                        tracing::debug!("Android: possible suggestion pick, deferring cursor sync");
                        // Defer cursor sync by 20ms to let selection settle
                        let paras = cached_paras;
                        let doc_sig = document_signal;
                        let window = web_sys::window();
                        if let Some(window) = window {
                            let closure = Closure::once(move || {
                                let paras = paras();
                                sync_cursor_from_dom(&mut doc_sig.clone(), editor_id, &paras);
                            });
                            let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(
                                closure.as_ref().unchecked_ref(),
                                20,
                            );
                            closure.forget();
                        }
                    }
                }
            }
        }) as Box<dyn FnMut(web_sys::InputEvent)>);

        let _ = editor
            .add_event_listener_with_callback("beforeinput", closure.as_ref().unchecked_ref());
        closure.forget();
    });

    // Local state for adding new tags
    let mut new_tag = use_signal(String::new);

    rsx! {
        Stylesheet { href: asset!("/assets/styling/editor.css") }
        div { class: "markdown-editor-container",
            // Title bar
            div { class: "editor-title-bar",
                input {
                    r#type: "text",
                    class: "title-input",
                    placeholder: "Entry title...",
                    value: "{document().title()}",
                    oninput: move |e| {
                        document.with_mut(|doc| doc.set_title(&e.value()));
                    },
                }
            }

            // Meta row - path, tags, publish
            div { class: "editor-meta-row",
                    div { class: "meta-path",
                        label { "Path" }
                        input {
                            r#type: "text",
                            class: "path-input",
                            placeholder: "url-slug",
                            value: "{document().path()}",
                            oninput: move |e| {
                                document.with_mut(|doc| doc.set_path(&e.value()));
                            },
                        }
                    }

                    div { class: "meta-tags",
                        label { "Tags" }
                        div { class: "tags-container",
                            for tag in document().tags() {
                                span {
                                    class: "tag-chip",
                                    "{tag}"
                                    button {
                                        class: "tag-remove",
                                        onclick: {
                                            let tag_to_remove = tag.clone();
                                            move |_| {
                                                document.with_mut(|doc| doc.remove_tag(&tag_to_remove));
                                            }
                                        },
                                        "×"
                                    }
                                }
                            }
                            input {
                                r#type: "text",
                                class: "tag-input",
                                placeholder: "Add tag...",
                                value: "{new_tag}",
                                oninput: move |e| new_tag.set(e.value()),
                                onkeydown: move |e| {
                                    use dioxus::prelude::keyboard_types::Key;
                                    if e.key() == Key::Enter && !new_tag().trim().is_empty() {
                                        e.prevent_default();
                                        let tag = new_tag().trim().to_string();
                                        document.with_mut(|doc| doc.add_tag(&tag));
                                        new_tag.set(String::new());
                                    }
                                },
                            }
                        }
                    }

                    PublishButton {
                        document: document,
                        draft_key: draft_key.to_string(),
                    }
                }

                // Editor content
                div { class: "editor-content-wrapper",
                    div {
                        id: "{editor_id}",
                        class: "editor-content",
                        contenteditable: "true",

                        onkeydown: move |evt| {
                        use dioxus::prelude::keyboard_types::Key;
                        use std::time::Duration;

                        let plat = platform::platform();
                        let mods = evt.modifiers();
                        let has_modifier = mods.ctrl() || mods.meta() || mods.alt();

                        // During IME composition:
                        // - Allow modifier shortcuts (Ctrl+B, Ctrl+Z, etc.)
                        // - Allow Escape to cancel composition
                        // - Block text input (let browser handle composition preview)
                        if document.peek().composition.is_some() {
                            if evt.key() == Key::Escape {
                                tracing::debug!("Escape pressed - cancelling composition");
                                document.with_mut(|doc| {
                                    doc.composition = None;
                                });
                                return;
                            }

                            // Allow modifier shortcuts through during composition
                            if !has_modifier {
                                tracing::debug!(
                                    key = ?evt.key(),
                                    "keydown during composition - delegating to browser"
                                );
                                return;
                            }
                            // Fall through to handle the shortcut
                        }

                        // Safari workaround: After Japanese IME composition ends, both
                        // compositionend and keydown fire for Enter. Ignore keydown
                        // within 500ms of composition end to prevent double-newline.
                        if plat.safari && evt.key() == Key::Enter {
                            if let Some(ended_at) = document.peek().composition_ended_at {
                                if ended_at.elapsed() < Duration::from_millis(500) {
                                    tracing::debug!(
                                        "Safari: ignoring Enter within 500ms of compositionend"
                                    );
                                    return;
                                }
                            }
                        }

                        // Android workaround: Chrome Android gets confused by Enter during/after
                        // composition. Defer Enter handling to onkeypress instead.
                        if plat.android && evt.key() == Key::Enter {
                            tracing::debug!("Android: deferring Enter to keypress");
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

                        // Navigation keys (with or without Shift for selection)
                        let navigation = matches!(
                            evt.key(),
                            Key::ArrowLeft | Key::ArrowRight | Key::ArrowUp | Key::ArrowDown |
                            Key::Home | Key::End | Key::PageUp | Key::PageDown
                        );

                        // Cmd/Ctrl+A for select all
                        let select_all = (evt.modifiers().meta() || evt.modifiers().ctrl())
                            && matches!(evt.key(), Key::Character(ref c) if c == "a");

                        if navigation || select_all {
                            let paras = cached_paragraphs();
                            sync_cursor_from_dom(&mut document, editor_id, &paras);
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

                    onselect: move |_evt| {
                        tracing::debug!("onselect fired");
                        let paras = cached_paragraphs();
                        sync_cursor_from_dom(&mut document, editor_id, &paras);
                        let doc = document();
                        let spans = syntax_spans();
                        update_syntax_visibility(
                            doc.cursor.offset,
                            doc.selection.as_ref(),
                            &spans,
                            &paras,
                        );
                    },

                    onselectstart: move |_evt| {
                        tracing::debug!("onselectstart fired");
                        let paras = cached_paragraphs();
                        sync_cursor_from_dom(&mut document, editor_id, &paras);
                        let doc = document();
                        let spans = syntax_spans();
                        update_syntax_visibility(
                            doc.cursor.offset,
                            doc.selection.as_ref(),
                            &spans,
                            &paras,
                        );
                    },

                    onselectionchange: move |_evt| {
                        tracing::debug!("onselectionchange fired");
                        let paras = cached_paragraphs();
                        sync_cursor_from_dom(&mut document, editor_id, &paras);
                        let doc = document();
                        let spans = syntax_spans();
                        update_syntax_visibility(
                            doc.cursor.offset,
                            doc.selection.as_ref(),
                            &spans,
                            &paras,
                        );
                    },

                    onclick: move |_evt| {
                        tracing::debug!("onclick fired");
                        let paras = cached_paragraphs();
                        sync_cursor_from_dom(&mut document, editor_id, &paras);
                        let doc = document();
                        let spans = syntax_spans();
                        update_syntax_visibility(
                            doc.cursor.offset,
                            doc.selection.as_ref(),
                            &spans,
                            &paras,
                        );
                    },

                    // Android workaround: Handle Enter in keypress instead of keydown.
                    // Chrome Android fires confused composition events on Enter in keydown,
                    // but keypress fires after composition state settles.
                    onkeypress: move |evt| {
                        use dioxus::prelude::keyboard_types::Key;

                        let plat = platform::platform();
                        if plat.android && evt.key() == Key::Enter {
                            tracing::debug!("Android: handling Enter in keypress");
                            evt.prevent_default();
                            handle_keydown(evt, &mut document);
                        }
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
                            tracing::debug!("onblur: clearing active composition");
                        }
                        document.with_mut(|doc| {
                            doc.composition = None;
                        });
                    },

                    oncompositionstart: move |evt: CompositionEvent| {
                        let data = evt.data().data();
                        tracing::debug!(
                            data = %data,
                            "compositionstart"
                        );
                        document.with_mut(|doc| {
                            // Delete selection if present (composition replaces it)
                            if let Some(sel) = doc.selection.take() {
                                let (start, end) =
                                    (sel.anchor.min(sel.head), sel.anchor.max(sel.head));
                                tracing::debug!(
                                    start,
                                    end,
                                    "compositionstart: deleting selection"
                                );
                                let _ = doc.remove_tracked(start, end.saturating_sub(start));
                                doc.cursor.offset = start;
                            }

                            tracing::debug!(
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
                        tracing::debug!(
                            data = %data,
                            "compositionupdate"
                        );
                        document.with_mut(|doc| {
                            if let Some(ref mut comp) = doc.composition {
                                comp.text = data;
                            } else {
                                tracing::debug!("compositionupdate without active composition state");
                            }
                        });
                    },

                    oncompositionend: move |evt: CompositionEvent| {
                        let final_text = evt.data().data();
                        tracing::debug!(
                            data = %final_text,
                            "compositionend"
                        );
                        document.with_mut(|doc| {
                            // Record when composition ended for Safari timing workaround
                            doc.composition_ended_at = Some(web_time::Instant::now());

                            if let Some(comp) = doc.composition.take() {
                                tracing::debug!(
                                    start_offset = comp.start_offset,
                                    final_text = %final_text,
                                    chars = final_text.chars().count(),
                                    "compositionend: inserting text"
                                );

                                if !final_text.is_empty() {
                                    let mut delete_start = comp.start_offset;
                                    while delete_start > 0 {
                                        match get_char_at(doc.loro_text(), delete_start - 1) {
                                            Some('\u{200C}') | Some('\u{200B}') => delete_start -= 1,
                                            _ => break,
                                        }
                                    }

                                    let zw_count = doc.cursor.offset - delete_start;
                                    if zw_count > 0 {
                                        // Splice: delete zero-width chars and insert new char in one op
                                        let _ = doc.replace_tracked(delete_start, zw_count, &final_text);
                                        doc.cursor.offset = delete_start + final_text.chars().count();
                                    } else if doc.cursor.offset == doc.len_chars() {
                                        // Fast path: append at end
                                        let _ = doc.push_tracked(&final_text);
                                        doc.cursor.offset = comp.start_offset + final_text.chars().count();
                                    } else {
                                        let _ = doc.insert_tracked(doc.cursor.offset, &final_text);
                                        doc.cursor.offset = comp.start_offset + final_text.chars().count();
                                    }
                                }
                            } else {
                                tracing::debug!("compositionend without active composition state");
                            }
                        });
                    },
                    }

                    // Debug panel snug below editor
                    div { class: "editor-debug",
                        div { "Cursor: {document().cursor.offset}, Chars: {document().len_chars()}" },
                        ReportButton {
                            email: "editor-bugs@weaver.sh".to_string(),
                            editor_id: "markdown-editor".to_string(),
                        }
                    }
                }

            // Toolbar in grid column 2, row 3
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
