// The dioxus prelude contains a ton of common items used in dioxus apps. It's a good idea to import wherever you
// need dioxus
use components::{EntryPage, Repository, RepositoryIndex};
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
use std::sync::{Arc, LazyLock};
#[allow(unused)]
use views::{
    Callback, Editor, Home, Navbar, Notebook, NotebookIndex, NotebookPage, RecordIndex, RecordPage,
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
mod record_utils;
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
        #[route("/editor?:entry")]
        Editor { entry: Option<String> },
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
            #[nest("/:book_title")]
              #[layout(Notebook)]
              #[route("/")]
              NotebookIndex { ident: AtIdentifier<'static>, book_title: SmolStr },
                #[route("/:title")]
                EntryPage { ident: AtIdentifier<'static>, book_title: SmolStr, title: SmolStr }

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

    // Set up tracing subscriber with both console output and log capture (wasm only)
    // Must happen before dioxus::launch so dioxus skips its own init
    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    {
        use tracing::Level;
        use tracing::subscriber::set_global_default;
        use tracing_subscriber::Registry;
        use tracing_subscriber::layer::SubscriberExt;

        let console_level = if cfg!(debug_assertions) {
            Level::DEBUG
        } else {
            Level::INFO
        };

        let wasm_layer = tracing_wasm::WASMLayer::new(
            tracing_wasm::WASMLayerConfigBuilder::new()
                .set_max_level(console_level)
                .build(),
        );

        let reg = Registry::default()
            .with(wasm_layer)
            .with(components::editor::LogCaptureLayer);

        let _ = set_global_default(reg);
    }

    #[cfg(feature = "server")]
    std::panic::set_hook(Box::new(|panic_info| {
        tracing::error!("PANIC: {:?}", panic_info);
    }));

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
        let router = { axum::Router::new().merge(dioxus::server::router(App)) };

        #[cfg(feature = "fullstack-server")]
        let router = {
            use jacquard::client::UnauthenticatedSession;
            let fetcher = Arc::new(fetch::Fetcher::new(OAuthClient::new(
                AuthStore::new(),
                ClientData::new_public(CONFIG.oauth.clone()),
            )));
            let blob_cache = Arc::new(BlobCache::new(Arc::new(
                UnauthenticatedSession::new_public(),
            )));
            axum::Router::new()
                .route("/favicon.ico", get(favicon))
                // Server side render the application, serve static assets, and register server functions
                .serve_dioxus_application(
                    ServeConfig::builder(), // Enable incremental rendering
                    // .incremental(
                    //     dioxus::server::IncrementalRendererConfig::new()
                    //         .pre_render(true)
                    //         .clear_cache(false),
                    // )
                    //.enable_out_of_order_streaming(),
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
const THEME_DEFAULTS_CSS: Asset = asset!("/assets/styling/theme-defaults.css");

#[component]
fn App() -> Element {
    #[allow(unused)]
    let fetcher = use_context_provider(|| {
        fetch::Fetcher::new(OAuthClient::new(
            AuthStore::new(),
            ClientData::new_public(CONFIG.oauth.clone()),
        ))
    });
    let auth_state = use_signal(|| AuthState::default());
    #[allow(unused)]
    let auth_state = use_context_provider(|| auth_state);
    #[cfg(all(
        target_family = "wasm",
        target_os = "unknown",
        feature = "fullstack-server"
    ))]
    {
        let fetcher = fetcher.clone();
        use_effect(move || {
            let fetcher = fetcher.clone();
            use_future(move || {
                let fetcher = fetcher.clone();
                async move {
                    if let Err(e) = auth::restore_session(fetcher, auth_state).await {
                        tracing::debug!("Session restoration failed: {}", e);
                    }
                }
            });
        });
    }

    #[cfg(all(
        target_family = "wasm",
        target_os = "unknown",
        not(feature = "fullstack-server")
    ))]
    {
        let fetcher = fetcher.clone();
        use_future(move || {
            let fetcher = fetcher.clone();
            async move {
                if let Err(e) = auth::restore_session(fetcher, auth_state).await {
                    tracing::debug!("Session restoration failed: {}", e);
                }
            }
        });
    }

    #[cfg(all(
        target_family = "wasm",
        target_os = "unknown",
        not(feature = "fullstack-server")
    ))]
    {
        use_effect(move || {
            let fetcher = fetcher.clone();
            spawn(async move {
                tracing::info!("Registering service worker");
                let _ = service_worker::register_service_worker().await;
            });
        });
    }

    rsx! {
        document::Link { rel: "icon", href: FAVICON }
        document::Link { rel: "stylesheet", href: MAIN_CSS }
        document::Link { rel: "stylesheet", href: "https://fonts.googleapis.com/css2?family=IBM+Plex+Mono:ital,wght@0,200;0,300;0,400;0,500;0,600;0,700;1,200;1,300;1,400;1,500;1,600;1,700&family=IBM+Plex+Sans:ital,wght@0,100..700;1,100..700&family=IBM+Plex+Serif:ital,wght@0,200;0,300;0,400;0,500;0,600;0,700;1,200;1,300;1,400;1,500;1,600;1,700&display=swap" }
        document::Link { rel: "preconnect", href: "https://fonts.googleapis.com" }
        document::Link { rel: "preconnect", href: "https://fonts.gstatic.com" }

        document::Link { rel: "stylesheet", href: THEME_DEFAULTS_CSS }
        Router::<Route> {}
    }
}

