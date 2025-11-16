// The dioxus prelude contains a ton of common items used in dioxus apps. It's a good idea to import wherever you
// need dioxus
use components::{Entry, Repository, RepositoryIndex};
#[allow(unused)]
use dioxus::{CapturedError, prelude::*};

#[cfg(feature = "fullstack-server")]
use dioxus::fullstack::FullstackContext;
#[cfg(all(feature = "fullstack-server", feature = "server"))]
use dioxus::fullstack::response::Extension;
use jacquard::oauth::{client::OAuthClient, session::ClientData};
#[allow(unused)]
use jacquard::{
    smol_str::SmolStr,
    types::{cid::Cid, string::AtIdentifier},
};
#[cfg(feature = "server")]
use std::sync::Arc;
use std::sync::LazyLock;
#[allow(unused)]
use views::{
    Callback, Home, Navbar, Notebook, NotebookIndex, NotebookPage, RecordIndex, RecordView,
};

use crate::{
    auth::{AuthState, AuthStore},
    config::{Config, OAuthConfig},
};

mod auth;
#[cfg(feature = "server")]
mod blobcache;
mod cache_impl;
/// Define a components module that contains all shared components for our app.
mod components;
mod config;
mod data;
mod env;
mod fetch;
mod service_worker;
/// Define a views module that contains the UI for all Layouts and Routes for our app.
mod views;

#[cfg(target_arch = "wasm32")]
use lol_alloc::{FreeListAllocator, LockedAllocator};

#[cfg(target_arch = "wasm32")]
#[global_allocator]
static ALLOCATOR: LockedAllocator<FreeListAllocator> =
    LockedAllocator::new(FreeListAllocator::new());

#[derive(Debug, Clone, Routable, PartialEq)]
#[rustfmt::skip]
enum Route {
    #[layout(Navbar)]
        #[route("/")]
        Home {},
        #[layout(ErrorLayout)]
        #[nest("/record")]
          #[layout(RecordIndex)]
            #[route("/:..uri")]
            RecordView { uri: Vec<String> },
                     #[end_layout]
        #[end_nest]
        #[route("/callback?:state&:iss&:code")]
        Callback { state: SmolStr, iss: SmolStr, code: SmolStr },
        #[nest("/:ident")]
          #[layout(Repository)]
            #[route("/")]
            RepositoryIndex { ident: AtIdentifier<'static> },
            #[nest("/:book_title")]
              #[layout(Notebook)]
              #[route("/")]
              NotebookIndex { ident: AtIdentifier<'static>, book_title: SmolStr },
                #[route("/:title")]
                Entry { ident: AtIdentifier<'static>, book_title: SmolStr, title: SmolStr }

}
const FAVICON: Asset = asset!("/assets/weaver_photo_sm.jpg");
const MAIN_CSS: Asset = asset!("/assets/styling/main.css");

#[cfg(not(feature = "fullstack-server"))]
#[cfg(feature = "server")]
async fn serve_sw() -> impl axum::response::IntoResponse {
    use axum::response::IntoResponse;
    let sw_js = include_str!("../assets/sw.js");
    (
        [(axum::http::header::CONTENT_TYPE, "application/javascript")],
        sw_js,
    )
        .into_response()
}

pub static CONFIG: LazyLock<Config> = LazyLock::new(|| Config {
    oauth: OAuthConfig::from_env().as_metadata(),
});
fn main() {
    // Set up better panic messages for wasm
    #[cfg(target_arch = "wasm32")]
    console_error_panic_hook::set_once();

    // Run `serve()` on the server only
    #[cfg(feature = "server")]
    dioxus::serve(|| async move {
        use crate::blobcache::BlobCache;
        use axum::{
            extract::{Extension, Request},
            middleware,
            middleware::Next,
            routing::get,
        };
        use std::convert::Infallible;
        use std::sync::Arc;

        #[cfg(not(feature = "fullstack-server"))]
        let router = {
            axum::Router::new()
                .route("/sw.js", get(serve_sw))
                .merge(dioxus::server::router(App))
        };

        #[cfg(feature = "fullstack-server")]
        let router = {
            use jacquard::client::UnauthenticatedSession;
            let fetcher = Arc::new(fetch::CachedFetcher::new(OAuthClient::new(
                AuthStore::new(),
                ClientData::new_public(CONFIG.oauth.clone()),
            )));
            let blob_cache = Arc::new(BlobCache::new(Arc::new(
                UnauthenticatedSession::new_public(),
            )));
            axum::Router::new()
                // Server side render the application, serve static assets, and register server functions
                .serve_dioxus_application(
                    ServeConfig::builder()
                        // Enable incremental rendering
                        .incremental(
                            dioxus::server::IncrementalRendererConfig::new()
                                .static_dir(
                                    std::env::current_exe()
                                        .unwrap()
                                        .parent()
                                        .unwrap()
                                        .join("public"),
                                )
                                .clear_cache(false),
                        )
                        .enable_out_of_order_streaming(),
                    App,
                )
                .layer(middleware::from_fn({
                    let blob_cache = blob_cache.clone();
                    let fetcher = fetcher.clone();
                    move |mut req: Request, next: Next| {
                        let blob_cache = blob_cache.clone();
                        let fetcher = fetcher.clone();
                        async move {
                            req.extensions_mut().insert(blob_cache);
                            req.extensions_mut().insert(fetcher);
                            Ok::<_, Infallible>(next.run(req).await)
                        }
                    }
                }))
        };
        Ok(router)
    });

    #[cfg(not(feature = "server"))]
    dioxus::launch(App);
}

#[component]
fn App() -> Element {
    use_context_provider(|| {
        fetch::CachedFetcher::new(OAuthClient::new(
            AuthStore::new(),
            ClientData::new_public(CONFIG.oauth.clone()),
        ))
    });
    use_context_provider(|| Signal::new(AuthState::default()));

    use_effect(move || {
        spawn(async move {
            if let Err(e) = auth::restore_session().await {
                tracing::warn!("Session restoration failed: {}", e);
            }
        });
    });

    #[cfg(all(
        target_family = "wasm",
        target_os = "unknown",
        not(feature = "fullstack-server")
    ))]
    use_effect(move || {
        spawn(async move {
            let _ = service_worker::register_service_worker().await;
        });
    });

