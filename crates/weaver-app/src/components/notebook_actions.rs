//! Action buttons for notebooks (pin/unpin, delete).

use crate::auth::AuthState;
use crate::components::button::{Button, ButtonVariant};
use crate::components::dialog::{DialogContent, DialogDescription, DialogRoot, DialogTitle};
use crate::fetch::Fetcher;
use dioxus::prelude::*;
use jacquard::types::aturi::AtUri;
use jacquard::types::ident::AtIdentifier;
use jacquard::types::string::Cid;
use jacquard::IntoStatic;
use weaver_api::com_atproto::repo::delete_record::DeleteRecord;
use weaver_api::com_atproto::repo::put_record::PutRecord;
use weaver_api::com_atproto::repo::strong_ref::StrongRef;
use weaver_api::sh_weaver::actor::profile::Profile as WeaverProfile;

/// Action buttons for a notebook: pin/unpin, delete.
#[component]
pub fn NotebookActions(
    notebook_uri: AtUri<'static>,
    notebook_cid: Cid<'static>,
    notebook_title: String,
    #[props(default = false)] is_pinned: bool,
    #[props(default)] on_deleted: Option<EventHandler<()>>,
    #[props(default)] on_pinned_changed: Option<EventHandler<bool>>,
) -> Element {
    let auth_state = use_context::<Signal<AuthState>>();
    let fetcher = use_context::<Fetcher>();

    let mut show_delete_confirm = use_signal(|| false);
    let mut show_dropdown = use_signal(|| false);
    let mut deleting = use_signal(|| false);
    let mut pinning = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);

    // Check ownership - compare auth DID with notebook's authority
    let current_did = auth_state.read().did.clone();
    let notebook_authority = notebook_uri.authority();
    let is_owner = match (&current_did, notebook_authority) {
        (Some(current), AtIdentifier::Did(notebook_did)) => *current == *notebook_did,
        _ => false,
    };

    if !is_owner {
        return rsx! {};
    }

    let notebook_uri_for_delete = notebook_uri.clone();
    let title_for_display = notebook_title.clone();
    let on_deleted_handler = on_deleted.clone();

    let delete_fetcher = fetcher.clone();
    let handle_delete = move |_| {
        let fetcher = delete_fetcher.clone();
        let uri = notebook_uri_for_delete.clone();
        let on_deleted = on_deleted_handler.clone();

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
                        if let Some(handler) = &on_deleted {
                            handler.call(());
                        }
                    }
                    Err(e) => {
                        error.set(Some(format!("Delete failed: {:?}", e)));
                    }
                }
            } else {
                error.set(Some("Invalid notebook URI".to_string()));
            }
            deleting.set(false);
        });
    };

    // Handler for pinning/unpinning
    let notebook_uri_for_pin = notebook_uri.clone();
    let notebook_cid_for_pin = notebook_cid.clone();
    let is_currently_pinned = is_pinned;
    let on_pinned_changed_handler = on_pinned_changed.clone();
    let pin_fetcher = fetcher.clone();
    let handle_pin_toggle = move |_| {
        let fetcher = pin_fetcher.clone();
        let notebook_uri = notebook_uri_for_pin.clone();
        let notebook_cid = notebook_cid_for_pin.clone();
        let on_pinned_changed = on_pinned_changed_handler.clone();

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
                            .filter(|r| r.uri.as_ref() != notebook_uri.as_ref())
                            .cloned()
                            .collect()
                    })
                    .unwrap_or_default()
            } else {
                // Pin: add to list
                let new_ref = StrongRef::new()
                    .uri(notebook_uri.clone().into_static())
                    .cid(notebook_cid.clone())
                    .build();
                let mut pins = existing_profile
                    .as_ref()
                    .and_then(|p| p.pinned.clone())
                    .unwrap_or_default();
                // Don't add if already exists
                if !pins.iter().any(|r| r.uri.as_ref() == notebook_uri.as_ref()) {
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
        div { class: "notebook-actions",
            // Dropdown for actions
            div { class: "notebook-actions-dropdown",
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
                            } else if is_pinned {
                                "Unpin"
                            } else {
                                "Pin"
                            }
                        }
                        // Delete (danger style)
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
                    DialogTitle { "Delete Notebook?" }
                    DialogDescription {
                        "Delete \"{title_for_display}\"? The entries will remain but will no longer be part of this notebook."
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
        }
    }
}