// And then our Outlet is wrapped in a fallback UI
#[component]
fn ErrorLayout() -> Element {
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
    let bytes = include_bytes!("../assets/weaver_photo_sm.jpg");
    ([(CONTENT_TYPE, "image/jpg")], bytes).into_response()
}

#[cfg(all(feature = "fullstack-server", feature = "server"))]
#[get("/{notebook}/image/{name}", blob_cache: Extension<Arc<crate::blobcache::BlobCache>>)]
pub async fn image_named(notebook: SmolStr, name: SmolStr) -> Result<axum::response::Response> {
    use axum::{
        http::header::{CACHE_CONTROL, CONTENT_TYPE},
        response::IntoResponse,
    };
    use mime_sniffer::MimeTypeSniffer;
    if let Some(bytes) = blob_cache.get_named(&name) {
        let blob = bytes.clone();
        let mime = blob.sniff_mime_type().unwrap_or("image/jpg");
        // Blobs are immutable by CID - cache aggressively
        Ok((
            [
                (CONTENT_TYPE, mime),
                (CACHE_CONTROL, "public, max-age=31536000, immutable"),
            ],
            bytes,
        )
            .into_response())
    } else {
        Err(CapturedError::from_display("no image"))
    }
}

#[cfg(all(feature = "fullstack-server", feature = "server"))]
#[get("/{notebook}/blob/{cid}", blob_cache: Extension<Arc<crate::blobcache::BlobCache>>)]
pub async fn blob(notebook: SmolStr, cid: SmolStr) -> Result<axum::response::Response> {
    use axum::{
        http::header::{CACHE_CONTROL, CONTENT_TYPE},
        response::IntoResponse,
    };
    use mime_sniffer::MimeTypeSniffer;
    if let Some(bytes) = blob_cache.get_cid(&Cid::new_owned(cid.as_bytes())?) {
        let blob = bytes.clone();
        let mime = blob.sniff_mime_type().unwrap_or("application/octet-stream");
        // Blobs are immutable by CID - cache aggressively
        Ok((
            [
                (CONTENT_TYPE, mime),
                (CACHE_CONTROL, "public, max-age=31536000, immutable"),
            ],
            bytes,
        )
            .into_response())
    } else {
        Err(CapturedError::from_display("no blob"))
    }
}

// New image routes with unified /image/ prefix
// Route: /image/{notebook}/{name} - notebook entry image by name
#[cfg(all(feature = "fullstack-server", feature = "server"))]
#[get("/image/{notebook}/{name}", blob_cache: Extension<Arc<crate::blobcache::BlobCache>>)]
pub async fn image_notebook(notebook: SmolStr, name: SmolStr) -> Result<axum::response::Response> {
    use axum::{
        http::header::{CACHE_CONTROL, CONTENT_TYPE},
        response::IntoResponse,
    };
    use mime_sniffer::MimeTypeSniffer;
    
    // Try name-based lookup first (backwards compat with cached entries)
    if let Some(bytes) = blob_cache.get_named(&name) {
        let blob = bytes.clone();
        let mime = blob.sniff_mime_type().unwrap_or("image/jpg");
        return Ok((
            [
                (CONTENT_TYPE, mime),
                (CACHE_CONTROL, "public, max-age=31536000, immutable"),
            ],
            bytes,
        )
            .into_response());
    }
    
    // Try to resolve from notebook
    match blob_cache.resolve_from_notebook(&notebook, &name).await {
        Ok(bytes) => {
            let blob = bytes.clone();
            let mime = blob.sniff_mime_type().unwrap_or("image/jpg");
            Ok((
                [
                    (CONTENT_TYPE, mime),
                    (CACHE_CONTROL, "public, max-age=31536000, immutable"),
                ],
                bytes,
            )
                .into_response())
        }
        Err(e) => Err(e),
    }
}

