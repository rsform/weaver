//! Panel showing current collaborators on a resource.

use crate::auth::AuthState;
use crate::components::button::{Button, ButtonVariant};
use crate::fetch::Fetcher;
use dioxus::prelude::*;
use jacquard::types::string::AtUri;

use super::InviteDialog;
use super::api::{SentInvite, fetch_sent_invites};

/// Props for the CollaboratorsPanel component.
#[derive(Props, Clone, PartialEq)]
pub struct CollaboratorsPanelProps {
    /// The resource to show collaborators for.
    pub resource_uri: AtUri<'static>,
    /// CID of the resource.
    pub resource_cid: String,
    /// Optional title for display.
    #[props(default)]
    pub resource_title: Option<String>,
    /// Callback when panel should close (for modal mode).
    #[props(default)]
    pub on_close: Option<EventHandler<()>>,
}

/// Panel showing collaborators and invite button.
#[component]
pub fn CollaboratorsPanel(props: CollaboratorsPanelProps) -> Element {
    let auth_state = use_context::<Signal<AuthState>>();
    let fetcher = use_context::<Fetcher>();
    let mut show_invite_dialog = use_signal(|| false);

    // Clone props we need in closures
    let on_close = props.on_close.clone();
    let on_close_overlay = props.on_close.clone();
    let resource_uri = props.resource_uri.clone();
    let resource_uri_dialog = props.resource_uri.clone();
    let resource_cid = props.resource_cid.clone();
    let resource_title = props.resource_title.clone();

    // Fetch invites for this resource to show collaborators
    let invites_resource = {
        let fetcher = fetcher.clone();
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
                    .filter(|i| i.resource_uri == resource_uri)
                    .collect::<Vec<_>>()
            }
        })
    };

    let invites: Vec<SentInvite> = invites_resource().unwrap_or_default();
    let accepted_count = invites.iter().filter(|i| i.accepted).count();
    let pending_count = invites.len() - accepted_count;

    let is_modal = on_close.is_some();

    let panel_content = rsx! {
        div { class: "collaborators-panel",
            div { class: "collaborators-header",
                h4 { "Collaborators" }
                div { class: "collaborators-header-actions",
                    Button {
                        variant: ButtonVariant::Ghost,
                        onclick: move |_| show_invite_dialog.set(true),
                        "Invite"
                    }
                    if let Some(ref handler) = on_close {
                        {
                            let handler = handler.clone();
                            rsx! {
                                Button {
                                    variant: ButtonVariant::Ghost,
                                    onclick: move |_| handler.call(()),
                                    "×"
                                }
                            }
                        }
                    }
                }
            }

            if invites.is_empty() {
                p { class: "empty-state", "No collaborators yet" }
            } else {
                div { class: "collaborators-list",
                    for invite in &invites {
                        div {
                            class: if invite.accepted { "collaborator accepted" } else { "collaborator pending" },
                            span { class: "collaborator-did", "{invite.invitee}" }
                            span {
                                class: "collaborator-status",
                                if invite.accepted { "✓" } else { "..." }
                            }
                        }
                    }
                }

                div { class: "collaborators-summary",
                    "{accepted_count} active, {pending_count} pending"
                }
            }
        }

        InviteDialog {
            open: show_invite_dialog(),
            on_close: move |_| show_invite_dialog.set(false),
            resource_uri: resource_uri_dialog.clone(),
            resource_cid: resource_cid.clone(),
            resource_title: resource_title.clone(),
        }
    };

    if is_modal {
        rsx! {
            div {
                class: "collaborators-overlay",
                onclick: move |_| {
                    if let Some(ref handler) = on_close_overlay {
                        handler.call(());
                    }
                },
                div {
                    class: "collaborators-modal",
                    onclick: move |e| e.stop_propagation(),
                    {panel_content}
                }
            }
        }
    } else {
        panel_content
    }
}
