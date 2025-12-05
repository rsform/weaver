//! List of pending collaboration invites.

use crate::auth::AuthState;
use crate::components::button::{Button, ButtonVariant};
use crate::fetch::Fetcher;
use dioxus::prelude::*;
use jacquard::IntoStatic;
use jacquard::types::string::{AtUri, Cid};
use weaver_api::com_atproto::repo::strong_ref::StrongRef;

use super::api::{
    ReceivedInvite, SentInvite, accept_invite, fetch_received_invites, fetch_sent_invites,
};

/// Props for the InvitesList component.
#[derive(Props, Clone, PartialEq)]
pub struct InvitesListProps {
    /// Filter to a specific resource (optional).
    #[props(default)]
    pub resource_uri: Option<AtUri<'static>>,
}

/// List showing both sent and received invites.
#[component]
pub fn InvitesList(props: InvitesListProps) -> Element {
    let auth_state = use_context::<Signal<AuthState>>();
    let fetcher = use_context::<Fetcher>();

    let sent_invites = {
        let fetcher = fetcher.clone();
        use_resource(move || {
            let fetcher = fetcher.clone();
            let _auth = auth_state.read().did.clone();
            async move { fetch_sent_invites(&fetcher).await.ok().unwrap_or_default() }
        })
    };

    let received_invites = {
        let fetcher = fetcher.clone();
        use_resource(move || {
            let fetcher = fetcher.clone();
            let _auth = auth_state.read().did.clone();
            async move {
                fetch_received_invites(&fetcher)
                    .await
                    .ok()
                    .unwrap_or_default()
            }
        })
    };

    let filter_uri = props.resource_uri.clone();

    rsx! {
        div { class: "invites-list",
            // Received invites section
            div { class: "invites-section",
                h3 { "Received Invites" }
                {
                    let invites: Vec<ReceivedInvite> = received_invites()
                        .unwrap_or_default()
                        .into_iter()
                        .filter(|i| {
                            filter_uri.as_ref().map_or(true, |uri| &i.resource_uri == uri)
                        })
                        .collect();

                    if invites.is_empty() {
                        rsx! { p { class: "empty-state", "No pending invites" } }
                    } else {
                        rsx! {
                            for invite in invites {
                                ReceivedInviteCard { invite: invite.clone() }
                            }
                        }
                    }
                }
            }

            // Sent invites section
            div { class: "invites-section",
                h3 { "Sent Invites" }
                {
                    let invites: Vec<SentInvite> = sent_invites()
                        .unwrap_or_default()
                        .into_iter()
                        .filter(|i| {
                            filter_uri.as_ref().map_or(true, |uri| &i.resource_uri == uri)
                        })
                        .collect();

                    if invites.is_empty() {
                        rsx! { p { class: "empty-state", "No sent invites" } }
                    } else {
                        rsx! {
                            for invite in invites {
                                SentInviteCard { invite: invite.clone() }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Card showing a received invite with accept/decline actions.
#[component]
fn ReceivedInviteCard(invite: ReceivedInvite) -> Element {
    let fetcher = use_context::<Fetcher>();
    let mut is_accepting = use_signal(|| false);
    let mut accepted = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);

    let invite_uri = invite.uri.clone();
    let invite_cid = invite.cid.clone();
    let resource_uri = invite.resource_uri.clone();

    let handle_accept = move |_| {
        let fetcher = fetcher.clone();
        let invite_uri = invite_uri.clone();
        let invite_cid = invite_cid.clone();
        let resource_uri = resource_uri.clone();

        spawn(async move {
            is_accepting.set(true);
            error.set(None);

            let invite_ref = StrongRef::new().uri(invite_uri).cid(invite_cid).build();

            match accept_invite(&fetcher, invite_ref, resource_uri).await {
                Ok(_) => {
                    accepted.set(true);
                }
                Err(e) => {
                    error.set(Some(format!("Failed to accept: {}", e)));
                }
            }

            is_accepting.set(false);
        });
    };

    rsx! {
        div { class: "invite-card",
            div { class: "invite-info",
                span { class: "invite-from", "From: {invite.inviter}" }
                span { class: "invite-resource", "Resource: {invite.resource_uri}" }
                if let Some(msg) = &invite.message {
                    p { class: "invite-message", "{msg}" }
                }
            }

            if let Some(err) = error() {
                div { class: "error-message", "{err}" }
            }

            div { class: "invite-actions",
                if accepted() {
                    span { class: "invite-status accepted", "Accepted" }
                } else {
                    Button {
                        variant: ButtonVariant::Primary,
                        onclick: handle_accept,
                        disabled: is_accepting(),
                        if is_accepting() { "Accepting..." } else { "Accept" }
                    }
                }
            }
        }
    }
}

/// Card showing a sent invite with status.
#[component]
fn SentInviteCard(invite: SentInvite) -> Element {
    rsx! {
        div { class: "invite-card",
            div { class: "invite-info",
                span { class: "invite-to", "To: {invite.invitee}" }
                span { class: "invite-resource", "Resource: {invite.resource_uri}" }
                if let Some(msg) = &invite.message {
                    p { class: "invite-message", "{msg}" }
                }
            }

            div { class: "invite-status",
                if invite.accepted {
                    span { class: "status-badge accepted", "Accepted" }
                } else {
                    span { class: "status-badge pending", "Pending" }
                }
            }
        }
    }
}
