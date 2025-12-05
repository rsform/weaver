//! Action buttons for entries (edit, delete, remove from notebook, pin/unpin).

use crate::Route;
use crate::auth::AuthState;
use crate::components::button::{Button, ButtonVariant};
use crate::components::dialog::{DialogContent, DialogDescription, DialogRoot, DialogTitle};
use crate::fetch::Fetcher;
use dioxus::prelude::*;
use jacquard::smol_str::SmolStr;
use jacquard::types::aturi::AtUri;
use jacquard::types::ident::AtIdentifier;
use jacquard::types::string::Cid;
use jacquard::IntoStatic;
use weaver_api::com_atproto::repo::delete_record::DeleteRecord;
use weaver_api::com_atproto::repo::put_record::PutRecord;
use weaver_api::com_atproto::repo::strong_ref::StrongRef;
use weaver_api::sh_weaver::actor::profile::Profile as WeaverProfile;
use weaver_api::sh_weaver::notebook::PermissionsState;

const ENTRY_ACTIONS_CSS: Asset = asset!("/assets/styling/entry-actions.css");

#[derive(Props, Clone, PartialEq)]
pub struct EntryActionsProps {
    /// The AT-URI of the entry
    pub entry_uri: AtUri<'static>,
    /// The CID of the entry (for StrongRef when pinning)
    pub entry_cid: Cid<'static>,
    /// The entry title (for display in confirmation)
    pub entry_title: String,
    /// Whether this entry is in a notebook (enables "remove from notebook")
    #[props(default = false)]
    pub in_notebook: bool,
    /// Notebook title (if in_notebook is true, used for edit route)
    #[props(default)]
    pub notebook_title: Option<SmolStr>,
    /// Whether this entry is currently pinned
    #[props(default = false)]
    pub is_pinned: bool,
    /// Permissions state for edit access checking (if available)
    #[props(default)]
    pub permissions: Option<PermissionsState<'static>>,
    /// Callback when entry is removed from notebook (for optimistic UI update)
    #[props(default)]
    pub on_removed: Option<EventHandler<()>>,
    /// Callback when pin state changes
    #[props(default)]
    pub on_pinned_changed: Option<EventHandler<bool>>,
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
    let mut pinning = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);

    // Check edit access - use permissions if available, fall back to ownership check
    let current_did = auth_state.read().did.clone();
    let can_edit = match &current_did {
        Some(did) => {
            if let Some(ref perms) = props.permissions {
                // Use ACL-based permissions
                perms.editors.iter().any(|grant| grant.did == *did)
            } else {
                // Fall back to ownership check
                match props.entry_uri.authority() {
                    AtIdentifier::Did(entry_did) => *did == *entry_did,
                    _ => false,
                }
            }
        }
        None => false,
    };

    if !can_edit {
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
    let remove_fetcher = fetcher.clone();
    let handle_remove_from_notebook = move |_| {
        let fetcher = remove_fetcher.clone();
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

    // Handler for pinning/unpinning
    let entry_uri_for_pin = props.entry_uri.clone();
    let entry_cid_for_pin = props.entry_cid.clone();
    let is_currently_pinned = props.is_pinned;
    let on_pinned_changed = props.on_pinned_changed.clone();
    let pin_fetcher = fetcher.clone();
    let handle_pin_toggle = move |_| {
        let fetcher = pin_fetcher.clone();
        let entry_uri = entry_uri_for_pin.clone();
        let entry_cid = entry_cid_for_pin.clone();
        let on_pinned_changed = on_pinned_changed.clone();

        spawn(async move {
            use jacquard::{from_data, prelude::*, to_data, types::string::Nsid};
            use weaver_api::app_bsky::actor::profile::Profile as BskyProfile;

            pinning.set(true);
            error.set(None);

            let client = fetcher.get_client();

            let did = match fetcher.current_did().await {
                Some(d) => d,
                None => {
                    error.set(Some("Not authenticated".to_string()));
                    pinning.set(false);
                    return;
                }
            };

            let profile_uri_str = format!("at://{}/sh.weaver.actor.profile/self", did);

            // Try to fetch existing weaver profile
            let weaver_uri = match WeaverProfile::uri(&profile_uri_str) {
                Ok(u) => u,
                Err(_) => {
                    error.set(Some("Invalid profile URI".to_string()));
                    pinning.set(false);
                    return;
                }
            };
            let existing_profile: Option<WeaverProfile<'static>> =
                match client.fetch_record(&weaver_uri).await {
                    Ok(output) => Some(output.value),
                    Err(_) => None,
                };

            // Build the new pinned list
            let new_pinned: Vec<StrongRef<'static>> = if is_currently_pinned {
                // Unpin: remove from list
                existing_profile
                    .as_ref()
                    .and_then(|p| p.pinned.as_ref())
                    .map(|pins| {
                        pins.iter()
                            .filter(|r| r.uri.as_ref() != entry_uri.as_ref())
                            .cloned()
                            .collect()
                    })
                    .unwrap_or_default()
            } else {
                // Pin: add to list
                let new_ref = StrongRef::new()
                    .uri(entry_uri.clone().into_static())
                    .cid(entry_cid.clone())
                    .build();
                let mut pins = existing_profile
                    .as_ref()
                    .and_then(|p| p.pinned.clone())
                    .unwrap_or_default();
                // Don't add if already exists
                if !pins.iter().any(|r| r.uri.as_ref() == entry_uri.as_ref()) {
                    pins.push(new_ref);
                }
                pins
            };

            // Build the profile to save
            let profile_to_save = if let Some(existing) = existing_profile {
                // Update existing profile
                WeaverProfile {
                    pinned: Some(new_pinned),
                    ..existing
                }
            } else {
                // Create new profile from bsky data
                let bsky_uri_str = format!("at://{}/app.bsky.actor.profile/self", did);
                let bsky_profile: Option<BskyProfile<'static>> =
                    match BskyProfile::uri(&bsky_uri_str) {
                        Ok(bsky_uri) => match client.fetch_record(&bsky_uri).await {
                            Ok(output) => Some(output.value),
                            Err(_) => None,
                        },
                        Err(_) => None,
                    };

                WeaverProfile::new()
                    .maybe_display_name(
                        bsky_profile
                            .as_ref()
                            .and_then(|p| p.display_name.clone()),
                    )
                    .maybe_description(
                        bsky_profile.as_ref().and_then(|p| p.description.clone()),
                    )
                    .maybe_avatar(bsky_profile.as_ref().and_then(|p| p.avatar.clone()))
                    .maybe_banner(bsky_profile.as_ref().and_then(|p| p.banner.clone()))
                    .bluesky(true)
                    .created_at(jacquard::types::string::Datetime::now())
                    .pinned(new_pinned)
                    .build()
            };

            // Serialize and save
            let profile_data = match to_data(&profile_to_save) {
                Ok(d) => d,
                Err(e) => {
                    error.set(Some(format!("Failed to serialize profile: {:?}", e)));
                    pinning.set(false);
                    return;
                }
            };

            let request = PutRecord::new()
                .repo(AtIdentifier::Did(did))
                .collection(Nsid::new_static("sh.weaver.actor.profile").unwrap())
                .rkey(jacquard::types::string::Rkey::new("self").unwrap())
                .record(profile_data)
                .build();

            match client.send(request).await {
                Ok(_) => {
                    show_dropdown.set(false);
                    if let Some(handler) = &on_pinned_changed {
                        handler.call(!is_currently_pinned);
                    }
                }
                Err(e) => {
                    error.set(Some(format!("Failed to update profile: {:?}", e)));
                }
            }
            pinning.set(false);
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
                        // Pin/Unpin (first)
                        button {
                            class: "dropdown-item",
                            disabled: pinning(),
                            onclick: handle_pin_toggle,
                            if pinning() {
                                "Updating..."
                            } else if props.is_pinned {
                                "Unpin"
                            } else {
                                "Pin"
                            }
                        }
                        // Remove from notebook (if in notebook)
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
                        // Delete (last, danger style)
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
