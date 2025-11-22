#![allow(non_snake_case)]

use std::sync::Arc;

use crate::components::{
    BskyIcon, TangledIcon,
    avatar::{Avatar, AvatarImage},
};
use dioxus::prelude::*;
use weaver_api::com_atproto::repo::strong_ref::StrongRef;
use weaver_api::sh_weaver::actor::{ProfileDataView, ProfileDataViewInner};
use weaver_common::agent::NotebookView;

const PROFILE_CSS: Asset = asset!("/assets/styling/profile.css");

#[component]
pub fn ProfileDisplay(
    profile: Memo<Option<ProfileDataView<'static>>>,
    notebooks: Memo<Option<Vec<(NotebookView<'static>, Vec<StrongRef<'static>>)>>>,
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
                            ProfileStats { notebooks: notebooks }

                            // Links
                            ProfileLinks { profile_view }
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
) -> Element {
    // Fetch notebook count
    let notebook_count = if let Some(notebooks) = &*notebooks.read() {
        notebooks.len()
    } else {
        0
    };

    rsx! {
        div { class: "profile-stats",
            div { class: "profile-stat",
                span { class: "profile-stat-label", "{notebook_count} notebooks" }
            }
            // TODO: Add entry count, subscriber counts when available
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
