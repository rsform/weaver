//! The main MarkdownEditor component.

use super::actions::{
    EditorAction, KeydownResult, Range, execute_action, handle_keydown_with_bindings,
};
use super::document::SignalEditorDocument;
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
use super::dom_sync::update_paragraph_dom;
use super::publish::PublishButton;
use super::remote_cursors::RemoteCursors;
use super::storage;
use super::sync::{LoadEditorResult, SyncStatus, load_editor_state};
use super::toolbar::EditorToolbar;
use crate::auth::AuthState;
use crate::components::collab::CollaboratorAvatars;
use crate::components::editor::collab::CollabCoordinator;
use crate::components::editor::{LoadedDocState, ReportButton};
use crate::fetch::Fetcher;
use dioxus::prelude::*;
use jacquard::IntoStatic;
use jacquard::smol_str::SmolStr;
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
use jacquard::types::blob::BlobRef;
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
use weaver_editor_browser::{BeforeInputContext, BeforeInputResult, update_syntax_visibility};
use weaver_editor_browser::{
    handle_compositionend, handle_compositionstart, handle_compositionupdate, handle_copy,
    handle_cut, handle_paste, platform, sync_cursor_and_visibility,
};
use weaver_editor_core::EditorImageResolver;
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
use weaver_editor_core::InputType;
use weaver_editor_core::ParagraphRender;
use weaver_editor_core::SnapDirection;
use weaver_editor_core::apply_formatting;

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
        let target_notebook = target_notebook.clone();

        async move {
            load_editor_state(
                &fetcher,
                &draft_key,
                entry_uri.as_ref(),
                initial_content.as_deref(),
                target_notebook.as_deref(),
            )
            .await
        }
    });

    match &*load_resource.read() {
        Some(LoadEditorResult::Loaded(state)) => {
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
        Some(LoadEditorResult::Failed(err)) => {
            rsx! {
                div { class: "editor-error",
                    "Failed to load: {err}"
                }
            }
        }
        None => {
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

    #[allow(unused_mut)]
    let mut document = use_hook(|| {
        let mut doc = SignalEditorDocument::from_loaded_state(loaded_state.clone());

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
    let mut render_cache = use_signal(|| weaver_editor_browser::RenderCache::default());

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
    // Use pre-resolved content from loaded state (avoids embed pop-in)
    let resolved_content = use_signal(|| loaded_state.resolved_content.clone());

    // Presence snapshot for remote collaborators (updated by collab coordinator)
    let presence = use_signal(weaver_common::transport::PresenceSnapshot::default);

    // Resource URI for real-time collab (entry URI if editing published entry)
    let collab_resource_uri = document.entry_ref().map(|r| r.uri.to_string());

    let doc_for_memo = document.clone();
    let doc_for_refs = document.clone();
    let entry_index_for_memo = entry_index.clone();
    #[allow(unused_mut)]
    let mut paragraphs = use_memo(move || {
        // Read content_changed to establish reactive dependency
        let _ = doc_for_memo.content_changed.read();
        let edit = doc_for_memo.last_edit();
        let cache = render_cache.peek();
        let resolver = image_resolver();
        let resolved = resolved_content();

        tracing::trace!(
            "Rendering with {} pre-resolved embeds",
            resolved.embed_content.len()
        );

        let cursor_offset = doc_for_memo.cursor.read().offset;
        let result = weaver_editor_core::render_paragraphs_incremental(
            doc_for_memo.buffer(),
            Some(&cache),
            cursor_offset,
            edit.as_ref(),
            Some(&resolver),
            entry_index_for_memo.as_ref(),
            &resolved,
        );
        let paras = result.paragraphs;
        let new_cache = result.cache;
        let refs = result.collected_refs;
        let mut doc_for_spawn = doc_for_refs.clone();
        dioxus::prelude::spawn(async move {
            render_cache.set(new_cache);
            doc_for_spawn.set_collected_refs(refs);
        });

        paras
    });

    // Background fetch for AT embeds via worker
    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    {
        use dioxus::prelude::Writable;
        use weaver_embed_worker::{EmbedWorkerHost, EmbedWorkerOutput};

        let resolved_content_for_fetch = resolved_content;
        let mut embed_host: Signal<Option<EmbedWorkerHost>> = use_signal(|| None);

        // Spawn embed worker on mount
        let doc_for_embeds = document.clone();
        use_effect(move || {
            // Callback for worker responses - uses write_unchecked since we're in a Fn closure
            let on_output = move |output: EmbedWorkerOutput| match output {
                EmbedWorkerOutput::Embeds {
                    results,
                    errors,
                    fetch_ms,
                } => {
                    if !results.is_empty() {
                        let mut rc = resolved_content_for_fetch.write_unchecked();
                        for (uri_str, html) in results {
                            if let Ok(at_uri) = jacquard::types::string::AtUri::new_owned(uri_str) {
                                rc.add_embed(at_uri, html, None);
                            }
                        }
                        tracing::debug!(
                            count = rc.embed_content.len(),
                            fetch_ms,
                            "embed worker fetched embeds"
                        );
                    }
                    for (uri, err) in errors {
                        tracing::warn!("embed worker failed to fetch {}: {}", uri, err);
                    }
                }
                EmbedWorkerOutput::CacheCleared => {
                    tracing::debug!("embed worker cache cleared");
                }
            };

            let host = EmbedWorkerHost::spawn("/embed_worker.js", on_output);
            embed_host.set(Some(host));
            tracing::info!("Embed worker spawned");
        });

        // Send embeds to worker when collected_refs changes
        use_effect(move || {
            let refs = doc_for_embeds.collected_refs.read();
            let current_resolved = resolved_content_for_fetch.peek();

            // Find AT embeds that need fetching
            let to_fetch: Vec<String> = refs
                .iter()
                .filter_map(|r| match r {
                    weaver_common::ExtractedRef::AtEmbed { uri, .. } => {
                        // Skip if already resolved
                        if let Ok(at_uri) = jacquard::types::string::AtUri::new_owned(uri) {
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

            // Send to worker
            if let Some(ref host) = *embed_host.peek() {
                host.fetch_embeds(to_fetch);
            }
        });
    }

    let mut new_tag = use_signal(String::new);

    #[allow(unused)]
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
    #[allow(unused_mut)]
    let mut cached_paragraphs = use_signal(|| Vec::<ParagraphRender>::new());

    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    let mut doc_for_dom = document.clone();
    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    use_effect(move || {
        // Skip DOM updates during IME composition - browser controls the preview
        if doc_for_dom.composition.read().is_some() {
            tracing::debug!("skipping DOM update during composition");
            return;
        }

        tracing::trace!(
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

        let cursor_para_updated =
            update_paragraph_dom(editor_id, &prev, &new_paras, cursor_offset, false);

        // Store for next comparison AND for event handlers (write-only, no reactive read)
        cached_paragraphs.set(new_paras.clone());

        // Update syntax visibility after DOM changes
        update_syntax_visibility(cursor_offset, selection.as_ref(), &spans, &new_paras);
    });

    // Track last saved frontiers to detect changes (peek-only, no subscriptions)
    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    let mut last_saved_frontiers: Signal<Option<loro::Frontiers>> = use_signal(|| None);

    // Store interval handle so it's dropped when component unmounts (prevents panic)
    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    let mut interval_holder: Signal<Option<gloo_timers::callback::Interval>> = use_signal(|| None);

    // Autosave interval - saves to localStorage when document changes
    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    {
        let doc_for_autosave = document.clone();
        let draft_key_for_autosave = draft_key.clone();
        use_effect(move || {
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

                if !needs_save {
                    return;
                }

                doc.sync_loro_cursor();
                let _ = storage::save_to_storage(&doc, &draft_key);
                last_saved_frontiers.set(Some(current_frontiers));
            });

            interval_holder.set(Some(interval));
        });
    }

    // Set up beforeinput listener for all text input handling.
    // This is the primary handler for text insertion, deletion, etc.
    // Keydown only handles shortcuts now.
    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    type BeforeInputClosure = wasm_bindgen::closure::Closure<dyn FnMut(web_sys::InputEvent)>;
    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    let mut beforeinput_closure: Signal<Option<BeforeInputClosure>> = use_signal(|| None);

    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    let doc_for_beforeinput = document.clone();
    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    use_effect(move || {
        use gloo_timers::callback::Timeout;
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

        let closure: BeforeInputClosure = Closure::wrap(Box::new(move |evt: web_sys::InputEvent| {
            let input_type_str = evt.input_type();
            tracing::debug!(input_type = %input_type_str, "beforeinput");

            let plat = platform::platform();
            let input_type = weaver_editor_browser::parse_browser_input_type(&input_type_str);
            let is_composing = evt.is_composing();

            // Get target range from the event if available
            let paras = cached_paras.peek().clone();
            let target_range =
                weaver_editor_browser::get_target_range_from_event(&evt, editor_id, &paras);
            let data = weaver_editor_browser::get_data_from_event(&evt);
            let ctx = BeforeInputContext {
                input_type: input_type.clone(),
                data,
                target_range,
                is_composing,
                platform: &plat,
            };

            let current_range = weaver_editor_browser::get_current_range(&doc);
            let result = weaver_editor_browser::handle_beforeinput(&mut doc, &ctx, current_range);

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

                    Timeout::new(50, move || {
                        if doc_for_timeout.len_chars() == doc_len_before {
                            tracing::debug!("Android backspace fallback triggered");
                            // Refocus to work around virtual keyboard issues
                            if let Some(window) = web_sys::window() {
                                if let Some(dom_doc) = window.document() {
                                    if let Some(elem) = dom_doc.get_element_by_id(editor_id) {
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
                    })
                    .forget(); // One-shot timer, runs and cleans up
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

                        Timeout::new(20, move || {
                            let paras = paras();
                            weaver_editor_browser::sync_cursor_from_dom(
                                &mut doc_for_timeout,
                                editor_id,
                                &paras,
                                None,
                            );
                        })
                        .forget(); // One-shot timer, runs and cleans up
                    }
                }
            }
        })
            as Box<dyn FnMut(web_sys::InputEvent)>);

        let _ = editor
            .add_event_listener_with_callback("beforeinput", closure.as_ref().unchecked_ref());

        // Store closure in signal for proper lifecycle management
        beforeinput_closure.set(Some(closure));
    });

    // Clean up event listener on unmount
    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    use_drop(move || {
        if let Some(closure) = beforeinput_closure.peek().as_ref() {
            if let Some(window) = web_sys::window() {
                if let Some(dom_document) = window.document() {
                    if let Some(editor) = dom_document.get_element_by_id(editor_id) {
                        use wasm_bindgen::JsCast;
                        let _ = editor.remove_event_listener_with_callback(
                            "beforeinput",
                            closure.as_ref().unchecked_ref(),
                        );
                    }
                }
            }
        }
    });

    rsx! {
        Stylesheet { href: asset!("/assets/styling/editor.css") }
        CollabCoordinator {
            document: document.clone(),
            resource_uri: collab_resource_uri.clone().unwrap_or(draft_key.clone()),
            presence,
            div { class: "markdown-editor-container",
                // Title bar
                div { class: "editor-title-bar",
                    input {
                        r#type: "text",
                        class: "title-input",
                        aria_label: "Entry title",
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
                                aria_label: "URL path",
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
                                            aria_label: "Remove tag {tag}",
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
                                    aria_label: "Add tag",
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
                            // Show collaborator avatars when editing an existing entry
                            if let Some(entry_ref) = document.entry_ref() {
                                {
                                    let title = document.title();
                                    rsx! {
                                        CollaboratorAvatars {
                                            resource_uri: entry_ref.uri.clone(),
                                            resource_cid: entry_ref.cid.to_string(),
                                            resource_title: if title.is_empty() { None } else { Some(title) },
                                        }
                                    }
                                }
                            }

                            {
                                // Enable collaborative sync for any published entry (both owners and collaborators)
                                let is_published = document.entry_ref().is_some();

                                // Refresh callback: fetch and merge collaborator changes (incremental)
                                let on_refresh = if is_published {
                                    let fetcher_for_refresh = fetcher.clone();
                                    let doc_for_refresh = document.clone();
                                    let entry_uri = document.entry_ref().map(|r| r.uri.clone().into_static());

                                    Some(EventHandler::new(move |_| {
                                        let fetcher = fetcher_for_refresh.clone();
                                        let mut doc = doc_for_refresh.clone();
                                        let uri = entry_uri.clone();

                                        spawn(async move {
                                            if let Some(uri) = uri {
                                                // Get last seen diffs for incremental sync
                                                let last_seen = doc.last_seen_diffs.read().clone();

                                                match super::sync::load_all_edit_states_from_pds(&fetcher, &uri, &last_seen).await {
                                                    Ok(Some(pds_state)) => {
                                                        if let Err(e) = doc.import_updates(&pds_state.root_snapshot) {
                                                            tracing::error!("Failed to import collaborator updates: {:?}", e);
                                                        } else {
                                                            tracing::info!("Successfully merged collaborator updates");
                                                            // Update the last seen diffs for next incremental sync
                                                            *doc.last_seen_diffs.write() = pds_state.last_seen_diffs;
                                                        }
                                                    }
                                                    Ok(None) => {
                                                        tracing::debug!("No collaborator updates found");
                                                    }
                                                    Err(e) => {
                                                        tracing::error!("Failed to fetch collaborator updates: {}", e);
                                                    }
                                                }
                                            }
                                        });
                                    }))
                                } else {
                                    None
                                };

                                rsx! {
                                    SyncStatus {
                                        document: document.clone(),
                                        draft_key: draft_key.to_string(),
                                        on_refresh,
                                        is_collaborative: is_published,
                                    }
                                }
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
                        // Remote collaborator cursors overlay
                        RemoteCursors { presence, document: document.clone(), render_cache }
                        div {
                            id: "{editor_id}",
                            class: "editor-content",
                            contenteditable: "true",
                            role: "textbox",
                            aria_multiline: "true",
                            aria_label: "Document content",

                            onkeydown: {
                            let mut doc = document.clone();
                            let keybindings = super::actions::default_keybindings(platform::platform());
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
                                let combo = super::actions::keycombo_from_dioxus_event(&evt.data());
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
                                    tracing::debug!(
                                        key = ?evt.key(),
                                        navigation,
                                        select_all,
                                        "onkeyup navigation - syncing cursor from DOM"
                                    );
                                    let paras = cached_paragraphs();
                                    let spans = syntax_spans();
                                    sync_cursor_and_visibility(
                                        &mut doc, editor_id, &paras, &spans, direction_hint,
                                    );
                                }
                            }
                        },

                        onselect: {
                            let mut doc = document.clone();
                            move |_evt| {
                                tracing::debug!("onselect fired - syncing cursor from DOM");
                                let paras = cached_paragraphs();
                                let spans = syntax_spans();
                                sync_cursor_and_visibility(&mut doc, editor_id, &paras, &spans, None);
                            }
                        },

                        onselectstart: {
                            let mut doc = document.clone();
                            move |_evt| {
                                tracing::debug!("onselectstart fired - syncing cursor from DOM");
                                let paras = cached_paragraphs();
                                let spans = syntax_spans();
                                sync_cursor_and_visibility(&mut doc, editor_id, &paras, &spans, None);
                            }
                        },

                        onselectionchange: {
                            let mut doc = document.clone();
                            move |_evt| {
                                tracing::debug!("onselectionchange fired - syncing cursor from DOM");
                                let paras = cached_paragraphs();
                                let spans = syntax_spans();
                                sync_cursor_and_visibility(&mut doc, editor_id, &paras, &spans, None);
                            }
                        },

                        onclick: {
                            let mut doc = document.clone();
                            move |evt| {
                                tracing::debug!("onclick fired - syncing cursor from DOM");
                                let paras = cached_paragraphs();
                                let spans = syntax_spans();
                                #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
                                let _ = evt;

                                // Check if click target is a math-clickable element.
                                #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
                                {
                                    let map = offset_map();
                                    use dioxus::web::WebEventExt;

                                    let web_evt = evt.as_web_event();
                                    if let Some(target) = web_evt.target() {
                                        if weaver_editor_browser::handle_math_click(
                                            &target, &mut doc, &spans, &paras, &map,
                                        ) {
                                            return;
                                        }
                                    }
                                }

                                sync_cursor_and_visibility(&mut doc, editor_id, &paras, &spans, None);
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
                                handle_compositionstart(evt, &mut doc);
                            }
                        },

                        oncompositionupdate: {
                            let mut doc = document.clone();
                            move |evt: CompositionEvent| {
                                handle_compositionupdate(evt, &mut doc);
                            }
                        },

                        oncompositionend: {
                            let mut doc = document.clone();
                            move |evt: CompositionEvent| {
                                handle_compositionend(evt, &mut doc);
                            }
                        },
                        }
                        div { class: "editor-debug",
                            div { "Cursor: {document.cursor.read().offset}, Chars: {document.len_chars()}" },
                            // Collab debug info
                            {
                                if let Some(debug_state) = crate::collab_context::try_use_collab_debug() {
                                    let ds = debug_state.read();
                                    rsx! {
                                        div { class: "collab-debug",
                                            if let Some(ref node_id) = ds.node_id {
                                                span { title: "{node_id}", "Node: {&node_id[..8.min(node_id.len())]}…" }
                                            }
                                            if ds.is_joined {
                                                span { class: "joined", "✓ Joined" }
                                            }
                                            span { "Peers: {ds.discovered_peers}" }
                                            if let Some(ref err) = ds.last_error {
                                                span { class: "error", title: "{err}", "⚠" }
                                            }
                                        }
                                    }
                                } else {
                                    rsx! {}
                                }
                            },
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
                            apply_formatting(&mut doc, action);
                        }
                    },
                    on_image: {
                        let mut doc = document.clone();
                        move |uploaded: super::image_upload::UploadedImage| {
                            super::image_upload::handle_image_upload(
                                uploaded,
                                &mut doc,
                                &mut image_resolver,
                                &auth_state,
                                &fetcher,
                            );
                        }
                    },
                }

            }
        }
    }
}
