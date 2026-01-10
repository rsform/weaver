use crate::Route;
use crate::auth::{AuthState, RestoreResult};
use crate::components::button::{Button, ButtonVariant};
use crate::components::login::LoginModal;
use crate::data::{use_get_handle, use_load_handle};
use crate::fetch::Fetcher;
use crate::views::{Footer, should_show_full_footer};
use dioxus::prelude::*;
use dioxus_primitives::toast::{ToastOptions, use_toast};
use jacquard::types::ident::AtIdentifier;
use jacquard::types::string::Did;

const NAVBAR_CSS: Asset = asset!("/assets/styling/navbar.css");
const CARDS_BASE_CSS: Asset = asset!("/assets/styling/cards-base.css");
const ENTRY_CARD_CSS: Asset = asset!("/assets/styling/entry-card.css");
const NOTEBOOK_CARD_CSS: Asset = asset!("/assets/styling/notebook-card.css");

/// The Navbar component that will be rendered on all pages of our app since every page is under the layout.
///
///
/// This layout component wraps the UI of [Route::Home] and [Route::Blog] in a common navbar. The contents of the Home and Blog
/// routes will be rendered under the outlet inside this component
#[component]
pub fn Navbar() -> Element {
    // Provide navigator for programmatic navigation in shared components
    crate::components::use_main_navigator_provider();

    let route = use_route::<Route>();
    tracing::trace!("Route: {:?}", route);

    let auth_state = use_context::<Signal<crate::auth::AuthState>>();

    // Show toast if session expired
    #[cfg(all(target_family = "wasm", target_os = "unknown"))]
    {
        let restore_result = use_context::<Resource<RestoreResult>>();
        let toast = use_toast();
        let mut shown = use_signal(|| false);

        if !shown() && restore_result() == Some(RestoreResult::SessionExpired) {
            shown.set(true);
            toast.warning(
                "Session Expired".to_string(),
                ToastOptions::new().description("Please sign in again"),
            );
        }
    }
    let (route_handle_res, route_handle) = use_load_handle(match &route {
        Route::EntryPage { ident, .. } => Some(ident.clone()),
        Route::RepositoryIndex { ident } => Some(ident.clone()),
        Route::NotebookIndex { ident, .. } => Some(ident.clone()),
        Route::DraftsList { ident } => Some(ident.clone()),
        Route::DraftEdit { ident, .. } => Some(ident.clone()),
        Route::NewDraft { ident, .. } => Some(ident.clone()),
        Route::StandaloneEntry { ident, .. } => Some(ident.clone()),
        Route::StandaloneEntryEdit { ident, .. } => Some(ident.clone()),
        Route::NotebookEntryByRkey { ident, .. } => Some(ident.clone()),
        Route::NotebookEntryEdit { ident, .. } => Some(ident.clone()),
        _ => None,
    });

    #[cfg(feature = "fullstack-server")]
    route_handle_res?;

    #[allow(unused)]
    let fetcher = use_context::<Fetcher>();
    let mut show_login_modal = use_signal(|| false);

    rsx! {
        document::Link { rel: "stylesheet", href: NAVBAR_CSS }
        document::Link { rel: "stylesheet", href: asset!("/assets/styling/button.css") }
        document::Link { rel: "stylesheet", href: CARDS_BASE_CSS }
        document::Link { rel: "stylesheet", href: ENTRY_CARD_CSS }
        document::Link { rel: "stylesheet", href: NOTEBOOK_CARD_CSS }

        div { class: "app-shell",
            div {
                id: "navbar",
                nav { class: "breadcrumbs",
                    // On home page: show profile link if authenticated, otherwise "Home"
                    match (&route, &auth_state.read().did) {
                        (Route::Home {}, Some(did)) => rsx! {
                            ProfileBreadcrumb { did: did.clone() }
                        },
                        _ => rsx! {
                            a {
                                href: "/",
                                class: "breadcrumb",
                                "Home"
                            }
                        }
                    }

                    // Show repository breadcrumb if we're on a repository page
                    match &route {
                        Route::RepositoryIndex { ident } => {
                            let route_handle = route_handle.read().clone();
                            let handle = route_handle.unwrap_or(ident.clone());
                            rsx! {
                                span { class:"breadcrumb-separator"," > "}
                                span { class:"breadcrumb breadcrumb-current","@{handle}"}
                            }
                        },
                        Route::NotebookIndex{ ident, book_title } => {
                            let route_handle = route_handle.read().clone();
                            let handle = route_handle.unwrap_or(ident.clone());
                            rsx! {
                                span { class:"breadcrumb-separator"," > " }
                                Link {
                                    to: Route::RepositoryIndex { ident: ident.clone()
                                    },
                                    class: "breadcrumb","@{handle}"
                                }
                                span{ class: "breadcrumb-separator"," > "}
                                span{ class: "breadcrumb breadcrumb-current","{book_title}"}
                            }
                        },
                        Route::EntryPage { ident, book_title, .. } => {
                            let route_handle=route_handle.read().clone();
                            let handle=route_handle.unwrap_or(ident.clone());
                            rsx! {
                                span { class:"breadcrumb-separator"," > "}
                                Link {
                                    to: Route::RepositoryIndex {
                                        ident:ident.clone()
                                    },
                                    class:"breadcrumb","@{handle}"
                                }
                                span { class:"breadcrumb-separator"," > "}
                                Link {
                                    to: Route::NotebookIndex {
                                        ident: ident.clone(),
                                        book_title: book_title.clone()
                                    },
                                    class: "breadcrumb",
                                    "{book_title}"
                                }
                            }
                        },
                        Route::DraftsList { ident } => {
                            let route_handle = route_handle.read().clone();
                            let handle = route_handle.unwrap_or(ident.clone());
                            rsx! {
                                span { class:"breadcrumb-separator"," > "}
                                Link {
                                    to: Route::RepositoryIndex { ident: ident.clone()
                                    },
                                    class: "breadcrumb","@{handle}"
                                }
                            }
                        },
                        Route::DraftEdit { ident, .. } => {
                            let route_handle = route_handle.read().clone();
                            let handle = route_handle.unwrap_or(ident.clone());
                            rsx! {
                                span { class:"breadcrumb-separator"," > "}
                                Link {
                                    to: Route::RepositoryIndex { ident: ident.clone()
                                    },
                                    class: "breadcrumb","@{handle}"
                                }
                            }
                        },
                        Route::NewDraft { ident, notebook } => {
                            let route_handle = route_handle.read().clone();
                            let handle = route_handle.unwrap_or(ident.clone());
                            if let Some(notebook) = notebook {
                                rsx! {
                                    span { class:"breadcrumb-separator"," > "}
                                    Link {
                                        to: Route::RepositoryIndex {
                                            ident:ident.clone()
                                        },
                                        class:"breadcrumb","@{handle}"
                                    }
                                    span { class:"breadcrumb-separator"," > "}
                                    Link {
                                        to: Route::NotebookIndex {
                                            ident: ident.clone(),
                                            book_title: notebook.clone()
                                        },
                                        class: "breadcrumb",
                                        "{notebook}"
                                    }
                                }
                            } else {
                                rsx! {
                                    span { class:"breadcrumb-separator"," > "}
                                    span { class:"breadcrumb breadcrumb-current","@{handle}"}
                                }
                            }
                        },
                        Route::StandaloneEntry { ident, .. } => {
                            let route_handle = route_handle.read().clone();
                            let handle = route_handle.unwrap_or(ident.clone());
                            rsx! {
                                span { class:"breadcrumb-separator"," > "}
                                Link {
                                    to: Route::RepositoryIndex { ident: ident.clone()
                                    },
                                    class: "breadcrumb","@{handle}"
                                }
                            }
                        },
                        Route::StandaloneEntryEdit { ident, .. } => {
                            let route_handle = route_handle.read().clone();
                            let handle = route_handle.unwrap_or(ident.clone());
                            rsx! {
                                span { class:"breadcrumb-separator"," > "}
                                Link {
                                    to: Route::RepositoryIndex { ident: ident.clone()
                                    },
                                    class: "breadcrumb","@{handle}"
                                }
                            }
                        },
                        Route::NotebookEntryByRkey { ident, book_title, .. } => {
                            let route_handle=route_handle.read().clone();
                            let handle=route_handle.unwrap_or(ident.clone());
                            rsx! {
                                span { class:"breadcrumb-separator"," > "}
                                Link {
                                    to: Route::RepositoryIndex {
                                        ident:ident.clone()
                                    },
                                    class:"breadcrumb","@{handle}"
                                }
                                span { class:"breadcrumb-separator"," > "}
                                Link {
                                    to: Route::NotebookIndex {
                                        ident: ident.clone(),
                                        book_title: book_title.clone()
                                    },
                                    class: "breadcrumb",
                                    "{book_title}"
                                }
                            }
                        },
                        Route::NotebookEntryEdit { ident, book_title, .. } => {
                            let route_handle=route_handle.read().clone();
                            let handle=route_handle.unwrap_or(ident.clone());
                            rsx! {
                                span { class:"breadcrumb-separator"," > "}
                                Link {
                                    to: Route::RepositoryIndex {
                                        ident:ident.clone()
                                    },
                                    class:"breadcrumb","@{handle}"
                                }
                                span { class:"breadcrumb-separator"," > "}
                                Link {
                                    to: Route::NotebookIndex {
                                        ident: ident.clone(),
                                        book_title: book_title.clone()
                                    },
                                    class: "breadcrumb",
                                    "{book_title}"
                                }
                            }
                        },
                        _ => rsx! {},
                    }
                }

                // Tool links (show on home page)
                if matches!(route, Route::Home {}) {
                    nav { class: "nav-tools",
                        Link {
                            to: Route::RecordPage { uri: vec![] },
                            class: "nav-tool-link",
                            "Record Viewer"
                        }
                        Link {
                            to: Route::Editor { entry: None },
                            class: "nav-tool-link",
                            "Editor"
                        }
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
                        open: show_login_modal,
                        cached_route: format!("{}", route),
                    }
                }
            }

            main { class: "app-main",
                Outlet::<Route> {}
            }

            Footer { show_full: should_show_full_footer(&route) }
        }
    }
}

#[component]
fn ProfileBreadcrumb(did: Did<'static>) -> Element {
    rsx! {
        Link {
            to: Route::RepositoryIndex { ident: AtIdentifier::Did(did) },
            class: "breadcrumb",
            "Profile"
        }
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
