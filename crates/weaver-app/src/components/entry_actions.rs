//! Action buttons for entries (edit, delete, remove from notebook).

use crate::Route;
use crate::auth::AuthState;
use crate::components::button::{Button, ButtonVariant};
use crate::components::dialog::{DialogContent, DialogDescription, DialogRoot, DialogTitle};
use crate::fetch::Fetcher;
use dioxus::prelude::*;
use jacquard::smol_str::SmolStr;
use jacquard::types::aturi::AtUri;
use jacquard::types::ident::AtIdentifier;
use jacquard::IntoStatic;
use weaver_api::com_atproto::repo::delete_record::DeleteRecord;
use weaver_api::com_atproto::repo::put_record::PutRecord;

const ENTRY_ACTIONS_CSS: Asset = asset!("/assets/styling/entry-actions.css");

#[derive(Props, Clone, PartialEq)]
pub struct EntryActionsProps {
    /// The AT-URI of the entry
    pub entry_uri: AtUri<'static>,
    /// The entry title (for display in confirmation)
    pub entry_title: String,
    /// Whether this entry is in a notebook (enables "remove from notebook")
    #[props(default = false)]
    pub in_notebook: bool,
    /// Notebook title (if in_notebook is true, used for edit route)
    #[props(default)]
    pub notebook_title: Option<SmolStr>,
    /// Callback when entry is removed from notebook (for optimistic UI update)
    #[props(default)]
    pub on_removed: Option<EventHandler<()>>,
}

