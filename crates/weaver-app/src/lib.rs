//! Weaver App library.
#[allow(unused)]
use dioxus::{CapturedError, prelude::*};

#[cfg(feature = "fullstack-server")]
pub use dioxus::fullstack::FullstackContext;
use jacquard::oauth::{client::OAuthClient, session::ClientData};
#[allow(unused)]
use jacquard::{
    smol_str::SmolStr,
    types::{cid::Cid, string::AtIdentifier},
};
use std::sync::LazyLock;

pub mod auth;
#[cfg(feature = "server")]
pub mod blobcache;
pub mod cache_impl;
pub mod collab_context;
pub mod components;
pub mod config;
pub mod data;
pub mod env;
pub mod fetch;
pub mod host_mode;
#[cfg(feature = "server")]
pub mod og;
pub mod perf;
pub mod record_utils;
pub mod service_worker;

pub mod subdomain_app;
pub mod views;

pub use host_mode::{LinkMode, SubdomainContext};
pub use subdomain_app::SubdomainApp;
pub use subdomain_app::SubdomainRoute;

use auth::{AuthState, AuthStore};
use components::{EntryPage, Repository, RepositoryIndex};
use config::{Config, OAuthConfig};
#[allow(unused)]
use views::{
    AboutPage, Callback, DraftEdit, DraftsList, Editor, Home, InvitesPage, LeafletEntry,
    LeafletEntryNsid, Navbar, NewDraft, Notebook, NotebookEntryByRkey, NotebookEntryEdit,
    NotebookIndex, NotebookPage, PcktEntry, PcktEntryBlogNsid, PcktEntryNsid, PrivacyPage,
    RecordIndex, RecordPage, StandaloneEntry, StandaloneEntryEdit, StandaloneEntryNsid, TermsPage,
    WhiteWindEntry, WhiteWindEntryNsid,
};

use crate::{
    env::WEAVER_APP_DOMAIN,
    subdomain_app::{extract_subdomain, lookup_subdomain_context},
};

