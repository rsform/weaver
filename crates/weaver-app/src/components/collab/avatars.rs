//! Collaborator avatars display for the editor meta row.

use crate::auth::AuthState;
use crate::fetch::Fetcher;
use dioxus::prelude::*;
use jacquard::types::string::AtUri;

use super::api::{fetch_sent_invites, SentInvite};
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
#[component]
pub fn CollaboratorAvatars(props: CollaboratorAvatarsProps) -> Element {
    let auth_state = use_context::<Signal<AuthState>>();
    let fetcher = use_context::<Fetcher>();
    let mut show_panel = use_signal(|| false);

    let resource_uri = props.resource_uri.clone();

    // Fetch collaborators for this resource
    let collaborators = {
        let fetcher = fetcher.clone();
        let resource_uri = resource_uri.clone();
        use_resource(move || {
            let fetcher = fetcher.clone();
            let resource_uri = resource_uri.clone();
            let _auth = auth_state.read().did.clone();
            async move {
                fetch_sent_invites(&fetcher)
                    .await
                    .ok()
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|i| i.resource_uri == resource_uri && i.accepted)
                    .collect::<Vec<SentInvite>>()
            }
        })
    };

    let collabs: Vec<SentInvite> = collaborators().unwrap_or_default();
    let collab_count = collabs.len();

    rsx! {
        div { class: "collaborator-avatars",
            onclick: move |_| show_panel.set(true),

            // Show up to 3 avatar circles
            for (i, collab) in collabs.iter().take(3).enumerate() {
                div {
                    class: "collab-avatar",
                    style: "z-index: {3 - i}",
                    title: "{collab.invitee}",
                    // First letter of DID as placeholder
                    {collab.invitee.as_ref().chars().last().unwrap_or('?').to_string()}
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
