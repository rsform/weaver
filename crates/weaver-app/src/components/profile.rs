#![allow(non_snake_case)]

use std::sync::Arc;

use crate::Route;
use crate::components::button::{Button, ButtonVariant};
use crate::components::collab::api::{ReceivedInvite, accept_invite, fetch_received_invites};
use crate::components::{
    BskyIcon, TangledIcon,
    avatar::{Avatar, AvatarImage},
};
use crate::fetch::Fetcher;
use dioxus::prelude::*;
use weaver_api::com_atproto::repo::strong_ref::StrongRef;
use weaver_api::sh_weaver::actor::{ProfileDataView, ProfileDataViewInner};
use weaver_common::agent::NotebookView;

const PROFILE_CSS: Asset = asset!("/assets/styling/profile.css");

#[component]
pub fn ProfileDisplay(
    profile: Memo<Option<ProfileDataView<'static>>>,
    notebooks: Memo<Option<Vec<(NotebookView<'static>, Vec<StrongRef<'static>>)>>>,
    #[props(default)] entry_count: usize,
    #[props(default)] is_own_profile: bool,
) -> Element {
    match &*profile.read() {
        Some(profile_view) => {
            let profile_view = Arc::new(profile_view.clone());
            rsx! {
                document::Stylesheet { href: PROFILE_CSS }

                div { class: "profile-display",
                    // Banner if present
                    {match &profile_view.inner {
                        ProfileDataViewInner::ProfileView(p) => {
                            if let Some(ref banner) = p.banner {
                                rsx! {
                                    div { class: "profile-banner",
                                        img { src: "{banner.as_ref()}", alt: "Profile banner" }
                                    }
                                }
                            } else {
                                rsx! { }
                            }
                        }
                        ProfileDataViewInner::ProfileViewDetailed(p) => {
                            if let Some(ref banner) = p.banner {
                                rsx! {
                                    div { class: "profile-banner",
                                        img { src: "{banner.as_ref()}", alt: "Profile banner" }
                                    }
                                }
                            } else {
                                rsx! { }
                            }
                        }
                        _ => rsx! { }
                    }}

                    div { class: "profile-content",
                        // Avatar and identity
                        ProfileIdentity { profile_view: profile_view.clone() }
                        div {
                            class: "profile-extras",
                            // Stats
                            ProfileStats { notebooks, entry_count }

                            // Links
                            ProfileLinks { profile_view }

                            // Invites (only on own profile)
                            if is_own_profile {
                                ProfileInvites {}
                            }
                        }
                    }
                }
            }
        }
        _ => rsx! {
            div { class: "profile-display profile-loading",
                "Loading profile..."
            }
        },
    }
}

#[component]
fn ProfileIdentity(profile_view: Arc<ProfileDataView<'static>>) -> Element {
    match &profile_view.inner {
        ProfileDataViewInner::ProfileView(profile) => {
            let display_name = profile
                .display_name
                .as_ref()
                .map(|n| n.as_ref())
                .unwrap_or("Unknown");

            // Format pronouns
            let pronouns_text = if let Some(ref pronouns) = profile.pronouns {
                if !pronouns.is_empty() {
                    Some(
                        pronouns
                            .iter()
                            .map(|p| p.as_ref())
                            .collect::<Vec<_>>()
                            .join(", "),
                    )
                } else {
                    None
                }
            } else {
                None
            };

            rsx! {
                div { class: "profile-identity",
                    div {
                        class: "profile-block",
                        if let Some(ref avatar) = profile.avatar {
                            Avatar {
                                AvatarImage { src: avatar.as_ref() }
                            }
                        }

                        div { class: "profile-name-section",
                            h1 { class: "profile-display-name",
                                "{display_name}"
                                if let Some(ref pronouns) = pronouns_text {
                                    span { class: "profile-pronouns", " ({pronouns})" }
                                }
                            }
                            div { class: "profile-handle", "@{profile.handle}" }

                            if let Some(ref location) = profile.location {
                                div { class: "profile-location", "{location}" }
                            }
                        }
                    }


                    if let Some(ref description) = profile.description {
                        div { class: "profile-description", "{description}" }
                    }
                }
            }
        }
        ProfileDataViewInner::ProfileViewDetailed(profile) => {
            let display_name = profile
                .display_name
                .as_ref()
                .map(|n| n.as_ref())
                .unwrap_or("Unknown");

            rsx! {
                div { class: "profile-identity",
                    div {
                        class: "profile-block",
                        if let Some(ref avatar) = profile.avatar {
                            Avatar {
                                AvatarImage { src: avatar.as_ref() }
                            }
                        }

                        div { class: "profile-name-section",
                            h1 { class: "profile-display-name", "{display_name}" }
                            div { class: "profile-handle", "@{profile.handle}" }
                        }
                    }

                    if let Some(ref description) = profile.description {
                        div { class: "profile-description", "{description}" }
                    }
                }
            }
        }
        ProfileDataViewInner::TangledProfileView(profile) => {
            rsx! {
                div { class: "profile-identity",
                    div { class: "profile-name-section",
                        h1 { class: "profile-display-name", "@{profile.handle.as_ref()}" }
                        //div { class: "profile-handle", "{profile.handle.as_ref()}" }

                        if let Some(ref location) = profile.location {
                            div { class: "profile-location", "{location}" }
                        }
                    }

                    if let Some(ref description) = profile.description {
                        div { class: "profile-description", "{description}" }
                    }
                }
            }
        }
        _ => rsx! {
            div { class: "profile-identity",
                "Unknown profile type"
            }
        },
    }
}

