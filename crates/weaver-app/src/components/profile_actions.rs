//! Actions sidebar/menubar for profile page.

use crate::auth::AuthState;
use crate::components::app_link::{AppLink, AppLinkTarget};
use crate::components::button::{Button, ButtonVariant};
use dioxus::prelude::*;
use jacquard::types::ident::AtIdentifier;

const PROFILE_ACTIONS_CSS: Asset = asset!("/assets/styling/profile-actions.css");

/// Actions available on the profile page for the owner.
#[component]
pub fn ProfileActions(ident: ReadSignal<AtIdentifier<'static>>) -> Element {
    let auth_state = use_context::<Signal<AuthState>>();

    // Check if viewing own profile
    let is_owner = {
        let current_did = auth_state.read().did.clone();
        match (&current_did, ident()) {
            (Some(did), AtIdentifier::Did(ref ident_did)) => *did == *ident_did,
            _ => false,
        }
    };

    if !is_owner {
        return rsx! {};
    }

    rsx! {
        document::Link { rel: "stylesheet", href: PROFILE_ACTIONS_CSS }

        aside { class: "profile-actions",
            div { class: "profile-actions-container",
                div { class: "profile-actions-list",
                    AppLink {
                        to: AppLinkTarget::NewDraft { ident: ident(), notebook: None },
                        class: "profile-action-link".to_string(),
                        Button {
                            variant: ButtonVariant::Outline,
                            "New Entry"
                        }
                    }

                    // TODO: New Notebook button (disabled for now)
                    Button {
                        variant: ButtonVariant::Outline,
                        disabled: true,
                        "New Notebook"
                    }

                    AppLink {
                        to: AppLinkTarget::Drafts { ident: ident() },
                        class: "profile-action-link".to_string(),
                        Button {
                            variant: ButtonVariant::Ghost,
                            "Drafts"
                        }
                    }

                    AppLink {
                        to: AppLinkTarget::Invites { ident: ident() },
                        class: "profile-action-link".to_string(),
                        Button {
                            variant: ButtonVariant::Ghost,
                            "Invites"
                        }
                    }
                }
            }
        }
    }
}

/// Mobile-friendly menubar version of profile actions.
#[component]
pub fn ProfileActionsMenubar(ident: ReadSignal<AtIdentifier<'static>>) -> Element {
    let auth_state = use_context::<Signal<AuthState>>();

    let is_owner = {
        let current_did = auth_state.read().did.clone();
        match (&current_did, ident()) {
            (Some(did), AtIdentifier::Did(ref ident_did)) => *did == *ident_did,
            _ => false,
        }
    };

    if !is_owner {
        return rsx! {};
    }

    rsx! {
        div { class: "profile-actions-menubar",
            AppLink {
                to: AppLinkTarget::NewDraft { ident: ident(), notebook: None },
                Button {
                    variant: ButtonVariant::Primary,
                    "New Entry"
                }
            }

            AppLink {
                to: AppLinkTarget::Drafts { ident: ident() },
                Button {
                    variant: ButtonVariant::Ghost,
                    "Drafts"
                }
            }

            AppLink {
                to: AppLinkTarget::Invites { ident: ident() },
                Button {
                    variant: ButtonVariant::Ghost,
                    "Invites"
                }
            }
        }
    }
}
