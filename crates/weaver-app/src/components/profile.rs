#![allow(non_snake_case)]

use crate::{
    components::{
        avatar::{Avatar, AvatarImage},
        BskyIcon, TangledIcon,
    },
    data::use_handle,
};
use dioxus::prelude::*;
use jacquard::types::ident::AtIdentifier;
use weaver_api::sh_weaver::actor::ProfileDataViewInner;

const PROFILE_CSS: Asset = asset!("/assets/styling/profile.css");

#[component]
pub fn ProfileDisplay(ident: AtIdentifier<'static>) -> Element {
    // Fetch profile data
    let profile = crate::data::use_profile_data(ident.clone())?;

    match profile().as_ref() {
        Some(profile_view) => rsx! {
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
                    ProfileIdentity { profile_view: profile_view.clone(), ident: ident.clone() }
                    div {
                        class: "profile-extras",
                        // Stats
                        ProfileStats { ident: ident.clone() }

                        // Links
                        ProfileLinks { profile_view: profile_view.clone(), ident: ident.clone() }
                    }


                }
            }
        },
        None => rsx! {
            div { class: "profile-display profile-loading",
                "Loading profile..."
            }
        },
    }
}

#[component]
fn ProfileIdentity(
    profile_view: weaver_api::sh_weaver::actor::ProfileDataView<'static>,
    ident: AtIdentifier<'static>,
) -> Element {
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
                            div { class: "profile-handle", "@{use_handle(ident.clone())?}" }

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
                            div { class: "profile-handle", "@{use_handle(ident.clone())?}" }
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
                        div { class: "profile-handle", "{ident}" }

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
fn ProfileStats(ident: AtIdentifier<'static>) -> Element {
    // Fetch notebook count
    let notebooks = crate::data::use_notebooks_for_did(ident.clone())?;

    let notebook_count = notebooks().as_ref().map(|n| n.len()).unwrap_or(0);

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
fn ProfileLinks(
    profile_view: weaver_api::sh_weaver::actor::ProfileDataView<'static>,
    ident: AtIdentifier<'static>,
) -> Element {
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
                            href: "https://bsky.app/profile/{ident}",
                            target: "_blank",
                            rel: "noopener noreferrer",
                            class: "profile-link profile-link-platform",
                            BskyIcon { width: 20, height: 20, style: "vertical-align: text-bottom" }
                            " Bluesky"
                        }
                    }

                    if profile.tangled.unwrap_or(false) {
                        a {
                            href: "https://tangled.org/@{ident}",
                            target: "_blank",
                            rel: "noopener noreferrer",
                            class: "profile-link profile-link-platform",
                            TangledIcon { width: 20, height: 20, style: "vertical-align: text-bottom" }
                            " Tangled"
                        }
                    }

                    if profile.streamplace.unwrap_or(false) {
                        a {
                            href: "https://stream.place/{ident}",
                            target: "_blank",
                            rel: "noopener noreferrer",
                            class: "profile-link profile-link-platform",
                            "View on stream.place"
                        }
                    }
                }
            }
        }
        ProfileDataViewInner::ProfileViewDetailed(_profile) => {
            // Bluesky ProfileViewDetailed - doesn't have weaver platform flags
            rsx! {
                div { class: "profile-links",
                    a {
                        href: "https://bsky.app/profile/{ident}",
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
                        href: "https://tangled.org/@{ident}",
                        target: "_blank",
                        rel: "noopener noreferrer",
                        class: "profile-link profile-link-platform",
                        TangledIcon { width: 20, height: 20, style: "vertical-align: text-bottom" }
                        " Tangled"
                    }

                    if profile.bluesky {
                        a {
                            href: "https://bsky.app/profile/{ident}",
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