#[component]
fn ProfileStats(
    notebooks: Memo<Option<Vec<(NotebookView<'static>, Vec<StrongRef<'static>>)>>>,
    #[props(default)] entry_count: usize,
) -> Element {
    let notebook_count = notebooks.read().as_ref().map(|n| n.len()).unwrap_or(0);

    rsx! {
        div { class: "profile-stats",
            div { class: "profile-stat",
                span { class: "profile-stat-label", "{notebook_count} notebooks" }
            }
            if entry_count > 0 {
                div { class: "profile-stat",
                    span { class: "profile-stat-label", "{entry_count} entries" }
                }
            }
        }
    }
}

#[component]
fn ProfileLinks(profile_view: Arc<ProfileDataView<'static>>) -> Element {
    match &profile_view.inner {
        ProfileDataViewInner::ProfileView(profile) => {
            rsx! {
                div { class: "profile-links",
                    // Generic links
                    if let Some(ref links) = profile.links {
                        for link in links.iter() {
                            a {
                                href: "{link.as_ref()}",
                                target: "_blank",
                                rel: "noopener noreferrer",
                                class: "profile-link",
                                "{link.as_ref()}"
                            }
                        }
                    }

                    // Platform-specific links
                    if profile.bluesky.unwrap_or(false) {
                        a {
                            href: "https://bsky.app/profile/{profile.did}",
                            target: "_blank",
                            rel: "noopener noreferrer",
                            class: "profile-link profile-link-platform",
                            BskyIcon { width: 20, height: 20, style: "vertical-align: text-bottom" }
                            " Bluesky"
                        }
                    }

                    if profile.tangled.unwrap_or(false) {
                        a {
                            href: "https://tangled.org/{profile.did}",
                            target: "_blank",
                            rel: "noopener noreferrer",
                            class: "profile-link profile-link-platform",
                            TangledIcon { width: 20, height: 20, style: "vertical-align: text-bottom" }
                            " Tangled"
                        }
                    }

                    if profile.streamplace.unwrap_or(false) {
                        a {
                            href: "https://stream.place/{profile.did}",
                            target: "_blank",
                            rel: "noopener noreferrer",
                            class: "profile-link profile-link-platform",
                            "View on stream.place"
                        }
                    }
                }
            }
        }
        ProfileDataViewInner::ProfileViewDetailed(profile) => {
            // Bluesky ProfileViewDetailed - doesn't have weaver platform flags
            rsx! {
                div { class: "profile-links",
                    a {
                        href: "https://bsky.app/profile/{profile.did}",
                        target: "_blank",
                        rel: "noopener noreferrer",
                        class: "profile-link profile-link-platform",
                        BskyIcon { width: 20, height: 20, style: "vertical-align: text-bottom" }
                        " Bluesky"
                    }

                }
            }
        }
        ProfileDataViewInner::TangledProfileView(profile) => {
            rsx! {
                div { class: "profile-links",
                    if let Some(ref links) = profile.links {
                        for link in links.iter() {
                            a {
                                href: "{link.as_ref()}",
                                target: "_blank",
                                rel: "noopener noreferrer",
                                class: "profile-link",
                                "{link.as_ref()}"
                            }
                        }
                    }
                    a {
                        href: "https://tangled.org/{profile.did}",
                        target: "_blank",
                        rel: "noopener noreferrer",
                        class: "profile-link profile-link-platform",
                        TangledIcon { width: 20, height: 20, style: "vertical-align: text-bottom" }
                        " Tangled"
                    }

                    if profile.bluesky {
                        a {
                            href: "https://bsky.app/profile/{profile.did}",
                            target: "_blank",
                            rel: "noopener noreferrer",
                            class: "profile-link profile-link-platform",
                            BskyIcon { width: 20, height: 20, style: "vertical-align: text-bottom" }
                            " Bluesky"
                        }
                    }
                }
            }
        }
        _ => rsx! {},
    }
}