/// Action buttons for an entry: edit, delete, optionally remove from notebook.
#[component]
pub fn EntryActions(props: EntryActionsProps) -> Element {
    let auth_state = use_context::<Signal<AuthState>>();
    let fetcher = use_context::<Fetcher>();
    let navigator = use_navigator();

    let mut show_delete_confirm = use_signal(|| false);
    let mut show_remove_confirm = use_signal(|| false);
    let mut show_dropdown = use_signal(|| false);
    let mut deleting = use_signal(|| false);
    let mut removing = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);

    // Check ownership - compare auth DID with entry's authority
    let current_did = auth_state.read().did.clone();
    let entry_authority = props.entry_uri.authority();
    let is_owner = match (&current_did, entry_authority) {
        (Some(current), AtIdentifier::Did(entry_did)) => *current == *entry_did,
        _ => false,
    };

    if !is_owner {
        return rsx! {};
    }

    // Extract rkey from URI for edit route
    let rkey = match props.entry_uri.rkey() {
        Some(r) => r.0.to_string(),
        None => return rsx! {}, // Can't edit without rkey
    };

    // Build edit route based on whether entry is in a notebook
    let ident = props.entry_uri.authority().clone();
    let edit_route = if props.in_notebook {
        if let Some(ref notebook) = props.notebook_title {
            Route::NotebookEntryEdit {
                ident: ident.into_static(),
                book_title: notebook.clone(),
                rkey: rkey.clone().into(),
            }
        } else {
            Route::StandaloneEntryEdit {
                ident: ident.into_static(),
                rkey: rkey.clone().into(),
            }
        }
    } else {
        Route::StandaloneEntryEdit {
            ident: ident.into_static(),
            rkey: rkey.clone().into(),
        }
    };

    let entry_uri_for_delete = props.entry_uri.clone();
    let entry_title = props.entry_title.clone();

    let delete_fetcher = fetcher.clone();
    let handle_delete = move |_| {
        let fetcher = delete_fetcher.clone();
        let uri = entry_uri_for_delete.clone();
        let navigator = navigator.clone();

        spawn(async move {
            use jacquard::prelude::*;

            deleting.set(true);
            error.set(None);

            let client = fetcher.get_client();
            let collection = uri.collection();
            let rkey = uri.rkey();

            if let (Some(collection), Some(rkey)) = (collection, rkey) {
                let did = match fetcher.current_did().await {
                    Some(d) => d,
                    None => {
                        error.set(Some("Not authenticated".to_string()));
                        deleting.set(false);
                        return;
                    }
                };

                let request = DeleteRecord::new()
                    .repo(AtIdentifier::Did(did))
                    .collection(collection.clone())
                    .rkey(rkey.clone())
                    .build();

                match client.send(request).await {
                    Ok(_) => {
                        show_delete_confirm.set(false);
                        // Navigate back to home after delete
                        navigator.push(Route::Home {});
                    }
                    Err(e) => {
                        error.set(Some(format!("Delete failed: {:?}", e)));
                    }
                }
            } else {
                error.set(Some("Invalid entry URI".to_string()));
            }
            deleting.set(false);
        });
    };

    // Handler for removing entry from notebook (keeps entry, just removes from notebook's list)
    let entry_uri_for_remove = props.entry_uri.clone();
    let notebook_title_for_remove = props.notebook_title.clone();
    let on_removed = props.on_removed.clone();
    let handle_remove_from_notebook = move |_| {
        let fetcher = fetcher.clone();
        let entry_uri = entry_uri_for_remove.clone();
        let notebook_title = notebook_title_for_remove.clone();
        let on_removed = on_removed.clone();

        spawn(async move {
            use jacquard::{from_data, to_data, prelude::*, types::string::Nsid};
            use weaver_api::sh_weaver::notebook::book::Book;

            let client = fetcher.get_client();

            removing.set(true);
            error.set(None);

            let notebook_title = match notebook_title {
                Some(t) => t,
                None => {
                    error.set(Some("No notebook specified".to_string()));
                    removing.set(false);
                    return;
                }
            };

            let did = match fetcher.current_did().await {
                Some(d) => d,
                None => {
                    error.set(Some("Not authenticated".to_string()));
                    removing.set(false);
                    return;
                }
            };

            // Get the notebook by title
            let ident = AtIdentifier::Did(did.clone());
            let notebook_result = fetcher.get_notebook(ident.clone(), notebook_title.clone()).await;

            let (notebook_view, _) = match notebook_result {
                Ok(Some(data)) => data.as_ref().clone(),
                Ok(None) => {
                    error.set(Some("Notebook not found".to_string()));
                    removing.set(false);
                    return;
                }
                Err(e) => {
                    error.set(Some(format!("Failed to get notebook: {:?}", e)));
                    removing.set(false);
                    return;
                }
            };

            // Parse the book record to get the entry_list
            let mut book: Book = match from_data(&notebook_view.record) {
                Ok(b) => b,
                Err(e) => {
                    error.set(Some(format!("Failed to parse notebook: {:?}", e)));
                    removing.set(false);
                    return;
                }
            };

            // Filter out the entry
            let entry_uri_str = entry_uri.as_str();
            let original_len = book.entry_list.len();
            book.entry_list.retain(|ref_| ref_.uri.as_str() != entry_uri_str);

            if book.entry_list.len() == original_len {
                error.set(Some("Entry not found in notebook".to_string()));
                removing.set(false);
                return;
            }

            // Get the notebook's rkey from its URI
            let notebook_rkey = match notebook_view.uri.rkey() {
                Some(r) => r,
                None => {
                    error.set(Some("Invalid notebook URI".to_string()));
                    removing.set(false);
                    return;
                }
            };

            // Convert book to Data for the request
            let book_data = match to_data(&book) {
                Ok(d) => d,
                Err(e) => {
                    error.set(Some(format!("Failed to serialize notebook: {:?}", e)));
                    removing.set(false);
                    return;
                }
            };

            // Update the notebook record
            let request = PutRecord::new()
                .repo(AtIdentifier::Did(did))
                .collection(Nsid::new_static("sh.weaver.notebook.book").unwrap())
                .rkey(notebook_rkey.clone())
                .record(book_data)
                .build();

            match client.send(request).await {
                Ok(_) => {
                    show_remove_confirm.set(false);
                    // Notify parent to remove from local state
                    if let Some(handler) = &on_removed {
                        handler.call(());
                    }
                }
                Err(e) => {
                    error.set(Some(format!("Failed to update notebook: {:?}", e)));
                }
            }
            removing.set(false);
        });
    };

    rsx! {
        document::Link { rel: "stylesheet", href: ENTRY_ACTIONS_CSS }

        div { class: "entry-actions",
            // Edit button (always visible for owner)
            Link {
                to: edit_route,
                class: "entry-action-link",
                Button {
                    variant: ButtonVariant::Ghost,
                    "Edit"
                }
            }

            // Dropdown for destructive actions
            div { class: "entry-actions-dropdown",
                Button {
                    variant: ButtonVariant::Ghost,
                    onclick: move |_| show_dropdown.toggle(),
                    "⋮"
                }

                if show_dropdown() {
                    div { class: "dropdown-menu",
                        if props.in_notebook {
                            button {
                                class: "dropdown-item",
                                onclick: move |_| {
                                    show_dropdown.set(false);
                                    show_remove_confirm.set(true);
                                },
                                "Remove from notebook"
                            }
                        }
                        button {
                            class: "dropdown-item dropdown-item-danger",
                            onclick: move |_| {
                                show_dropdown.set(false);
                                show_delete_confirm.set(true);
                            },
                            "Delete"
                        }
                    }
                }
            }

            // Delete confirmation dialog
            DialogRoot {
                open: show_delete_confirm(),
                on_open_change: move |open: bool| show_delete_confirm.set(open),
                DialogContent {
                    DialogTitle { "Delete Entry?" }
                    DialogDescription {
                        "Delete \"{entry_title}\"? This removes the published entry. You can restore from drafts if needed."
                    }
                    if let Some(ref err) = error() {
                        div { class: "dialog-error", "{err}" }
                    }
                    div { class: "dialog-actions",
                        Button {
                            variant: ButtonVariant::Destructive,
                            onclick: handle_delete,
                            disabled: deleting(),
                            if deleting() { "Deleting..." } else { "Delete" }
                        }
                        Button {
                            variant: ButtonVariant::Ghost,
                            onclick: move |_| show_delete_confirm.set(false),
                            "Cancel"
                        }
                    }
                }
            }

            // Remove from notebook confirmation dialog
            if props.in_notebook {
                {
                    let entry_title_for_remove = entry_title.clone();
                    rsx! {
                        DialogRoot {
                            open: show_remove_confirm(),
                            on_open_change: move |open: bool| show_remove_confirm.set(open),
                            DialogContent {
                                DialogTitle { "Remove from Notebook?" }
                                DialogDescription {
                                    "Remove \"{entry_title_for_remove}\" from this notebook? The entry will still exist but won't be part of this notebook."
                                }
                                if let Some(ref err) = error() {
                                    div { class: "dialog-error", "{err}" }
                                }
                                div { class: "dialog-actions",
                                    Button {
                                        variant: ButtonVariant::Primary,
                                        onclick: handle_remove_from_notebook,
                                        disabled: removing(),
                                        if removing() { "Removing..." } else { "Remove" }
                                    }
                                    Button {
                                        variant: ButtonVariant::Ghost,
                                        onclick: move |_| show_remove_confirm.set(false),
                                        "Cancel"
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
