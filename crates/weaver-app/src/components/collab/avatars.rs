//! Collaborator avatars display for the editor meta row.

use std::sync::Arc;

use crate::auth::AuthState;
use crate::fetch::Fetcher;
use dioxus::prelude::*;
use jacquard::types::ident::AtIdentifier;
use jacquard::types::string::AtUri;
use weaver_api::sh_weaver::actor::{ProfileDataView, ProfileDataViewInner};

use super::api::find_all_participants;
use super::CollaboratorsPanel;

/// Props for the CollaboratorAvatars component.
#[derive(Props, Clone, PartialEq)]
pub struct CollaboratorAvatarsProps {
    /// The resource URI to show collaborators for.
    pub resource_uri: AtUri<'static>,
    /// CID of the resource.
    pub resource_cid: String,
    /// Optional title for display in the panel.
    #[props(default)]
    pub resource_title: Option<String>,
}

/// Shows collaborator avatars with a button to manage collaborators.
/// Displays all participants (collaborators with accepted invites) regardless of
/// whether you're the owner or a collaborator.
#[component]
pub fn CollaboratorAvatars(props: CollaboratorAvatarsProps) -> Element {
    let auth_state = use_context::<Signal<AuthState>>();
    let fetcher = use_context::<Fetcher>();
    let mut show_panel = use_signal(|| false);

    let resource_uri = props.resource_uri.clone();

    // Fetch all participants (owner + collaborators) with their profiles
    let collaborators = {
        let fetcher = fetcher.clone();
        let resource_uri = resource_uri.clone();
        use_resource(move || {
            let fetcher = fetcher.clone();
            let resource_uri = resource_uri.clone();
            let _auth = auth_state.read().did.clone(); // Reactivity trigger
            async move {
                let dids = find_all_participants(&fetcher, &resource_uri)
                    .await
                    .unwrap_or_default();

                // Fetch profile for each participant
                let mut profiles = Vec::new();
                for did in dids {
                    let ident = AtIdentifier::Did(did);
                    if let Ok(profile) = fetcher.fetch_profile(&ident).await {
                        profiles.push(profile);
                    }
                }
                profiles
            }
        })
    };

    let collabs: Vec<Arc<ProfileDataView<'static>>> = collaborators().unwrap_or_default();
    let collab_count = collabs.len();

    rsx! {
        div { class: "collaborator-avatars",
            onclick: move |_| show_panel.set(true),

            // Show up to 3 avatar circles
            for (i, profile) in collabs.iter().take(3).enumerate() {
                {
                    let (avatar, display_name, handle) = match &profile.inner {
                        ProfileDataViewInner::ProfileView(p) => (
                            p.avatar.as_ref(),
                            p.display_name.as_ref().map(|s| s.as_ref()),
                            p.handle.as_ref(),
                        ),
                        ProfileDataViewInner::ProfileViewDetailed(p) => (
                            p.avatar.as_ref(),
                            p.display_name.as_ref().map(|s| s.as_ref()),
                            p.handle.as_ref(),
                        ),
                        ProfileDataViewInner::TangledProfileView(p) => (
                            None,
                            None,
                            p.handle.as_ref(),
                        ),
                        _ => (None, None, "unknown"),
                    };
                    let title = display_name.unwrap_or(handle);
                    let initials = get_initials(display_name, handle);

                    rsx! {
                        div {
                            class: "collab-avatar",
                            style: "z-index: {3 - i}",
                            title: "{title}",

                            if let Some(avatar_url) = avatar {
                                img {
                                    class: "collab-avatar-img",
                                    src: avatar_url.as_ref(),
                                    alt: "{title}",
                                }
                            } else {
                                "{initials}"
                            }
                        }
                    }
                }
            }

            // Show +N if more than 3
            if collab_count > 3 {
                div { class: "collab-avatar collab-overflow",
                    "+{collab_count - 3}"
                }
            }

            // Always show the add button
            div { class: "collab-avatar collab-add",
                title: "Manage collaborators",
                "+"
            }
        }

        if show_panel() {
            CollaboratorsPanel {
                resource_uri: props.resource_uri.clone(),
                resource_cid: props.resource_cid.clone(),
                resource_title: props.resource_title.clone(),
                on_close: move |_| show_panel.set(false),
            }
        }
    }
}

/// Get initials from display name or handle.
fn get_initials(display_name: Option<&str>, handle: &str) -> String {
    if let Some(name) = display_name {
        name.split_whitespace()
            .take(2)
            .filter_map(|w| w.chars().next())
            .collect::<String>()
            .to_uppercase()
    } else {
        handle.chars().next().unwrap_or('?').to_uppercase().to_string()
    }
}