/// Shows pending collaboration invites on the user's own profile.
#[component]
fn ProfileInvites() -> Element {
    let fetcher = use_context::<Fetcher>();

    // Fetch received invites
    let invites_resource = {
        let fetcher = fetcher.clone();
        use_resource(move || {
            let fetcher = fetcher.clone();
            async move {
                fetch_received_invites(&fetcher)
                    .await
                    .ok()
                    .unwrap_or_default()
            }
        })
    };

    let invites: Vec<ReceivedInvite> = invites_resource().unwrap_or_default();

    // Don't render section if no invites
    if invites.is_empty() {
        return rsx! {};
    }

    rsx! {
        div { class: "profile-invites",
            h3 { class: "profile-invites-header", "Collaboration Invites" }

            div { class: "profile-invites-list",
                for invite in invites {
                    ProfileInviteCard { invite }
                }
            }
        }
    }
}

/// A single invite card in the profile sidebar.
#[component]
fn ProfileInviteCard(invite: ReceivedInvite) -> Element {
    let fetcher = use_context::<Fetcher>();
    let nav = use_navigator();
    let mut is_accepting = use_signal(|| false);
    let mut accepted = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);

    let invite_uri = invite.uri.clone();
    let invite_cid = invite.cid.clone();
    let resource_uri = invite.resource_uri.clone();
    let resource_uri_nav = invite.resource_uri.clone();

    let handle_accept = move |_| {
        let fetcher = fetcher.clone();
        let invite_uri = invite_uri.clone();
        let invite_cid = invite_cid.clone();
        let resource_uri = resource_uri.clone();
        let resource_uri_nav = resource_uri_nav.clone();

        spawn(async move {
            is_accepting.set(true);
            error.set(None);

            let invite_ref = StrongRef::new().uri(invite_uri).cid(invite_cid).build();

            match accept_invite(&fetcher, invite_ref, resource_uri).await {
                Ok(_) => {
                    accepted.set(true);
                    // Navigate to the resource after a short delay
                    #[cfg(target_arch = "wasm32")]
                    {
                        use gloo_timers::future::TimeoutFuture;
                        TimeoutFuture::new(500).await;
                    }
                    // Navigate to the entry - parse AT-URI into path segments
                    // at://did/collection/rkey -> ["did", "collection", "rkey"]
                    let uri_str = resource_uri_nav.to_string();
                    let uri_parts: Vec<String> = uri_str
                        .strip_prefix("at://")
                        .unwrap_or(&uri_str)
                        .split('/')
                        .map(|s| s.to_string())
                        .collect();
                    nav.push(Route::RecordPage { uri: uri_parts });
                }
                Err(e) => {
                    error.set(Some(format!("Failed: {}", e)));
                }
            }

            is_accepting.set(false);
        });
    };

    // Extract inviter display (last part of DID for now)
    let inviter_display = invite
        .inviter
        .as_ref()
        .split(':')
        .last()
        .unwrap_or("unknown")
        .chars()
        .take(12)
        .collect::<String>();

    rsx! {
        div { class: "profile-invite-card",
            div { class: "profile-invite-from",
                "From: "
                span { class: "profile-invite-did", "{inviter_display}…" }
            }

            if let Some(msg) = &invite.message {
                p { class: "profile-invite-message", "{msg}" }
            }

            if let Some(err) = error() {
                div { class: "profile-invite-error", "{err}" }
            }

            div { class: "profile-invite-actions",
                if accepted() {
                    span { class: "profile-invite-accepted", "Accepted ✓" }
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
