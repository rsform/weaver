//! Actions sidebar/menubar for profile page.

use crate::Route;
use crate::auth::AuthState;
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
                    Link {
                        to: Route::NewDraft { ident: ident(), notebook: None },
                        class: "profile-action-link",
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

                    Link {
                        to: Route::DraftsList { ident: ident() },
                        class: "profile-action-link",
                        Button {
                            variant: ButtonVariant::Ghost,
                            "Drafts"
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
            Link {
                to: Route::NewDraft { ident: ident(), notebook: None },
                Button {
                    variant: ButtonVariant::Primary,
                    "New Entry"
                }
            }

            Link {
                to: Route::DraftsList { ident: ident() },
                Button {
                    variant: ButtonVariant::Ghost,
                    "Drafts"
                }
            }
        }
    }
}