#[derive(Debug, Clone, Routable, PartialEq)]
#[rustfmt::skip]
pub enum Route {
    #[layout(Navbar)]
        #[route("/")]
        Home {},
        #[route("/editor?:entry")]
        Editor { entry: Option<String> },
        #[route("/about")]
        AboutPage {},
        #[route("/tos")]
        TermsPage {},
        #[route("/privacy")]
        PrivacyPage {},
        #[layout(ErrorLayout)]
        #[nest("/record")]
          #[layout(RecordIndex)]
            #[route("/:..uri")]
            RecordPage { uri: Vec<String> },
          #[end_layout]
        #[end_nest]
        #[route("/callback?:state&:iss&:code")]
        Callback { state: SmolStr, iss: SmolStr, code: SmolStr },
        #[nest("/:ident")]
          #[layout(Repository)]
            #[route("/")]
            RepositoryIndex { ident: AtIdentifier<'static> },
            // Drafts routes (before /:book_title to avoid capture)
            #[route("/drafts")]
            DraftsList { ident: AtIdentifier<'static> },
            #[route("/drafts/:tid")]
            DraftEdit { ident: AtIdentifier<'static>, tid: SmolStr },
            #[route("/new?:notebook")]
            NewDraft { ident: AtIdentifier<'static>, notebook: Option<SmolStr> },
            // Collaboration invites
            #[route("/invites")]
            InvitesPage { ident: AtIdentifier<'static> },
            // Standalone entry routes
            #[route("/e/:rkey")]
            StandaloneEntry { ident: AtIdentifier<'static>, rkey: SmolStr },
            #[route("/sh.weaver.notebook.entry/:rkey")]
            StandaloneEntryNsid { ident: AtIdentifier<'static>, rkey: SmolStr },
            #[route("/e/:rkey/edit")]
            StandaloneEntryEdit { ident: AtIdentifier<'static>, rkey: SmolStr },
            // External blog routes (short paths)
            #[route("/w/:rkey")]
            WhiteWindEntry { ident: AtIdentifier<'static>, rkey: SmolStr },
            #[route("/l/:rkey")]
            LeafletEntry { ident: AtIdentifier<'static>, rkey: SmolStr },
            #[route("/sd/:rkey")]
            PcktEntry { ident: AtIdentifier<'static>, rkey: SmolStr },
            // External blog routes (NSID paths - replace at:// with https://host/)
            #[route("/com.whtwnd.blog.entry/:rkey")]
            WhiteWindEntryNsid { ident: AtIdentifier<'static>, rkey: SmolStr },
            #[route("/pub.leaflet.document/:rkey")]
            LeafletEntryNsid { ident: AtIdentifier<'static>, rkey: SmolStr },
            #[route("/site.standard.document/:rkey")]
            PcktEntryNsid { ident: AtIdentifier<'static>, rkey: SmolStr },
            #[route("/blog.pckt.document/:rkey")]
            PcktEntryBlogNsid { ident: AtIdentifier<'static>, rkey: SmolStr },
            // Notebook routes
            #[nest("/:book_title")]
              #[layout(Notebook)]
              #[route("/")]
              NotebookIndex { ident: AtIdentifier<'static>, book_title: SmolStr },
                #[route("/:title")]
                EntryPage { ident: AtIdentifier<'static>, book_title: SmolStr, title: SmolStr },
                // Entry by rkey (canonical path)
                #[route("/e/:rkey")]
                NotebookEntryByRkey { ident: AtIdentifier<'static>, book_title: SmolStr, rkey: SmolStr },
                #[route("/e/:rkey/edit")]
                NotebookEntryEdit { ident: AtIdentifier<'static>, book_title: SmolStr, rkey: SmolStr },
}

pub static CONFIG: LazyLock<Config> = LazyLock::new(|| Config {
    oauth: OAuthConfig::from_env().as_metadata(),
});

const FAVICON: Asset = asset!("/assets/weaver_photo_sm.jpg");
const MAIN_CSS: Asset = asset!("/assets/styling/main.css");
const THEME_DEFAULTS_CSS: Asset = asset!("/assets/styling/theme-defaults.css");

#[component]
pub fn App() -> Element {
    #[allow(unused)]
    let fetcher = use_context_provider(|| {
        fetch::Fetcher::new(OAuthClient::new(
            AuthStore::new(),
            ClientData::new_public(CONFIG.oauth.clone()),
        ))
    });

    #[cfg(feature = "fullstack-server")]
    let ctx_resource = use_server_future({
        let fetcher = fetcher.clone();
        move || {
            let fetcher = fetcher.clone();
            async move {
                use dioxus::fullstack::FullstackContext;
                use http::header::HOST;

                // Get host header from request
                let Some(ctx) = FullstackContext::current() else {
                    tracing::warn!("No FullstackContext available");
                    return None;
                };
                let parts = ctx.parts_mut();

                let Some(host_header) = parts.headers.get(HOST) else {
                    tracing::warn!("No Host header in request");
                    return None;
                };
                let Ok(host) = host_header.to_str() else {
                    tracing::warn!("Host header not valid UTF-8");
                    return None;
                };

                let host_str = host.split(':').next().unwrap_or(host);
                let Some(subdomain) = extract_subdomain(host_str, WEAVER_APP_DOMAIN) else {
                    tracing::info!(
                        host_str,
                        domain = WEAVER_APP_DOMAIN,
                        "Not a subdomain request"
                    );
                    return None;
                };
                // Look up notebook by global path
                let result = lookup_subdomain_context(&fetcher, &subdomain).await;
                if result.is_none() {
                    tracing::warn!(subdomain, "Notebook lookup failed for subdomain");
                }
                result
            }
        }
    })?;

    #[cfg(feature = "fullstack-server")]
    let ctx = match &*ctx_resource.read() {
        Some(ctx) => ctx.clone(),
        None => {
            return rsx! { div { "Loading..." } };
        }
    };

    #[cfg(not(feature = "fullstack-server"))]
    let ctx = { None::<SubdomainContext> };

    let auth_state = use_signal(|| AuthState::default());
    #[allow(unused)]
    let auth_state = use_context_provider(|| auth_state);

    // Provide link mode for router-agnostic link generation (subdomain mode)
    let sub = use_context_provider(|| {
        if ctx.is_some() {
            LinkMode::Subdomain
        } else {
            LinkMode::MainDomain
        }
    });

    #[cfg(all(target_family = "wasm", target_os = "unknown"))]
    let restore_result = {
        let fetcher = fetcher.clone();
        use_resource(move || {
            let fetcher = fetcher.clone();
            async move { auth::restore_session(fetcher, auth_state).await }
        })
    };
    #[cfg(not(all(target_family = "wasm", target_os = "unknown")))]
    let restore_result: Option<auth::RestoreResult> = None;

    #[cfg(all(target_family = "wasm", target_os = "unknown",))]
    {
        use_effect(move || {
            let fetcher = fetcher.clone();
            spawn(async move {
                use crate::service_worker;

                tracing::info!("Registering service worker");
                let _ = service_worker::register_service_worker().await;
            });
        });
    }

    use_context_provider(|| restore_result);

    if sub == LinkMode::Subdomain {
        tracing::info!("App: rendering SubdomainApp");
        use_context_provider(|| ctx.unwrap());
        rsx! {
             SubdomainApp {}
        }
    } else {
        tracing::info!("App: rendering MainDomainApp");
        rsx! {
            MainDomainApp {}
        }
    }
}

#[component]
pub fn MainDomainApp() -> Element {
    rsx! {
        document::Link { rel: "icon", href: FAVICON }
        // Preconnect for external fonts (before loading them)
        document::Link { rel: "preconnect", href: "https://fonts.googleapis.com" }
        document::Link { rel: "preconnect", href: "https://fonts.gstatic.com" }
        // Theme defaults first: CSS variables, font-faces, reset
        document::Link { rel: "stylesheet", href: THEME_DEFAULTS_CSS }
        document::Link { rel: "stylesheet", href: "https://fonts.googleapis.com/css2?family=IBM+Plex+Mono:ital,wght@0,200;0,300;0,400;0,500;0,600;0,700;1,200;1,300;1,400;1,500;1,600;1,700&family=IBM+Plex+Sans:ital,wght@0,100..700;1,100..700&family=IBM+Plex+Serif:ital,wght@0,200;0,300;0,400;0,500;0,600;0,700;1,200;1,300;1,400;1,500;1,600;1,700&display=swap" }
        // App shell styles (depends on theme variables)
        document::Link { rel: "stylesheet", href: MAIN_CSS }
        components::toast::ToastProvider {
            Router::<Route> {}
        }
    }
}

// And then our Outlet is wrapped in a fallback UI
#[component]
pub fn ErrorLayout() -> Element {
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
            Outlet::<Route> {}
        }
    }
}

#[cfg(all(feature = "fullstack-server", feature = "server"))]
pub async fn favicon() -> axum::response::Response {
    use axum::{http::header::CONTENT_TYPE, response::IntoResponse};
    let favicon_bytes = include_bytes!("../assets/weaver_photo_sm.jpg");

    ([(CONTENT_TYPE, "image/jpg")], favicon_bytes).into_response()
}
