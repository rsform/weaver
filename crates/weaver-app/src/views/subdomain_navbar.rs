//! Subdomain navbar component with auth support.

use dioxus::prelude::*;
use jacquard::types::string::{AtIdentifier, Did};

use crate::SubdomainRoute;
#[allow(unused_imports)]
use crate::auth::{AuthState, RestoreResult};
use crate::components::button::{Button, ButtonVariant};
use crate::components::login::LoginModal;
use crate::data::{use_get_handle, use_handle};
#[allow(unused_imports)]
use crate::env::WEAVER_APP_HOST;
use crate::fetch::Fetcher;
use crate::host_mode::SubdomainContext;
use crate::views::Footer;

#[cfg(feature = "fullstack-server")]
use {dioxus::fullstack::FullstackContext, http::StatusCode};

const NAVBAR_CSS: Asset = asset!("/assets/styling/navbar.css");
const BUTTON_CSS: Asset = asset!("/assets/styling/button.css");
const CARDS_BASE_CSS: Asset = asset!("/assets/styling/cards-base.css");
const ENTRY_CARD_CSS: Asset = asset!("/assets/styling/entry-card.css");
const NOTEBOOK_CARD_CSS: Asset = asset!("/assets/styling/notebook-card.css");

#[component]
pub fn SubdomainNavbar() -> Element {
    let ctx = use_context::<SubdomainContext>();
    let route = use_route::<SubdomainRoute>();
    let auth_state = use_context::<Signal<AuthState>>();

    #[allow(unused)]
    let fetcher = use_context::<Fetcher>();
    let mut show_login_modal = use_signal(|| false);

    // Show toast if session expired
    #[cfg(all(target_family = "wasm", target_os = "unknown"))]
    {
        use dioxus_primitives::toast::{ToastOptions, use_toast};

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

    // Provide navigator for programmatic navigation in shared components
    crate::components::use_subdomain_navigator_provider();

    rsx! {
        document::Link { rel: "stylesheet", href: NAVBAR_CSS }
        document::Link { rel: "stylesheet", href: BUTTON_CSS }
        document::Link { rel: "stylesheet", href: CARDS_BASE_CSS }
        document::Link { rel: "stylesheet", href: ENTRY_CARD_CSS }
        document::Link { rel: "stylesheet", href: NOTEBOOK_CARD_CSS }

        div { class: "app-shell",
            div {
                id: "navbar",
                nav { class: "breadcrumbs",
                    // Notebook title links to index
                    Link {
                        to: SubdomainRoute::SubdomainLanding {},
                        class: "breadcrumb",
                        "{ctx.notebook_title}"
                    }

                    // Show current location breadcrumb based on route
                    match &route {
                        SubdomainRoute::SubdomainLanding {} | SubdomainRoute::SubdomainIndexPage {} | SubdomainRoute::SubdomainEntryByRkey { .. } | SubdomainRoute::SubdomainEntry { .. } => {
                            rsx! {}
                        }
                        SubdomainRoute::SubdomainEntryEdit { rkey } => {
                            rsx! {
                                span { class: "breadcrumb-separator", " > " }
                                Link {
                                    to: SubdomainRoute::SubdomainEntryByRkey { rkey: rkey.clone() },
                                    class: "breadcrumb",
                                    "{rkey}"
                                }
                            }
                        }
                        SubdomainRoute::SubdomainProfile { ident } => {
                            rsx! {
                                span { class: "breadcrumb-separator", " > " }
                                span { class: "breadcrumb breadcrumb-current", "@{ident}" }
                            }
                        }
                    }
                }
                // Author profile link
                nav { class: "nav-tools",
                    AuthorProfileLink { ident: ctx.owner.clone() }
                }

                // Auth button
                if auth_state.read().is_authenticated() {
                    if let Some(did) = &auth_state.read().did {
                        SubdomainAuthButton { did: did.clone() }
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
                Outlet::<SubdomainRoute> {}
            }

            Footer { show_full: false }
        }
    }
}

#[component]
pub fn SubdomainErrorLayout() -> Element {
    rsx! {
        ErrorBoundary {
            handle_error: move |_err: ErrorContext| {
                #[cfg(feature = "fullstack-server")]
                {
                    let http_error = FullstackContext::commit_error_status(_err.error().unwrap());
                    match http_error.status {
                        StatusCode::NOT_FOUND => rsx! { div { "404 - Page not found" } },
                        _ => rsx! { div { "An unknown error occurred" } },
                    }
                }
                #[cfg(not(feature = "fullstack-server"))]
                {
                    rsx! { div { "An error occurred" } }
                }
            },
            Outlet::<SubdomainRoute> {}
        }
    }
}

#[component]
fn SubdomainAuthButton(did: Did<'static>) -> Element {
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

#[component]
fn AuthorProfileLink(ident: ReadSignal<AtIdentifier<'static>>) -> Element {
    let (handle_res, handle) = use_handle(ident);

    #[cfg(feature = "fullstack-server")]
    handle_res?;

    rsx! {
        Link {
            to: SubdomainRoute::SubdomainProfile { ident: ident() },
            class: "nav-tool-link",
            "@{handle()}"
        }
    }
}
