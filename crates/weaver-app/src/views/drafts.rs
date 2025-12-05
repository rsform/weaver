//! Drafts and standalone entry views.

use crate::Route;
use crate::auth::AuthState;
use crate::components::button::{Button, ButtonVariant};
use crate::components::dialog::{DialogContent, DialogDescription, DialogRoot, DialogTitle};
use crate::components::editor::{list_drafts_from_pds, RemoteDraft};
use crate::components::editor::{delete_draft, list_drafts};
use crate::fetch::Fetcher;
use dioxus::prelude::*;
use jacquard::smol_str::SmolStr;
use jacquard::types::ident::AtIdentifier;
use std::collections::HashSet;

const DRAFTS_CSS: Asset = asset!("/assets/styling/drafts.css");

/// Merged draft entry showing both local and remote state.
#[derive(Clone, Debug, PartialEq)]
struct MergedDraft {
    /// The rkey/tid of the draft
    rkey: String,
    /// Title from local storage (if available)
    title: String,
    /// Whether this draft exists locally
    is_local: bool,
    /// Whether this draft exists on PDS
    is_remote: bool,
    /// If editing an existing entry, the URI
    editing_uri: Option<String>,
}

/// Drafts list page - shows all drafts for the authenticated user.
#[component]
pub fn DraftsList(ident: ReadSignal<AtIdentifier<'static>>) -> Element {
    // ALL hooks must be called unconditionally at the top
    let auth_state = use_context::<Signal<AuthState>>();
    let fetcher = use_context::<Fetcher>();
    let navigator = use_navigator();
    let mut local_drafts = use_signal(list_drafts);
    let mut show_delete_confirm = use_signal(|| None::<String>);

    // Fetch remote drafts from PDS (depends on auth state to re-run when logged in)
    let remote_drafts_resource = use_resource(move || {
        let fetcher = fetcher.clone();
        let _did = auth_state.read().did.clone(); // Track auth state for reactivity
        async move { list_drafts_from_pds(&fetcher).await.ok().unwrap_or_default() }
    });

    // Check ownership - redirect if not viewing own drafts
    let current_did = auth_state.read().did.clone();
    let is_owner = match (&current_did, ident()) {
        (Some(did), AtIdentifier::Did(ref ident_did)) => *did == *ident_did,
        _ => false,
    };

    // Redirect non-owners
    let ident_for_redirect = ident();
    use_effect(move || {
        if !is_owner {
            navigator.replace(Route::RepositoryIndex {
                ident: ident_for_redirect.clone(),
            });
        }
    });

    if !is_owner {
        return rsx! { div { "Redirecting..." } };
    }

    // Merge local and remote drafts
    let merged_drafts = use_memo(move || {
        let local = local_drafts();
        let remote: Vec<RemoteDraft> = remote_drafts_resource().unwrap_or_default();

        tracing::debug!("Merging drafts: {} local, {} remote", local.len(), remote.len());
        for (key, _, _) in &local {
            tracing::debug!("  Local draft key: {}", key);
        }
        for rd in &remote {
            tracing::debug!("  Remote draft rkey: {}", rd.rkey);
        }

        // Build set of remote rkeys for quick lookup
        let remote_rkeys: HashSet<String> = remote.iter().map(|d| d.rkey.clone()).collect();

        // Build set of local rkeys
        let local_rkeys: HashSet<String> = local
            .iter()
            .map(|(key, _, _)| {
                key.strip_prefix("new:").unwrap_or(key).to_string()
            })
            .collect();

        let mut merged = Vec::new();

        // Add local drafts
        for (key, title, editing_uri) in &local {
            let rkey = key.strip_prefix("new:").unwrap_or(key).to_string();
            merged.push(MergedDraft {
                rkey: rkey.clone(),
                title: title.clone(),
                is_local: true,
                is_remote: remote_rkeys.contains(&rkey),
                editing_uri: editing_uri.clone(),
            });
        }

        // Add remote-only drafts
        for remote_draft in &remote {
            if !local_rkeys.contains(&remote_draft.rkey) {
                tracing::info!("Adding remote-only draft: {}", remote_draft.rkey);
                merged.push(MergedDraft {
                    rkey: remote_draft.rkey.clone(),
                    title: String::new(), // No local title available
                    is_local: false,
                    is_remote: true,
                    editing_uri: None,
                });
            }
        }

        // Sort by rkey (which is a TID, so newer drafts first)
        merged.sort_by(|a, b| b.rkey.cmp(&a.rkey));

        tracing::info!("Merged {} drafts total", merged.len());
        for m in &merged {
            tracing::info!("  Merged: rkey={} is_local={} is_remote={}", m.rkey, m.is_local, m.is_remote);
        }

        merged
    });

    let mut handle_delete = move |key: String| {
        delete_draft(&key);
        local_drafts.set(list_drafts());
        show_delete_confirm.set(None);
    };

    rsx! {
        document::Link { rel: "stylesheet", href: DRAFTS_CSS }
        document::Title { "Drafts" }

        div { class: "drafts-page",
            div { class: "drafts-header",
                h1 { "Drafts" }
                Link {
                    to: Route::NewDraft { ident: ident(), notebook: None },
                    Button {
                        variant: ButtonVariant::Primary,
                        "New Draft"
                    }
                }
            }

            if merged_drafts().is_empty() {
                div { class: "drafts-empty",
                    p { "No drafts yet." }
                    p { "Start writing something new!" }
                }
            } else {
                div { class: "drafts-list",
                    for draft in merged_drafts() {
                        {
                            let key_for_delete = format!("new:{}", draft.rkey);
                            let is_edit_draft = draft.editing_uri.is_some();
                            let display_title = if draft.title.is_empty() {
                                "Untitled".to_string()
                            } else {
                                draft.title.clone()
                            };

                            // Determine sync status badge
                            let (sync_badge, sync_class) = match (draft.is_local, draft.is_remote) {
                                (true, true) => ("Synced", "draft-badge-synced"),
                                (true, false) => ("Local", "draft-badge-local"),
                                (false, true) => ("Remote", "draft-badge-remote"),
                                (false, false) => ("", ""), // shouldn't happen
                            };
                            tracing::info!("Rendering draft {} - badge='{}' class='{}'", draft.rkey, sync_badge, sync_class);

                            rsx! {
                                div {
                                    class: "draft-card",
                                    key: "{draft.rkey}",

                                    Link {
                                        to: Route::DraftEdit {
                                            ident: ident(),
                                            tid: draft.rkey.clone().into(),
                                        },
                                        class: "draft-card-link",

                                        div { class: "draft-card-content",
                                            h3 { class: "draft-title", "{display_title}" }
                                            div { class: "draft-badges",
                                                if is_edit_draft {
                                                    span { class: "draft-badge draft-badge-edit", "Editing" }
                                                }
                                                if !sync_badge.is_empty() {
                                                    span { class: "draft-badge {sync_class}", "{sync_badge}" }
                                                }
                                            }
                                        }
                                    }

                                    if draft.is_local {
                                        Button {
                                            variant: ButtonVariant::Ghost,
                                            onclick: move |_| show_delete_confirm.set(Some(key_for_delete.clone())),
                                            "×"
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // Delete confirmation
        DialogRoot {
            open: show_delete_confirm().is_some(),
            on_open_change: move |_: bool| show_delete_confirm.set(None),
            DialogContent {
                DialogTitle { "Delete Draft?" }
                DialogDescription {
                    "This will permanently delete this draft."
                }
                div { class: "dialog-actions",
                    Button {
                        variant: ButtonVariant::Destructive,
                        onclick: move |_| {
                            if let Some(key) = show_delete_confirm() {
                                handle_delete(key);
                            }
                        },
                        "Delete"
                    }
                    Button {
                        variant: ButtonVariant::Ghost,
                        onclick: move |_| show_delete_confirm.set(None),
                        "Cancel"
                    }
                }
            }
        }
    }
}

/// Edit an existing draft by TID.
#[component]
pub fn DraftEdit(ident: ReadSignal<AtIdentifier<'static>>, tid: ReadSignal<SmolStr>) -> Element {
    use crate::components::editor::MarkdownEditor;
    use crate::views::editor::EditorCss;

    // Draft key for "new" drafts is "new:{tid}"
    let draft_key = format!("new:{}", tid());

    rsx! {
        EditorCss {}
        div { class: "editor-page",
            MarkdownEditor { entry_uri: Some(draft_key), target_notebook: None }
        }
    }
}

/// Create a new draft.
#[component]
pub fn NewDraft(
    ident: ReadSignal<AtIdentifier<'static>>,
    notebook: ReadSignal<Option<SmolStr>>,
) -> Element {
    use crate::components::editor::MarkdownEditor;
    use crate::views::editor::EditorCss;

    rsx! {
        EditorCss {}
        div { class: "editor-page",
            MarkdownEditor {
                entry_uri: None,
                target_notebook: notebook()
            }
        }
    }
}

/// Edit a standalone entry.
#[component]
pub fn StandaloneEntryEdit(
    ident: ReadSignal<AtIdentifier<'static>>,
    rkey: ReadSignal<SmolStr>,
) -> Element {
    use crate::components::editor::MarkdownEditor;
    use crate::views::editor::EditorCss;

    // Construct AT-URI for the entry
    let entry_uri =
        use_memo(move || format!("at://{}/sh.weaver.notebook.entry/{}", ident(), rkey()));

    rsx! {
        EditorCss {}
        div { class: "editor-page",
            MarkdownEditor { entry_uri: Some(entry_uri()), target_notebook: None }
        }
    }
}

/// Edit a notebook entry by rkey.
#[component]
pub fn NotebookEntryEdit(
    ident: ReadSignal<AtIdentifier<'static>>,
    book_title: ReadSignal<SmolStr>,
    rkey: ReadSignal<SmolStr>,
) -> Element {
    use crate::components::editor::MarkdownEditor;
    use crate::data::use_notebook_entries;
    use crate::views::editor::EditorCss;
    use weaver_common::EntryIndex;

    // Construct AT-URI for the entry
    let entry_uri =
        use_memo(move || format!("at://{}/sh.weaver.notebook.entry/{}", ident(), rkey()));

    // Fetch notebook entries for wikilink validation
    let (_entries_resource, entries_memo) = use_notebook_entries(ident, book_title);

    // Build entry index from notebook entries
    let entry_index = use_memo(move || {
        entries_memo().map(|entries| {
            let mut index = EntryIndex::new();
            let ident_str = ident().to_string();
            let book = book_title();
            for book_entry in &entries {
                // EntryView has optional title/path
                let title = book_entry.entry.title.as_ref().map(|t| t.as_str()).unwrap_or("");
                let path = book_entry.entry.path.as_ref().map(|p| p.as_str()).unwrap_or("");
                if !title.is_empty() || !path.is_empty() {
                    // Build canonical URL: /{ident}/{book}/{path}
                    let canonical_url = format!("/{}/{}/{}", ident_str, book, path);
                    index.add_entry(title, path, canonical_url);
                }
            }
            index
        })
    });

    rsx! {
        EditorCss {}
        div { class: "editor-page",
            MarkdownEditor {
                entry_uri: Some(entry_uri()),
                target_notebook: Some(book_title()),
                entry_index: entry_index(),
            }
        }
    }
}
