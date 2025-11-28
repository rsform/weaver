//! The main MarkdownEditor component.

use dioxus::prelude::*;
use jacquard::IntoStatic;
use jacquard::cowstr::ToCowStr;
use jacquard::identity::resolver::IdentityResolver;
use jacquard::types::blob::BlobRef;
use jacquard::types::ident::AtIdentifier;
use weaver_api::sh_weaver::embed::images::Image;
use weaver_common::WeaverExt;

use crate::auth::AuthState;
use crate::components::editor::ReportButton;
use crate::fetch::Fetcher;

use super::document::{CompositionState, EditorDocument};
use super::dom_sync::{
    sync_cursor_from_dom, sync_cursor_from_dom_with_direction, update_paragraph_dom,
};
use super::formatting;
use super::input::{
    get_char_at, handle_copy, handle_cut, handle_keydown, handle_paste, should_intercept_key,
};
use super::offset_map::SnapDirection;
use super::paragraph::ParagraphRender;
use super::platform;
use super::document::LoadedDocState;
use super::publish::{LoadedEntry, PublishButton, load_entry_for_editing};
use super::render;
use super::storage;
use super::sync::{SyncStatus, load_and_merge_document};
use super::toolbar::EditorToolbar;
use super::visibility::update_syntax_visibility;
use super::writer::{EditorImageResolver, SyntaxSpanInfo};

/// Result of loading document state.
enum LoadResult {
    /// Document state loaded (may be merged from PDS + localStorage)
    Loaded(LoadedDocState),
    /// Loading failed
    Failed(String),
    /// Still loading
    Loading,
}

/// Wrapper component that handles loading document state before rendering the editor.
///
/// Loads and merges state from:
/// - localStorage (local CRDT snapshot)
/// - PDS edit state (if editing published entry)
/// - Entry content (if no edit state exists)
///
/// # Props
/// - `initial_content`: Optional initial markdown content (for new entries)
/// - `entry_uri`: Optional AT-URI of an existing entry to edit
#[component]
pub fn MarkdownEditor(initial_content: Option<String>, entry_uri: Option<String>) -> Element {
    let fetcher = use_context::<Fetcher>();

    // Determine draft key - use entry URI if editing existing, otherwise generate TID
    let draft_key = use_hook(|| {
        entry_uri.clone().unwrap_or_else(|| {
            format!("new:{}", jacquard::types::tid::Ticker::new().next(None).as_str())
        })
    });

    // Parse entry URI once
    let parsed_uri = entry_uri.as_ref().and_then(|s| {
        jacquard::types::string::AtUri::new(s).ok().map(|u| u.into_static())
    });

    // Clone draft_key for render (resource closure moves it)
    let draft_key_for_render = draft_key.clone();

    // Resource loads and merges document state
    let load_resource = use_resource(move || {
        let fetcher = fetcher.clone();
        let draft_key = draft_key.clone();
        let entry_uri = parsed_uri.clone();
        let initial_content = initial_content.clone();

        async move {
            // Try to load merged state from PDS + localStorage
            match load_and_merge_document(&fetcher, &draft_key, entry_uri.as_ref()).await {
                Ok(Some(state)) => {
                    tracing::debug!("Loaded merged document state");
                    return LoadResult::Loaded(state);
                }
                Ok(None) => {
                    // No existing state - check if we need to load entry content
                    if let Some(ref uri) = entry_uri {
                        // Check that this entry belongs to the current user
                        if let Some(current_did) = fetcher.current_did().await {
                            let entry_authority = uri.authority();
                            let is_own_entry = match entry_authority {
                                AtIdentifier::Did(did) => did == &current_did,
                                AtIdentifier::Handle(handle) => {
                                    // Resolve handle to DID and compare
                                    match fetcher.client.resolve_handle(handle).await {
                                        Ok(resolved_did) => resolved_did == current_did,
                                        Err(_) => false,
                                    }
                                }
                            };
                            if !is_own_entry {
                                tracing::warn!(
                                    "Cannot edit entry belonging to another user: {}",
                                    entry_authority
                                );
                                return LoadResult::Failed(
                                    "You can only edit your own entries".to_string()
                                );
                            }
                        }

                        // Try to load the entry content from PDS
                        match load_entry_for_editing(&fetcher, uri).await {
                            Ok(loaded) => {
                                // Create LoadedDocState from entry
                                let doc = loro::LoroDoc::new();
                                let content = doc.get_text("content");
                                let title = doc.get_text("title");
                                let path = doc.get_text("path");
                                let tags = doc.get_list("tags");

                                content.insert(0, loaded.entry.content.as_ref()).ok();
                                title.insert(0, loaded.entry.title.as_ref()).ok();
                                path.insert(0, loaded.entry.path.as_ref()).ok();
                                if let Some(ref entry_tags) = loaded.entry.tags {
                                    for tag in entry_tags {
                                        let tag_str: &str = tag.as_ref();
                                        tags.push(tag_str).ok();
                                    }
                                }
                                doc.commit();

                                return LoadResult::Loaded(LoadedDocState {
                                    doc,
                                    entry_ref: Some(loaded.entry_ref),
                                    edit_root: None,
                                    last_diff: None,
                                    synced_version: None, // Fresh from entry, never synced
                                });
                            }
                            Err(e) => {
                                tracing::error!("Failed to load entry: {}", e);
                                return LoadResult::Failed(e.to_string());
                            }
                        }
                    }

                    // New document with initial content
                    let doc = loro::LoroDoc::new();
                    if let Some(ref content) = initial_content {
                        let text = doc.get_text("content");
                        text.insert(0, content).ok();
                        doc.commit();
                    }

                    LoadResult::Loaded(LoadedDocState {
                        doc,
                        entry_ref: None,
                        edit_root: None,
                        last_diff: None,
                        synced_version: None, // New doc, never synced
                    })
                }
                Err(e) => {
                    tracing::error!("Failed to load document state: {}", e);
                    LoadResult::Failed(e.to_string())
                }
            }
        }
    });

    // Render based on load state
    match &*load_resource.read() {
        Some(LoadResult::Loaded(state)) => {
            rsx! {
                MarkdownEditorInner {
                    key: "{draft_key_for_render}",
                    draft_key: draft_key_for_render.clone(),
                    loaded_state: state.clone(),
                }
            }
        }
        Some(LoadResult::Failed(err)) => {
            rsx! {
                div { class: "editor-error",
                    "Failed to load: {err}"
                }
            }
        }
        Some(LoadResult::Loading) | None => {
            rsx! {
                div { class: "editor-loading",
                    "Loading..."
                }
            }
        }
    }
}