// Route: /image/{ident}/draft/{blob_rkey} - draft image (unpublished)
// Route: /image/{ident}/draft/{blob_rkey}/{name} - draft image with name
#[cfg(all(feature = "fullstack-server", feature = "server"))]
#[get("/image/{ident}/draft/{blob_rkey}", blob_cache: Extension<Arc<crate::blobcache::BlobCache>>)]
pub async fn image_draft(ident: SmolStr, blob_rkey: SmolStr) -> Result<axum::response::Response> {
    use axum::{
        http::header::{CACHE_CONTROL, CONTENT_TYPE},
        response::IntoResponse,
    };
    use mime_sniffer::MimeTypeSniffer;
    
    let at_ident = AtIdentifier::new_owned(ident.clone())
        .map_err(|e| CapturedError::from_display(format!("Invalid identifier '{}': {}", ident, e)))?;
    
    match blob_cache.resolve_from_draft(&at_ident, &blob_rkey).await {
        Ok(bytes) => {
            let blob = bytes.clone();
            let mime = blob.sniff_mime_type().unwrap_or("image/jpg");
            Ok((
                [
                    (CONTENT_TYPE, mime),
                    (CACHE_CONTROL, "public, max-age=31536000, immutable"),
                ],
                bytes,
            )
                .into_response())
        }
        Err(e) => Err(e),
    }
}

#[cfg(all(feature = "fullstack-server", feature = "server"))]
#[get("/image/{ident}/draft/{blob_rkey}/{name}", blob_cache: Extension<Arc<crate::blobcache::BlobCache>>)]
pub async fn image_draft_named(ident: SmolStr, blob_rkey: SmolStr, name: SmolStr) -> Result<axum::response::Response> {
    use axum::{
        http::header::{CACHE_CONTROL, CONTENT_TYPE},
        response::IntoResponse,
    };
    use mime_sniffer::MimeTypeSniffer;
    
    // Name is optional/decorative for drafts, just use the blob_rkey
    let at_ident = AtIdentifier::new_owned(ident.clone())
        .map_err(|e| CapturedError::from_display(format!("Invalid identifier '{}': {}", ident, e)))?;
    
    match blob_cache.resolve_from_draft(&at_ident, &blob_rkey).await {
        Ok(bytes) => {
            let blob = bytes.clone();
            let mime = blob.sniff_mime_type().unwrap_or("image/jpg");
            Ok((
                [
                    (CONTENT_TYPE, mime),
                    (CACHE_CONTROL, "public, max-age=31536000, immutable"),
                ],
                bytes,
            )
                .into_response())
        }
        Err(e) => Err(e),
    }
}

// Route: /image/{ident}/{rkey}/{name} - published entry image
#[cfg(all(feature = "fullstack-server", feature = "server"))]
#[get("/image/{ident}/{rkey}/{name}", blob_cache: Extension<Arc<crate::blobcache::BlobCache>>)]
pub async fn image_entry(ident: SmolStr, rkey: SmolStr, name: SmolStr) -> Result<axum::response::Response> {
    use axum::{
        http::header::{CACHE_CONTROL, CONTENT_TYPE},
        response::IntoResponse,
    };
    use mime_sniffer::MimeTypeSniffer;
    
    let at_ident = AtIdentifier::new_owned(ident.clone())
        .map_err(|e| CapturedError::from_display(format!("Invalid identifier '{}': {}", ident, e)))?;
    
    match blob_cache.resolve_from_entry(&at_ident, &rkey, &name).await {
        Ok(bytes) => {
            let blob = bytes.clone();
            let mime = blob.sniff_mime_type().unwrap_or("image/jpg");
            Ok((
                [
                    (CONTENT_TYPE, mime),
                    (CACHE_CONTROL, "public, max-age=31536000, immutable"),
                ],
                bytes,
            )
                .into_response())
        }
        Err(e) => Err(e),
    }
}

// #[server(endpoint = "static_routes", output = server_fn::codec::Json)]
// async fn static_routes() -> Result<Vec<String>, ServerFnError> {
//     // The `Routable` trait has a `static_routes` method that returns all static routes in the enum
//     Ok(Route::static_routes()
//         .iter()
//         .map(ToString::to_string)
//         .collect())
// }
