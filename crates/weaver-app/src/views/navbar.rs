use crate::Route;
use crate::components::button::{Button, ButtonVariant};
use crate::components::login::LoginModal;
use crate::data::use_handle;
use crate::fetch::CachedFetcher;
use dioxus::prelude::*;
use jacquard::types::string::AtIdentifier;

const THEME_DEFAULTS_CSS: Asset = asset!("/assets/styling/theme-defaults.css");
const NAVBAR_CSS: Asset = asset!("/assets/styling/navbar.css");

/// The Navbar component that will be rendered on all pages of our app since every page is under the layout.
///
///
/// This layout component wraps the UI of [Route::Home] and [Route::Blog] in a common navbar. The contents of the Home and Blog
/// routes will be rendered under the outlet inside this component
#[component]
pub fn Navbar() -> Element {
    let route = use_route::<Route>();
    let mut auth_state = use_context::<Signal<crate::auth::AuthState>>();
    let mut show_login_modal = use_signal(|| false);
    let fetcher = use_context::<CachedFetcher>();

    rsx! {
        document::Link { rel: "stylesheet", href: THEME_DEFAULTS_CSS }
        document::Link { rel: "stylesheet", href: NAVBAR_CSS }

        div {
            id: "navbar",
            nav { class: "breadcrumbs",
                Link {
                    to: Route::Home {},
                    class: "breadcrumb",
                    "Home"
                }

                // Show repository breadcrumb if we're on a repository page
                match route {
                    Route::RepositoryIndex { ident } => rsx! {
                        span { class: "breadcrumb-separator", " > " }
                        span { class: "breadcrumb breadcrumb-current", "@{use_handle(ident.clone())?}" }
                    },
                    Route::NotebookIndex { ident, book_title } => rsx! {
                        span { class: "breadcrumb-separator", " > " }
                        Link {
                            to: Route::RepositoryIndex { ident: ident.clone() },
                            class: "breadcrumb",
                            "@{use_handle(ident.clone())?}"
                        }
                        span { class: "breadcrumb-separator", " > " }
                        span { class: "breadcrumb breadcrumb-current", "{book_title}" }
                    },
                    Route::Entry { ident, book_title, .. } => rsx! {
                        span { class: "breadcrumb-separator", " > " }
                        Link {
                            to: Route::RepositoryIndex { ident: ident.clone() },
                            class: "breadcrumb",
                            "@{use_handle(ident.clone())?}"
                        }
                        span { class: "breadcrumb-separator", " > " }
                        Link {
                            to: Route::NotebookIndex { ident: ident.clone(), book_title: book_title.clone() },
                            class: "breadcrumb",
                            "{book_title}"
                        }
                    },
                    _ => rsx! {}
                }
            }
            if auth_state.read().is_authenticated() {
                if let Some(did) = &auth_state.read().did {
                    Button {
                        variant: ButtonVariant::Ghost,
                        onclick: move |_| {
                            let fetcher = fetcher.clone();
                            auth_state.write().clear();
                            async move {
                                fetcher.downgrade_to_unauthenticated().await;
                            }
                        },
                        span { class: "auth-handle", "@{use_handle(AtIdentifier::Did(did.clone()))?}" }
                    }
                }
            } else {
                div {
                    class: "auth-button",
                    Button {
                        variant: ButtonVariant::Ghost,
                        onclick: move |_| show_login_modal.set(true),
                        span { class: "auth-handle", "Sign In" }
                    }
                }

            }
            LoginModal {
                open: show_login_modal
            }
        }

        // The `Outlet` component is used to render the next component inside the layout. In this case, it will render either
        // the [`Home`] or [`Blog`] component depending on the current route.
        Outlet::<Route> {}
    }
}