/// Inner markdown editor component (actual editor implementation).
///
/// # Features
/// - Loro CRDT-based text storage with undo/redo support
/// - Event interception for full control over editing operations
/// - Toolbar formatting buttons
/// - LocalStorage auto-save with debouncing
/// - PDS sync with auto-save
/// - Keyboard shortcuts (Ctrl+B for bold, Ctrl+I for italic)
#[component]
fn MarkdownEditorInner(
    draft_key: String,
    loaded_state: LoadedDocState,
) -> Element {
    // Context for authenticated API calls
    let fetcher = use_context::<Fetcher>();
    let auth_state = use_context::<Signal<AuthState>>();

    // Create EditorDocument from loaded state (must be in use_hook for Signals)
    let mut document = use_hook(|| {
        let doc = EditorDocument::from_loaded_state(loaded_state.clone());
        // Save to localStorage so we have a local backup
        storage::save_to_storage(&doc, &draft_key).ok();
        doc
    });
    let editor_id = "markdown-editor";

    // Cache for incremental paragraph rendering
    let mut render_cache = use_signal(|| render::RenderCache::default());

    // Image resolver for mapping /image/{name} to data URLs or CDN URLs
    let mut image_resolver = use_signal(EditorImageResolver::default);

    // Render paragraphs with incremental caching
    // Reads document.last_edit signal - creates dependency on content changes only
    let doc_for_memo = document.clone();
    let paragraphs = use_memo(move || {
        let edit = doc_for_memo.last_edit(); // Signal read - reactive dependency
        let cache = render_cache.peek();
        let resolver = image_resolver();

        let (paras, new_cache) = render::render_paragraphs_incremental(
            doc_for_memo.loro_text(),
            Some(&cache),
            edit.as_ref(),
            Some(&resolver),
        );

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
    let mut doc_for_dom = document.clone();
    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    use_effect(move || {
        tracing::debug!("DOM update effect triggered");

        tracing::debug!(
            composition_active = doc_for_dom.composition.read().is_some(),
            cursor = doc_for_dom.cursor.read().offset,
            "DOM update: checking state"
        );

        // Skip DOM updates during IME composition - browser controls the preview
        if doc_for_dom.composition.read().is_some() {
            tracing::debug!("skipping DOM update during composition");
            return;
        }

        tracing::debug!(
            cursor = doc_for_dom.cursor.read().offset,
            len = doc_for_dom.len_chars(),
            "DOM update proceeding (not in composition)"
        );

        let cursor_offset = doc_for_dom.cursor.read().offset;
        let selection = *doc_for_dom.selection.read();

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

            // Read and consume pending snap direction
            let snap_direction = doc_for_dom.pending_snap.write().take();

            // Use requestAnimationFrame to wait for browser paint
            if let Some(window) = web_sys::window() {
                let closure = Closure::once(move || {
                    if let Err(e) = super::cursor::restore_cursor_position(
                        cursor_offset,
                        &map,
                        editor_id,
                        snap_direction,
                    ) {
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
    let doc_for_autosave = document.clone();
    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    let draft_key_for_autosave = draft_key.clone();
    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    use_effect(move || {
        // Check every 500ms if there are unsaved changes
        let mut doc = doc_for_autosave.clone();
        let draft_key = draft_key_for_autosave.clone();
        let interval = gloo_timers::callback::Interval::new(500, move || {
            let current_frontiers = doc.state_frontiers();

            // Only save if frontiers changed (document was edited)
            let needs_save = {
                let last_frontiers = last_saved_frontiers.peek();
                match &*last_frontiers {
                    None => true,
                    Some(last) => &current_frontiers != last,
                }
            }; // drop last_frontiers borrow here

            if needs_save {
                doc.sync_loro_cursor();
                let _ = storage::save_to_storage(&doc, &draft_key);

                // Update last saved frontiers
                last_saved_frontiers.set(Some(current_frontiers));
            }
        });
        interval.forget();
    });

    // Set up beforeinput listener for iOS/Android virtual keyboard quirks
    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    let doc_for_beforeinput = document.clone();
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

        let mut doc = doc_for_beforeinput.clone();
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
                let sel = doc.selection.write().take();
                if let Some(sel) = sel {
                    let (start, end) = (sel.anchor.min(sel.head), sel.anchor.max(sel.head));
                    let _ = doc.remove_tracked(start, end.saturating_sub(start));
                    doc.cursor.write().offset = start;
                }

                let cursor_offset = doc.cursor.read().offset;
                if input_type == "insertLineBreak" {
                    // Soft break (like Shift+Enter)
                    let _ = doc.insert_tracked(cursor_offset, "  \n\u{200C}");
                    doc.cursor.write().offset = cursor_offset + 3;
                } else {
                    // Paragraph break
                    let _ = doc.insert_tracked(cursor_offset, "\n\n");
                    doc.cursor.write().offset = cursor_offset + 2;
                }
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
                        let mut doc_for_timeout = doc.clone();
                        let window = web_sys::window();
                        if let Some(window) = window {
                            let closure = Closure::once(move || {
                                let paras = paras();
                                sync_cursor_from_dom(&mut doc_for_timeout, editor_id, &paras);
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
                    value: "{document.title()}",
                    oninput: {
                        let doc = document.clone();
                        move |e| {
                            doc.set_title(&e.value());
                        }
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
                            value: "{document.path()}",
                            oninput: {
                                let doc = document.clone();
                                move |e| {
                                    doc.set_path(&e.value());
                                }
                            },
                        }
                    }

                    div { class: "meta-tags",
                        label { "Tags" }
                        div { class: "tags-container",
                            for tag in document.tags() {
                                span {
                                    class: "tag-chip",
                                    "{tag}"
                                    button {
                                        class: "tag-remove",
                                        onclick: {
                                            let doc = document.clone();
                                            let tag_to_remove = tag.clone();
                                            move |_| {
                                                doc.remove_tag(&tag_to_remove);
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
                                onkeydown: {
                                    let doc = document.clone();
                                    move |e| {
                                        use dioxus::prelude::keyboard_types::Key;
                                        if e.key() == Key::Enter && !new_tag().trim().is_empty() {
                                            e.prevent_default();
                                            let tag = new_tag().trim().to_string();
                                            doc.add_tag(&tag);
                                            new_tag.set(String::new());
                                        }
                                    }
                                },
                            }
                        }
                    }

                    div { class: "meta-actions",
                        SyncStatus {
                            document: document.clone(),
                            draft_key: draft_key.to_string(),
                        }

                        PublishButton {
                            document: document.clone(),
                            draft_key: draft_key.to_string(),
                        }
                    }
                }

                // Editor content
                div { class: "editor-content-wrapper",
                    div {
                        id: "{editor_id}",
                        class: "editor-content",
                        contenteditable: "true",

                        onkeydown: {
                        let mut doc = document.clone();
                        move |evt| {
                            use dioxus::prelude::keyboard_types::Key;
                            use std::time::Duration;

                            let plat = platform::platform();
                            let mods = evt.modifiers();
                            let has_modifier = mods.ctrl() || mods.meta() || mods.alt();

                            // During IME composition:
                            // - Allow modifier shortcuts (Ctrl+B, Ctrl+Z, etc.)
                            // - Allow Escape to cancel composition
                            // - Block text input (let browser handle composition preview)
                            if doc.composition.read().is_some() {
                                if evt.key() == Key::Escape {
                                    tracing::debug!("Escape pressed - cancelling composition");
                                    doc.composition.set(None);
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
                                if let Some(ended_at) = *doc.composition_ended_at.read() {
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
                                handle_keydown(evt, &mut doc);
                            }
                        }
                    },

                    onkeyup: {
                        let mut doc = document.clone();
                        move |evt| {
                            use dioxus::prelude::keyboard_types::Key;

                            // Arrow keys with direction hint for snapping
                            let direction_hint = match evt.key() {
                                Key::ArrowLeft | Key::ArrowUp => Some(SnapDirection::Backward),
                                Key::ArrowRight | Key::ArrowDown => Some(SnapDirection::Forward),
                                _ => None,
                            };

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
                                if let Some(dir) = direction_hint {
                                    sync_cursor_from_dom_with_direction(&mut doc, editor_id, &paras, Some(dir));
                                } else {
                                    sync_cursor_from_dom(&mut doc, editor_id, &paras);
                                }
                                let spans = syntax_spans();
                                let cursor_offset = doc.cursor.read().offset;
                                let selection = *doc.selection.read();
                                update_syntax_visibility(
                                    cursor_offset,
                                    selection.as_ref(),
                                    &spans,
                                    &paras,
                                );
                            }
                        }
                    },

                    onselect: {
                        let mut doc = document.clone();
                        move |_evt| {
                            tracing::debug!("onselect fired");
                            let paras = cached_paragraphs();
                            sync_cursor_from_dom(&mut doc, editor_id, &paras);
                            let spans = syntax_spans();
                            let cursor_offset = doc.cursor.read().offset;
                            let selection = *doc.selection.read();
                            update_syntax_visibility(
                                cursor_offset,
                                selection.as_ref(),
                                &spans,
                                &paras,
                            );
                        }
                    },

                    onselectstart: {
                        let mut doc = document.clone();
                        move |_evt| {
                            tracing::debug!("onselectstart fired");
                            let paras = cached_paragraphs();
                            sync_cursor_from_dom(&mut doc, editor_id, &paras);
                            let spans = syntax_spans();
                            let cursor_offset = doc.cursor.read().offset;
                            let selection = *doc.selection.read();
                            update_syntax_visibility(
                                cursor_offset,
                                selection.as_ref(),
                                &spans,
                                &paras,
                            );
                        }
                    },

                    onselectionchange: {
                        let mut doc = document.clone();
                        move |_evt| {
                            tracing::debug!("onselectionchange fired");
                            let paras = cached_paragraphs();
                            sync_cursor_from_dom(&mut doc, editor_id, &paras);
                            let spans = syntax_spans();
                            let cursor_offset = doc.cursor.read().offset;
                            let selection = *doc.selection.read();
                            update_syntax_visibility(
                                cursor_offset,
                                selection.as_ref(),
                                &spans,
                                &paras,
                            );
                        }
                    },

                    onclick: {
                        let mut doc = document.clone();
                        move |_evt| {
                            tracing::debug!("onclick fired");
                            let paras = cached_paragraphs();
                            sync_cursor_from_dom(&mut doc, editor_id, &paras);
                            let spans = syntax_spans();
                            let cursor_offset = doc.cursor.read().offset;
                            let selection = *doc.selection.read();
                            update_syntax_visibility(
                                cursor_offset,
                                selection.as_ref(),
                                &spans,
                                &paras,
                            );
                        }
                    },

                    // Android workaround: Handle Enter in keypress instead of keydown.
                    // Chrome Android fires confused composition events on Enter in keydown,
                    // but keypress fires after composition state settles.
                    onkeypress: {
                        let mut doc = document.clone();
                        move |evt| {
                            use dioxus::prelude::keyboard_types::Key;

                            let plat = platform::platform();
                            if plat.android && evt.key() == Key::Enter {
                                tracing::debug!("Android: handling Enter in keypress");
                                evt.prevent_default();
                                handle_keydown(evt, &mut doc);
                            }
                        }
                    },

                    onpaste: {
                        let mut doc = document.clone();
                        move |evt| {
                            handle_paste(evt, &mut doc);
                        }
                    },

                    oncut: {
                        let mut doc = document.clone();
                        move |evt| {
                            handle_cut(evt, &mut doc);
                        }
                    },

                    oncopy: {
                        let doc = document.clone();
                        move |evt| {
                            handle_copy(evt, &doc);
                        }
                    },

                    onblur: {
                        let mut doc = document.clone();
                        move |_| {
                            // Cancel any in-progress IME composition on focus loss
                            let had_composition = doc.composition.read().is_some();
                            if had_composition {
                                tracing::debug!("onblur: clearing active composition");
                            }
                            doc.composition.set(None);
                        }
                    },

                    oncompositionstart: {
                        let mut doc = document.clone();
                        move |evt: CompositionEvent| {
                            let data = evt.data().data();
                            tracing::debug!(
                                data = %data,
                                "compositionstart"
                            );
                            // Delete selection if present (composition replaces it)
                            let sel = doc.selection.write().take();
                            if let Some(sel) = sel {
                                let (start, end) =
                                    (sel.anchor.min(sel.head), sel.anchor.max(sel.head));
                                tracing::debug!(
                                    start,
                                    end,
                                    "compositionstart: deleting selection"
                                );
                                let _ = doc.remove_tracked(start, end.saturating_sub(start));
                                doc.cursor.write().offset = start;
                            }

                            let cursor_offset = doc.cursor.read().offset;
                            tracing::debug!(
                                cursor = cursor_offset,
                                "compositionstart: setting composition state"
                            );
                            doc.composition.set(Some(CompositionState {
                                start_offset: cursor_offset,
                                text: data,
                            }));
                        }
                    },

                    oncompositionupdate: {
                        let mut doc = document.clone();
                        move |evt: CompositionEvent| {
                            let data = evt.data().data();
                            tracing::debug!(
                                data = %data,
                                "compositionupdate"
                            );
                            let mut comp_guard = doc.composition.write();
                            if let Some(ref mut comp) = *comp_guard {
                                comp.text = data;
                            } else {
                                tracing::debug!("compositionupdate without active composition state");
                            }
                        }
                    },

                    oncompositionend: {
                        let mut doc = document.clone();
                        move |evt: CompositionEvent| {
                            let final_text = evt.data().data();
                            tracing::debug!(
                                data = %final_text,
                                "compositionend"
                            );
                            // Record when composition ended for Safari timing workaround
                            doc.composition_ended_at.set(Some(web_time::Instant::now()));

                            let comp = doc.composition.write().take();
                            if let Some(comp) = comp {
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

                                    let cursor_offset = doc.cursor.read().offset;
                                    let zw_count = cursor_offset - delete_start;
                                    if zw_count > 0 {
                                        // Splice: delete zero-width chars and insert new char in one op
                                        let _ = doc.replace_tracked(delete_start, zw_count, &final_text);
                                        doc.cursor.write().offset = delete_start + final_text.chars().count();
                                    } else if cursor_offset == doc.len_chars() {
                                        // Fast path: append at end
                                        let _ = doc.push_tracked(&final_text);
                                        doc.cursor.write().offset = comp.start_offset + final_text.chars().count();
                                    } else {
                                        let _ = doc.insert_tracked(cursor_offset, &final_text);
                                        doc.cursor.write().offset = comp.start_offset + final_text.chars().count();
                                    }
                                }
                            } else {
                                tracing::debug!("compositionend without active composition state");
                            }
                        }
                    },
                    }

                    // Debug panel snug below editor
                    div { class: "editor-debug",
                        div { "Cursor: {document.cursor.read().offset}, Chars: {document.len_chars()}" },
                        ReportButton {
                            email: "editor-bugs@weaver.sh".to_string(),
                            editor_id: "markdown-editor".to_string(),
                        }
                    }
                }

            // Toolbar in grid column 2, row 3
            EditorToolbar {
                on_format: {
                    let mut doc = document.clone();
                    move |action| {
                        formatting::apply_formatting(&mut doc, action);
                    }
                },
                on_image: {
                    let mut doc = document.clone();
                    move |uploaded: super::image_upload::UploadedImage| {
                        // Build data URL for immediate preview
                        use base64::{Engine, engine::general_purpose::STANDARD};
                        let data_url = format!(
                            "data:{};base64,{}",
                            uploaded.mime_type,
                            STANDARD.encode(&uploaded.data)
                        );

                        // Add to resolver for immediate display
                        let name = uploaded.name.clone();
                        image_resolver.with_mut(|resolver| {
                            resolver.add_pending(name.clone(), data_url);
                        });

                        // Insert markdown image syntax at cursor
                        let alt_text = if uploaded.alt.is_empty() {
                            name.clone()
                        } else {
                            uploaded.alt.clone()
                        };
                        let markdown = format!("![{}](/image/{})", alt_text, name);

                        let pos = doc.cursor.read().offset;
                        let _ = doc.insert_tracked(pos, &markdown);
                        doc.cursor.write().offset = pos + markdown.chars().count();

                        // Upload to PDS in background if authenticated
                        let is_authenticated = auth_state.read().is_authenticated();
                        if is_authenticated {
                            let fetcher = fetcher.clone();
                            let name_for_upload = name.clone();
                            let alt_for_upload = alt_text.clone();
                            let data = uploaded.data.clone();
                            let mut doc_for_spawn = doc.clone();

                            spawn(async move {
                                let client = fetcher.get_client();

                                // Upload blob and create temporary PublishedBlob record
                                match client.publish_blob(data, &name_for_upload, None).await {
                                    Ok((strong_ref, published_blob)) => {
                                        // Get DID from fetcher
                                        let did = match fetcher.current_did().await {
                                            Some(d) => d,
                                            None => {
                                                tracing::warn!("No DID available");
                                                return;
                                            }
                                        };

                                        // Extract rkey from the AT-URI
                                        let blob_rkey = match strong_ref.uri.rkey() {
                                            Some(rkey) => rkey.0.clone().into_static(),
                                            None => {
                                                tracing::warn!("No rkey in PublishedBlob URI");
                                                return;
                                            }
                                        };

                                        // Build Image using the builder API
                                        let name_for_resolver = name_for_upload.clone();
                                        let image = Image::new()
                                            .alt(alt_for_upload.to_cowstr())
                                            .image(published_blob.upload)
                                            .name(name_for_upload.to_cowstr())
                                            .build();

                                        // Add to document
                                        doc_for_spawn.add_image(&image, Some(&strong_ref.uri));

                                        // Promote from pending to uploaded in resolver
                                        let ident = AtIdentifier::Did(did);
                                        image_resolver.with_mut(|resolver| {
                                            resolver.promote_to_uploaded(
                                                &name_for_resolver,
                                                blob_rkey,
                                                ident,
                                            );
                                        });

                                        tracing::info!(name = %name_for_resolver, "Image uploaded to PDS");
                                    }
                                    Err(e) => {
                                        tracing::error!(error = %e, "Failed to upload image");
                                        // Image stays as data URL - will work for preview but not publish
                                    }
                                }
                            });
                        } else {
                            tracing::info!(name = %name, "Image added with data URL (not authenticated)");
                        }
                    }
                },
            }

        }
    }
}
