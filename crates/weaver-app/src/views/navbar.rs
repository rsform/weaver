use crate::Route;
use crate::auth::AuthState;
use crate::components::button::{Button, ButtonVariant};
use crate::components::login::LoginModal;
use crate::data::{use_get_handle, use_load_handle};
use crate::fetch::Fetcher;
use dioxus::prelude::*;
use jacquard::types::string::Did;

const NAVBAR_CSS: Asset = asset!("/assets/styling/navbar.css");

/// The Navbar component that will be rendered on all pages of our app since every page is under the layout.
///
///
/// This layout component wraps the UI of [Route::Home] and [Route::Blog] in a common navbar. The contents of the Home and Blog
/// routes will be rendered under the outlet inside this component
#[component]
pub fn Navbar() -> Element {
    let route = use_route::<Route>();
    tracing::trace!("Route: {:?}", route);

    let mut auth_state = use_context::<Signal<crate::auth::AuthState>>();
    let (route_handle_res, route_handle) = use_load_handle(match &route {
        Route::EntryPage { ident, .. } => Some(ident.clone()),
        Route::RepositoryIndex { ident } => Some(ident.clone()),
        Route::NotebookIndex { ident, .. } => Some(ident.clone()),
        _ => None,
    });

    #[cfg(feature = "fullstack-server")]
    route_handle_res?;

    let fetcher = use_context::<Fetcher>();
    let mut show_login_modal = use_signal(|| false);

    rsx! {
        document::Link { rel: "stylesheet", href: NAVBAR_CSS }
        document::Link { rel: "stylesheet", href: asset!("/assets/styling/button.css") }
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
                    Route::RepositoryIndex { ident } => {
                        let route_handle = route_handle.read().clone();
                        let handle = route_handle.unwrap_or(ident.clone());
                        rsx! {
                            span { class: "breadcrumb-separator", " > " }
                            span { class: "breadcrumb breadcrumb-current", "@{handle}" }
                        }
                    },
                    Route::NotebookIndex { ident, book_title } => {
                        let route_handle = route_handle.read().clone();
                        let handle = route_handle.unwrap_or(ident.clone());
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
                        let route_handle = route_handle.read().clone();
                        let handle = route_handle.unwrap_or(ident.clone());
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
                    AuthButton { did: did.clone() }
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
                LoginModal {
                    open: show_login_modal
                }
            }
        }

        Outlet::<Route> {}
    }
}

#[component]
fn AuthButton(did: Did<'static>) -> Element {
    let auth_handle = use_get_handle(did);

    let fetcher = use_context::<Fetcher>();
    let mut auth_state = use_context::<Signal<AuthState>>();

    rsx! {
        Button {
            variant: ButtonVariant::Ghost,
            onclick: move |_| {
                let fetcher = fetcher.clone();
                auth_state.write().clear();
                async move {
                    fetcher.downgrade_to_unauthenticated().await;
                }
            },
            span { class: "auth-handle", "@{auth_handle()}" }
        }
    }
}
