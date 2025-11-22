use crate::Route;
use crate::components::button::{Button, ButtonVariant};
use crate::components::login::LoginModal;
use crate::data::{get_handle, use_notebook_handle};
use crate::fetch::Fetcher;
use dioxus::prelude::*;

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
    let fetcher = use_context::<Fetcher>();
    let route_handle = use_signal(|| match &route {
        Route::EntryPage { ident, .. } => Some(ident.clone()),
        Route::RepositoryIndex { ident } => Some(ident.clone()),
        Route::NotebookIndex { ident, .. } => Some(ident.clone()),
        _ => None,
    });
    let notebook_handle = use_notebook_handle(route_handle);

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
                    Route::RepositoryIndex { .. } => {
                        let handle = notebook_handle.as_ref().unwrap();
                        rsx! {
                            span { class: "breadcrumb-separator", " > " }
                            span { class: "breadcrumb breadcrumb-current", "@{handle}" }
                        }
                    },
                    Route::NotebookIndex { ident, book_title } => {
                        let handle = notebook_handle.as_ref().unwrap();
                        rsx! {
                            span { class: "breadcrumb-separator", " > " }
                            Link {
                                to: Route::RepositoryIndex { ident: ident.clone() },
                                class: "breadcrumb",
                                "@{handle}"
                            }
                            span { class: "breadcrumb-separator", " > " }
                            span { class: "breadcrumb breadcrumb-current", "{book_title}" }
                        }
                    },
                    Route::EntryPage { ident, book_title, .. } => {
                        let handle = notebook_handle.as_ref().unwrap();
                        rsx! {
                            span { class: "breadcrumb-separator", " > " }
                            Link {
                                to: Route::RepositoryIndex { ident: ident.clone() },
                                class: "breadcrumb",
                                "@{handle}"
                            }
                            span { class: "breadcrumb-separator", " > " }
                            Link {
                                to: Route::NotebookIndex { ident: ident.clone(), book_title: book_title.clone() },
                                class: "breadcrumb",
                                "{book_title}"
                            }
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
                        span { class: "auth-handle", "@{get_handle(did.clone())}" }
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

        Outlet::<Route> {}
    }
}
