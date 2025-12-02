//! Drafts and standalone entry views.

use crate::Route;
use crate::auth::AuthState;
use crate::components::button::{Button, ButtonVariant};
use crate::components::dialog::{DialogContent, DialogDescription, DialogRoot, DialogTitle};
use crate::components::editor::{delete_draft, list_drafts};
use dioxus::prelude::*;
use jacquard::smol_str::SmolStr;
use jacquard::types::ident::AtIdentifier;

const DRAFTS_CSS: Asset = asset!("/assets/styling/drafts.css");

/// Drafts list page - shows all drafts for the authenticated user.
#[component]
pub fn DraftsList(ident: ReadSignal<AtIdentifier<'static>>) -> Element {
    // ALL hooks must be called unconditionally at the top
    let auth_state = use_context::<Signal<AuthState>>();
    let navigator = use_navigator();
    let mut drafts = use_signal(list_drafts);
    let mut show_delete_confirm = use_signal(|| None::<String>);

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

    let mut handle_delete = move |key: String| {
        delete_draft(&key);
        drafts.set(list_drafts());
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

            if drafts().is_empty() {
                div { class: "drafts-empty",
                    p { "No drafts yet." }
                    p { "Start writing something new!" }
                }
            } else {
                div { class: "drafts-list",
                    for (key, title, editing_uri) in drafts() {
                        {
                            let key_for_delete = key.clone();
                            let is_edit_draft = editing_uri.is_some();
                            let display_title = if title.is_empty() { "Untitled".to_string() } else { title };
                            let tid = key.strip_prefix("new:").unwrap_or(&key);

                            rsx! {
                                div {
                                    class: "draft-card",
                                    key: "{key}",

                                    Link {
                                        to: Route::DraftEdit {
                                            ident: ident(),
                                            tid: tid.to_string().into(),
                                        },
                                        class: "draft-card-link",

                                        div { class: "draft-card-content",
                                            h3 { class: "draft-title", "{display_title}" }
                                            if is_edit_draft {
                                                span { class: "draft-badge draft-badge-edit", "Editing" }
                                            } else {
                                                span { class: "draft-badge draft-badge-new", "New" }
                                            }
                                        }
                                    }

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
    use crate::views::editor::EditorCss;

    // Construct AT-URI for the entry
    let entry_uri =
        use_memo(move || format!("at://{}/sh.weaver.notebook.entry/{}", ident(), rkey()));

    rsx! {
        EditorCss {}
        div { class: "editor-page",
            MarkdownEditor { entry_uri: Some(entry_uri()), target_notebook: Some(book_title()) }
        }
    }
}
