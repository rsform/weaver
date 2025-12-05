//! Collaboration invites page.

use crate::Route;
use crate::auth::AuthState;
use crate::components::collab::InvitesList;
use dioxus::prelude::*;
use jacquard::types::ident::AtIdentifier;

const INVITES_CSS: Asset = asset!("/assets/styling/invites.css");

/// Page showing collaboration invites (sent and received).
#[component]
pub fn InvitesPage(ident: ReadSignal<AtIdentifier<'static>>) -> Element {
    let auth_state = use_context::<Signal<AuthState>>();
    let navigator = use_navigator();

    // Check ownership - only show to authenticated user viewing their own invites
    let current_did = auth_state.read().did.clone();
    let is_owner = match (&current_did, ident()) {
        (Some(did), AtIdentifier::Did(ref ident_did)) => *did == *ident_did,
        _ => false,
    };

    // Redirect non-owners
    let ident_for_redirect = ident();
    use_effect(move || {
        if !is_owner {
            navigator.replace(Route::RepositoryIndex {
                ident: ident_for_redirect.clone(),
            });
        }
    });

    if !is_owner {
        return rsx! { div { "Redirecting..." } };
    }

    rsx! {
        document::Stylesheet { href: INVITES_CSS }

        div { class: "invites-page",
            header { class: "invites-header",
                h1 { "Collaboration Invites" }
                p { class: "invites-description",
                    "Manage your collaboration invitations. Accept invites to collaborate on entries and notebooks."
                }
            }

            InvitesList {}
        }
    }
}