    rsx! {
        document::Link { rel: "icon", href: FAVICON }
        document::Link { rel: "stylesheet", href: MAIN_CSS }
        document::Link { rel: "stylesheet", href: "https://fonts.googleapis.com/css2?family=IBM+Plex+Mono:ital,wght@0,200;0,300;0,400;0,500;0,600;0,700;1,200;1,300;1,400;1,500;1,600;1,700&family=IBM+Plex+Sans:ital,wght@0,100..700;1,100..700&family=IBM+Plex+Serif:ital,wght@0,200;0,300;0,400;0,500;0,600;0,700;1,200;1,300;1,400;1,500;1,600;1,700&display=swap" }
        document::Link { rel: "preconnect", href: "https://fonts.googleapis.com" }
        document::Link { rel: "preconnect", href: "https://fonts.gstatic.com" }
        Router::<Route> {}
    }
}

// And then our Outlet is wrapped in a fallback UI
#[component]
fn ErrorLayout() -> Element {
    rsx! {
        ErrorBoundary {
            handle_error: move |err: ErrorContext| {
                #[cfg(feature = "fullstack-server")]
                {
                    let http_error = FullstackContext::commit_error_status(err.error().unwrap());
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
#[get("/{notebook}/image/{name}", blob_cache: Extension<Arc<crate::blobcache::BlobCache>>)]
pub async fn image_named(notebook: SmolStr, name: SmolStr) -> Result<axum::response::Response> {
    use axum::{http::header::CONTENT_TYPE, response::IntoResponse};
    use mime_sniffer::MimeTypeSniffer;
    if let Some(bytes) = blob_cache.get_named(&name) {
        let blob = bytes.clone();
        let mime = blob.sniff_mime_type().unwrap_or("image/jpg");
        Ok(([(CONTENT_TYPE, mime)], bytes).into_response())
    } else {
        Err(CapturedError::from_display("no image"))
    }
}

#[cfg(all(feature = "fullstack-server", feature = "server"))]
#[get("/{notebook}/blob/{cid}", blob_cache: Extension<Arc<crate::blobcache::BlobCache>>)]
pub async fn blob(notebook: SmolStr, cid: SmolStr) -> Result<axum::response::Response> {
    use axum::{http::header::CONTENT_TYPE, response::IntoResponse};
    use mime_sniffer::MimeTypeSniffer;
    if let Some(bytes) = blob_cache.get_cid(&Cid::new_owned(cid.as_bytes())?) {
        let blob = bytes.clone();
        let mime = blob.sniff_mime_type().unwrap_or("application/octet-stream");
        Ok(([(CONTENT_TYPE, mime)], bytes).into_response())
    } else {
        Err(CapturedError::from_display("no blob"))
    }
}

#[server(endpoint = "static_routes", output = server_fn::codec::Json)]
async fn static_routes() -> Result<Vec<String>, ServerFnError> {
    // The `Routable` trait has a `static_routes` method that returns all static routes in the enum
    Ok(Route::static_routes()
        .iter()
        .map(ToString::to_string)
        .collect())
}
