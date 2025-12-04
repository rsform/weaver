//! The main MarkdownEditor component.

use dioxus::prelude::*;
use jacquard::IntoStatic;
use jacquard::cowstr::ToCowStr;
use jacquard::identity::resolver::IdentityResolver;
use jacquard::smol_str::SmolStr;
use jacquard::types::blob::BlobRef;
use jacquard::types::ident::AtIdentifier;
use weaver_api::sh_weaver::embed::images::Image;
use weaver_common::WeaverExt;

use crate::auth::AuthState;
use crate::components::editor::ReportButton;
use crate::fetch::Fetcher;

use super::actions::{
    EditorAction, Key, KeyCombo, KeybindingConfig, KeydownResult, Range, execute_action,
    handle_keydown_with_bindings,
};
use super::beforeinput::{BeforeInputContext, BeforeInputResult, InputType, handle_beforeinput};
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
use super::beforeinput::{get_data_from_event, get_target_range_from_event};
use super::document::{CompositionState, EditorDocument, LoadedDocState};
use super::dom_sync::{
    sync_cursor_from_dom, sync_cursor_from_dom_with_direction, update_paragraph_dom,
};
use super::formatting;
use super::input::{get_char_at, handle_copy, handle_cut, handle_paste};
use super::offset_map::SnapDirection;
use super::paragraph::ParagraphRender;
use super::platform;
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
/// - `target_notebook`: Optional notebook title to add the entry to when publishing
/// - `entry_index`: Optional index of entries for wikilink validation
#[component]
pub fn MarkdownEditor(
    initial_content: Option<String>,
    entry_uri: Option<String>,
    target_notebook: Option<SmolStr>,
    entry_index: Option<weaver_common::EntryIndex>,
) -> Element {
    let fetcher = use_context::<Fetcher>();

    let draft_key = use_hook(|| {
        entry_uri.clone().unwrap_or_else(|| {
            format!(
                "new:{}",
                jacquard::types::tid::Ticker::new().next(None).as_str()
            )
        })
    });

    let parsed_uri = entry_uri.as_ref().and_then(|s| {
        jacquard::types::string::AtUri::new(s)
            .ok()
            .map(|u| u.into_static())
    });
    let draft_key_for_render = draft_key.clone();
    let target_notebook_for_render = target_notebook.clone();

    let load_resource = use_resource(move || {
        let fetcher = fetcher.clone();
        let draft_key = draft_key.clone();
        let entry_uri = parsed_uri.clone();
        let initial_content = initial_content.clone();

        async move {
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
                                    "You can only edit your own entries".to_string(),
                                );
                            }
                        }
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

                                // Restore existing embeds from the entry
                                if let Some(ref embeds) = loaded.entry.embeds {
                                    let embeds_map = doc.get_map("embeds");

                                    // Restore images
                                    if let Some(ref images) = embeds.images {
                                        let images_list = embeds_map
                                            .get_or_create_container("images", loro::LoroList::new())
                                            .expect("images list");
                                        for image in &images.images {
                                            // Serialize image to JSON and add to list
                                            // No publishedBlobUri since these are already published
                                            let json = serde_json::to_value(image)
                                                .expect("Image serializes");
                                            images_list.push(json).ok();
                                        }
                                    }

                                    // Restore record embeds
                                    if let Some(ref records) = embeds.records {
                                        let records_list = embeds_map
                                            .get_or_create_container("records", loro::LoroList::new())
                                            .expect("records list");
                                        for record in &records.records {
                                            let json = serde_json::to_value(record)
                                                .expect("RecordEmbed serializes");
                                            records_list.push(json).ok();
                                        }
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

    match &*load_resource.read() {
        Some(LoadResult::Loaded(state)) => {
            rsx! {
                MarkdownEditorInner {
                    key: "{draft_key_for_render}",
                    draft_key: draft_key_for_render.clone(),
                    loaded_state: state.clone(),
                    target_notebook: target_notebook_for_render.clone(),
                    entry_index: entry_index.clone(),
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
    target_notebook: Option<SmolStr>,
    /// Optional entry index for wikilink validation in the editor
    entry_index: Option<weaver_common::EntryIndex>,
) -> Element {
    // Context for authenticated API calls
    let fetcher = use_context::<Fetcher>();
    let auth_state = use_context::<Signal<AuthState>>();

    let mut document = use_hook(|| {
        let mut doc = EditorDocument::from_loaded_state(loaded_state.clone());

        // Seed collected_refs with existing record embeds so they get fetched/rendered
        let record_embeds = doc.record_embeds();
        if !record_embeds.is_empty() {
            let refs: Vec<weaver_common::ExtractedRef> = record_embeds
                .into_iter()
                .filter_map(|embed| {
                    embed.name.map(|name| weaver_common::ExtractedRef::AtEmbed {
                        uri: name.to_string(),
                        alt_text: None,
                    })
                })
                .collect();
            doc.set_collected_refs(refs);
        }

        storage::save_to_storage(&doc, &draft_key).ok();
        doc
    });
    let editor_id = "markdown-editor";
    let mut render_cache = use_signal(|| render::RenderCache::default());

    // Populate resolver from existing images if editing a published entry
    let mut image_resolver: Signal<EditorImageResolver> = use_signal(|| {
        let images = document.images();
        if let (false, Some(ref r)) = (images.is_empty(), document.entry_ref()) {
            let ident = r.uri.authority().clone().into_static();
            let entry_rkey = r.uri.rkey().map(|rk| rk.0.clone().into_static());
            EditorImageResolver::from_images(&images, ident, entry_rkey)
        } else {
            EditorImageResolver::default()
        }
    });
    let resolved_content = use_signal(weaver_common::ResolvedContent::default);

    let doc_for_memo = document.clone();
    let doc_for_refs = document.clone();
    let paragraphs = use_memo(move || {
        let edit = doc_for_memo.last_edit();
        let cache = render_cache.peek();
        let resolver = image_resolver();
        let resolved = resolved_content();

        let (paras, new_cache, refs) = render::render_paragraphs_incremental(
            doc_for_memo.loro_text(),
            Some(&cache),
            edit.as_ref(),
            Some(&resolver),
            entry_index.as_ref(),
            &resolved,
        );
        let mut doc_for_spawn = doc_for_refs.clone();
        dioxus::prelude::spawn(async move {
            render_cache.set(new_cache);
            doc_for_spawn.set_collected_refs(refs);
        });

        paras
    });

    // Background fetch for AT embeds
    let mut resolved_content_for_fetch = resolved_content.clone();
    let doc_for_embeds = document.clone();
    let fetcher_for_embeds = fetcher.clone();
    use_effect(move || {
        let refs = doc_for_embeds.collected_refs.read();
        let current_resolved = resolved_content_for_fetch.peek();
        let fetcher = fetcher_for_embeds.clone();

        // Find AT embeds that need fetching
        let to_fetch: Vec<String> = refs
            .iter()
            .filter_map(|r| match r {
                weaver_common::ExtractedRef::AtEmbed { uri, .. } => {
                    // Skip if already resolved
                    if let Ok(at_uri) = jacquard::types::string::AtUri::new(uri) {
                        if current_resolved.get_embed_content(&at_uri).is_none() {
                            return Some(uri.clone());
                        }
                    }
                    None
                }
                _ => None,
            })
            .collect();

        if to_fetch.is_empty() {
            return;
        }

        // Spawn background fetches
        dioxus::prelude::spawn(async move {
            for uri_str in to_fetch {
                let Ok(at_uri) = jacquard::types::string::AtUri::new(&uri_str) else {
                    continue;
                };

                match weaver_renderer::atproto::fetch_and_render(&at_uri, &fetcher)
                    .await
                {
                    Ok(html) => {
                        resolved_content_for_fetch.with_mut(|rc| {
                            rc.add_embed(at_uri.into_static(), html, None);
                        });
                    }
                    Err(e) => {
                        tracing::warn!("failed to fetch embed {}: {}", uri_str, e);
                    }
                }
            }
        });
    });

    let mut new_tag = use_signal(String::new);

    let offset_map = use_memo(move || {
        paragraphs()
            .iter()
            .flat_map(|p| p.offset_map.iter().cloned())
            .collect::<Vec<_>>()
    });
    let syntax_spans = use_memo(move || {
        paragraphs()
            .iter()
            .flat_map(|p| p.syntax_spans.iter().cloned())
            .collect::<Vec<_>>()
    });
    let mut cached_paragraphs = use_signal(|| Vec::<ParagraphRender>::new());

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

    // Store interval handle so it's dropped when component unmounts (prevents panic)
    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    let mut interval_holder: Signal<Option<gloo_timers::callback::Interval>> = use_signal(|| None);

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
            };

            if needs_save {
                doc.sync_loro_cursor();
                let _ = storage::save_to_storage(&doc, &draft_key);

                // Update last saved frontiers
                last_saved_frontiers.set(Some(current_frontiers));
            }
        });
        // Store in signal instead of forget - interval drops when component unmounts
        interval_holder.set(Some(interval));
    });

    // Set up beforeinput listener for all text input handling.
    // This is the primary handler for text insertion, deletion, etc.
    // Keydown only handles shortcuts now.
    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    let doc_for_beforeinput = document.clone();
    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    use_effect(move || {
        use wasm_bindgen::JsCast;
        use wasm_bindgen::prelude::*;

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
            let input_type_str = evt.input_type();
            tracing::debug!(input_type = %input_type_str, "beforeinput");

            let plat = platform::platform();
            let input_type = InputType::from_str(&input_type_str);
            let is_composing = evt.is_composing();

            // Get target range from the event if available
            let paras = cached_paras.peek().clone();
            let target_range = get_target_range_from_event(&evt, editor_id, &paras);
            let data = get_data_from_event(&evt);
            let ctx = BeforeInputContext {
                input_type: input_type.clone(),
                data,
                target_range,
                is_composing,
                platform: &plat,
            };

            let result = handle_beforeinput(&mut doc, ctx);

            match result {
                BeforeInputResult::Handled => {
                    evt.prevent_default();
                }
                BeforeInputResult::PassThrough => {
                    // Let browser handle (e.g., during composition)
                }
                BeforeInputResult::HandledAsync => {
                    evt.prevent_default();
                    // Async follow-up will happen elsewhere
                }
                BeforeInputResult::DeferredCheck { fallback_action } => {
                    // Android backspace workaround: let browser try first,
                    // check in 50ms if anything happened, if not execute fallback
                    let mut doc_for_timeout = doc.clone();
                    let doc_len_before = doc.len_chars();

                    let window = web_sys::window();
                    if let Some(window) = window {
                        let closure = Closure::once(move || {
                            // Check if the document changed
                            if doc_for_timeout.len_chars() == doc_len_before {
                                // Nothing happened - execute fallback
                                tracing::debug!("Android backspace fallback triggered");
                                // Refocus to work around virtual keyboard issues
                                if let Some(window) = web_sys::window() {
                                    if let Some(doc) = window.document() {
                                        if let Some(elem) = doc.get_element_by_id(editor_id) {
                                            if let Some(html_elem) =
                                                elem.dyn_ref::<web_sys::HtmlElement>()
                                            {
                                                let _ = html_elem.blur();
                                                let _ = html_elem.focus();
                                            }
                                        }
                                    }
                                }
                                execute_action(&mut doc_for_timeout, &fallback_action);
                            }
                        });
                        let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(
                            closure.as_ref().unchecked_ref(),
                            50,
                        );
                        closure.forget();
                    }
                }
            }

            // Android workaround: When swipe keyboard picks a suggestion,
            // DOM mutations fire before selection moves. Defer cursor sync.
            if plat.android && matches!(input_type, InputType::InsertText) {
                if let Some(data) = evt.data() {
                    if data.contains(' ') || data.len() > 3 {
                        tracing::debug!("Android: possible suggestion pick, deferring cursor sync");
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
                            target_notebook: target_notebook.as_ref().map(|s| s.to_string()),
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
                        let keybindings = KeybindingConfig::default_for_platform(&platform::platform());
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

                            // Try keybindings first (for shortcuts like Ctrl+B, Ctrl+Z, etc.)
                            let combo = KeyCombo::from_keyboard_event(&evt.data());
                            let cursor_offset = doc.cursor.read().offset;
                            let selection = *doc.selection.read();
                            let range = selection
                                .map(|s| Range::new(s.anchor.min(s.head), s.anchor.max(s.head)))
                                .unwrap_or_else(|| Range::caret(cursor_offset));
                            match handle_keydown_with_bindings(&mut doc, &keybindings, combo, range) {
                                KeydownResult::Handled => {
                                    evt.prevent_default();
                                    return;
                                }
                                KeydownResult::PassThrough => {
                                    // Navigation keys - let browser handle, sync in keyup
                                    return;
                                }
                                KeydownResult::NotHandled => {
                                    // Text input - let beforeinput handle it
                                }
                            }

                            // Text input keys: let beforeinput handle them
                            // We don't prevent default here - beforeinput will do that
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
                        move |evt| {
                            tracing::debug!("onclick fired");
                            let paras = cached_paragraphs();

                            // Check if click target is a math-clickable element
                            #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
                            {
                                use dioxus::web::WebEventExt;
                                use wasm_bindgen::JsCast;

                                let web_evt = evt.as_web_event();
                                if let Some(target) = web_evt.target() {
                                    if let Some(element) = target.dyn_ref::<web_sys::Element>() {
                                        // Check element or ancestors for math-clickable
                                        if let Ok(Some(math_el)) = element.closest(".math-clickable") {
                                            if let Some(char_target) = math_el.get_attribute("data-char-target") {
                                                if let Ok(offset) = char_target.parse::<usize>() {
                                                    tracing::debug!("math-clickable clicked, moving cursor to {}", offset);
                                                    doc.cursor.write().offset = offset;
                                                    *doc.selection.write() = None;
                                                    // Update visibility FIRST so math-source is visible
                                                    let spans = syntax_spans();
                                                    update_syntax_visibility(offset, None, &spans, &paras);
                                                    // Then set DOM selection
                                                    let map = offset_map();
                                                    let _ = crate::components::editor::cursor::restore_cursor_position(
                                                        offset,
                                                        &map,
                                                        editor_id,
                                                        None,
                                                    );
                                                    return;
                                                }
                                            }
                                        }
                                    }
                                }
                            }

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

                                // Get current range
                                let range = if let Some(sel) = *doc.selection.read() {
                                    Range::new(sel.anchor.min(sel.head), sel.anchor.max(sel.head))
                                } else {
                                    Range::caret(doc.cursor.read().offset)
                                };

                                let action = EditorAction::InsertParagraph { range };
                                execute_action(&mut doc, &action);
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
                    div { class: "editor-debug",
                        div { "Cursor: {document.cursor.read().offset}, Chars: {document.len_chars()}" },
                        ReportButton {
                            email: "editor-bugs@weaver.sh".to_string(),
                            editor_id: "markdown-editor".to_string(),
                        }
                    }
                }

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

                        // Check if authenticated and get DID for draft path
                        let auth = auth_state.read();
                        let did_for_path = auth.did.clone();
                        let is_authenticated = auth.is_authenticated();
                        drop(auth);

                        // Pre-generate TID for the blob rkey (used in draft path and upload)
                        let blob_tid = jacquard::types::tid::Ticker::new().next(None);

                        // Build markdown with proper draft path if authenticated
                        let markdown = if let Some(ref did) = did_for_path {
                            format!("![{}](/image/{}/draft/{}/{})", alt_text, did, blob_tid.as_str(), name)
                        } else {
                            // Fallback for unauthenticated - simple path (won't be publishable anyway)
                            format!("![{}](/image/{})", alt_text, name)
                        };

                        let pos = doc.cursor.read().offset;
                        let _ = doc.insert_tracked(pos, &markdown);
                        doc.cursor.write().offset = pos + markdown.chars().count();

                        // Upload to PDS in background if authenticated
                        if is_authenticated {
                            let fetcher = fetcher.clone();
                            let name_for_upload = name.clone();
                            let alt_for_upload = alt_text.clone();
                            let data = uploaded.data.clone();
                            let mut doc_for_spawn = doc.clone();

                            spawn(async move {
                                let client = fetcher.get_client();

                                // Clone data for cache pre-warming
                                let data_for_cache = data.clone();

                                // Use pre-generated TID as rkey for the blob record
                                let rkey = jacquard::types::recordkey::RecordKey::any(blob_tid.as_str())
                                    .expect("TID is valid record key");

                                // Upload blob and create temporary PublishedBlob record
                                match client.publish_blob(data, &name_for_upload, Some(rkey)).await {
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

                                        let cid = published_blob.upload.blob().cid().clone().into_static();

                                        let name_for_resolver = name_for_upload.clone();
                                        let image = Image::new()
                                            .alt(alt_for_upload.to_cowstr())
                                            .image(published_blob.upload)
                                            .name(name_for_upload.to_cowstr())
                                            .build();
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

                                        // Pre-warm server cache with blob bytes
                                        #[cfg(feature = "fullstack-server")]
                                        {
                                            use jacquard::smol_str::ToSmolStr;
                                            if let Err(e) = crate::data::cache_blob_bytes(
                                                cid.to_smolstr(),
                                                Some(name_for_resolver.into()),
                                                None,
                                                data_for_cache.into(),
                                            ).await {
                                                tracing::warn!(error = %e, "Failed to pre-warm blob cache");
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        tracing::error!(error = %e, "Failed to upload image");
                                        // Image stays as data URL - will work for preview but not publish
                                    }
                                }
                            });
                        } else {
                            tracing::debug!(name = %name, "Image added with data URL (not authenticated)");
                        }
                    }
                },
            }

        }
    }
}
