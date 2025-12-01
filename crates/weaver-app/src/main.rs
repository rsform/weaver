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
    Callback, DraftEdit, DraftsList, Editor, Home, Navbar, NewDraft, Notebook, NotebookEntryByRkey,
    NotebookEntryEdit, NotebookIndex, NotebookPage, RecordIndex, RecordPage, StandaloneEntry,
    StandaloneEntryEdit,
};

use crate::{
    auth::{AuthState, AuthStore},
    config::{Config, OAuthConfig},
};

mod auth;
#[cfg(feature = "server")]
mod blobcache;
mod cache_impl;
#[cfg(feature = "server")]
mod og;
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
            // Drafts routes (before /:book_title to avoid capture)
            #[route("/drafts")]
            DraftsList { ident: AtIdentifier<'static> },
            #[route("/drafts/:tid")]
            DraftEdit { ident: AtIdentifier<'static>, tid: SmolStr },
            #[route("/new?:notebook")]
            NewDraft { ident: AtIdentifier<'static>, notebook: Option<SmolStr> },
            // Standalone entry routes
            #[route("/e/:rkey")]
            StandaloneEntry { ident: AtIdentifier<'static>, rkey: SmolStr },
            #[route("/e/:rkey/edit")]
            StandaloneEntryEdit { ident: AtIdentifier<'static>, rkey: SmolStr },
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
                NotebookEntryEdit { ident: AtIdentifier<'static>, book_title: SmolStr, rkey: SmolStr }

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
            Level::DEBUG
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

    #[cfg(all(target_family = "wasm", target_os = "unknown"))]
    use_context_provider(|| restore_result);

    rsx! {
        document::Link { rel: "icon", href: FAVICON }
        document::Link { rel: "stylesheet", href: MAIN_CSS }
        document::Link { rel: "stylesheet", href: "https://fonts.googleapis.com/css2?family=IBM+Plex+Mono:ital,wght@0,200;0,300;0,400;0,500;0,600;0,700;1,200;1,300;1,400;1,500;1,600;1,700&family=IBM+Plex+Sans:ital,wght@0,100..700;1,100..700&family=IBM+Plex+Serif:ital,wght@0,200;0,300;0,400;0,500;0,600;0,700;1,200;1,300;1,400;1,500;1,600;1,700&display=swap" }
        document::Link { rel: "preconnect", href: "https://fonts.googleapis.com" }
        document::Link { rel: "preconnect", href: "https://fonts.gstatic.com" }

        document::Link { rel: "stylesheet", href: THEME_DEFAULTS_CSS }
        components::toast::ToastProvider {
            Router::<Route> {}
        }
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

/// Build an image response with appropriate headers for immutable blobs.
#[cfg(all(feature = "fullstack-server", feature = "server"))]
fn build_image_response(bytes: jacquard::bytes::Bytes) -> axum::response::Response {
    use axum::{
        http::header::{CACHE_CONTROL, CONTENT_TYPE},
        response::IntoResponse,
    };
    use mime_sniffer::MimeTypeSniffer;

    let mime = bytes.sniff_mime_type().unwrap_or("image/jpg").to_string();
    (
        [
            (CONTENT_TYPE, mime),
            (
                CACHE_CONTROL,
                "public, max-age=31536000, immutable".to_string(),
            ),
        ],
        bytes,
    )
        .into_response()
}

/// Return a 404 response for missing images.
#[cfg(all(feature = "fullstack-server", feature = "server"))]
fn image_not_found() -> axum::response::Response {
    use axum::{http::StatusCode, response::IntoResponse};
    (StatusCode::NOT_FOUND, "Image not found").into_response()
}

#[cfg(all(feature = "fullstack-server", feature = "server"))]
#[get("/{_notebook}/image/{name}", blob_cache: Extension<Arc<crate::blobcache::BlobCache>>)]
pub async fn image_named(_notebook: SmolStr, name: SmolStr) -> Result<axum::response::Response> {
    if let Some(bytes) = blob_cache.get_named(&name) {
        Ok(build_image_response(bytes))
    } else {
        Ok(image_not_found())
    }
}

#[cfg(all(feature = "fullstack-server", feature = "server"))]
#[get("/{_notebook}/blob/{cid}", blob_cache: Extension<Arc<crate::blobcache::BlobCache>>)]
pub async fn blob(_notebook: SmolStr, cid: SmolStr) -> Result<axum::response::Response> {
    match Cid::new_owned(cid.as_bytes()) {
        Ok(cid) => {
            if let Some(bytes) = blob_cache.get_cid(&cid) {
                Ok(build_image_response(bytes))
            } else {
                Ok(image_not_found())
            }
        }
        Err(_) => Ok(image_not_found()),
    }
}

// Route: /image/{notebook}/{name} - notebook entry image by name
#[cfg(all(feature = "fullstack-server", feature = "server"))]
#[get("/image/{notebook}/{name}", blob_cache: Extension<Arc<crate::blobcache::BlobCache>>)]
pub async fn image_notebook(notebook: SmolStr, name: SmolStr) -> Result<axum::response::Response> {
    // Try name-based lookup first (backwards compat with cached entries)
    if let Some(bytes) = blob_cache.get_named(&name) {
        return Ok(build_image_response(bytes));
    }

    // Try to resolve from notebook
    match blob_cache.resolve_from_notebook(&notebook, &name).await {
        Ok(bytes) => Ok(build_image_response(bytes)),
        Err(_) => Ok(image_not_found()),
    }
}

// Route: /image/{ident}/draft/{blob_rkey} - draft image (unpublished)
#[cfg(all(feature = "fullstack-server", feature = "server"))]
#[get("/image/{ident}/draft/{blob_rkey}", blob_cache: Extension<Arc<crate::blobcache::BlobCache>>)]
pub async fn image_draft(ident: SmolStr, blob_rkey: SmolStr) -> Result<axum::response::Response> {
    let Ok(at_ident) = AtIdentifier::new_owned(ident.clone()) else {
        return Ok(image_not_found());
    };

    match blob_cache.resolve_from_draft(&at_ident, &blob_rkey).await {
        Ok(bytes) => Ok(build_image_response(bytes)),
        Err(_) => Ok(image_not_found()),
    }
}

// Route: /image/{ident}/draft/{blob_rkey}/{name} - draft image with name (name is decorative)
#[cfg(all(feature = "fullstack-server", feature = "server"))]
#[get("/image/{ident}/draft/{blob_rkey}/{_name}", blob_cache: Extension<Arc<crate::blobcache::BlobCache>>)]
pub async fn image_draft_named(
    ident: SmolStr,
    blob_rkey: SmolStr,
    _name: SmolStr,
) -> Result<axum::response::Response> {
    let Ok(at_ident) = AtIdentifier::new_owned(ident.clone()) else {
        return Ok(image_not_found());
    };

    match blob_cache.resolve_from_draft(&at_ident, &blob_rkey).await {
        Ok(bytes) => Ok(build_image_response(bytes)),
        Err(_) => Ok(image_not_found()),
    }
}

// Route: /image/{ident}/{rkey}/{name} - published entry image
#[cfg(all(feature = "fullstack-server", feature = "server"))]
#[get("/image/{ident}/{rkey}/{name}", blob_cache: Extension<Arc<crate::blobcache::BlobCache>>)]
pub async fn image_entry(
    ident: SmolStr,
    rkey: SmolStr,
    name: SmolStr,
) -> Result<axum::response::Response> {
    let Ok(at_ident) = AtIdentifier::new_owned(ident.clone()) else {
        return Ok(image_not_found());
    };

    match blob_cache.resolve_from_entry(&at_ident, &rkey, &name).await {
        Ok(bytes) => Ok(build_image_response(bytes)),
        Err(_) => Ok(image_not_found()),
    }
}

// Route: /og/{ident}/{book_title}/{entry_title} - OpenGraph image for entry
#[cfg(all(feature = "fullstack-server", feature = "server"))]
#[get("/og/{ident}/{book_title}/{entry_title}", fetcher: Extension<Arc<fetch::Fetcher>>)]
pub async fn og_image(
    ident: SmolStr,
    book_title: SmolStr,
    entry_title: SmolStr,
) -> Result<axum::response::Response> {
    use axum::{
        http::{header::{CACHE_CONTROL, CONTENT_TYPE}, StatusCode},
        response::IntoResponse,
    };
    use weaver_api::sh_weaver::actor::ProfileDataViewInner;
    use weaver_api::sh_weaver::notebook::Title;

    // Strip .png extension if present
    let entry_title = entry_title.strip_suffix(".png").unwrap_or(&entry_title);

    let Ok(at_ident) = AtIdentifier::new_owned(ident.clone()) else {
        return Ok((StatusCode::BAD_REQUEST, "Invalid identifier").into_response());
    };

    // Fetch entry data
    let entry_result = fetcher.get_entry(at_ident.clone(), book_title.clone(), entry_title.into()).await;

    let arc_data = match entry_result {
        Ok(Some(data)) => data,
        Ok(None) => return Ok((StatusCode::NOT_FOUND, "Entry not found").into_response()),
        Err(e) => {
            tracing::error!("Failed to fetch entry for OG image: {:?}", e);
            return Ok((StatusCode::INTERNAL_SERVER_ERROR, "Failed to fetch entry").into_response());
        }
    };
    let (book_entry, entry) = arc_data.as_ref();

    // Build cache key using entry CID
    let entry_cid = book_entry.entry.cid.as_ref();
    let cache_key = og::cache_key(&ident, &book_title, entry_title, entry_cid);

    // Check cache first
    if let Some(cached) = og::get_cached(&cache_key) {
        return Ok((
            [
                (CONTENT_TYPE, "image/png"),
                (CACHE_CONTROL, "public, max-age=3600"),
            ],
            cached,
        ).into_response());
    }

    // Extract metadata
    let title: &str = entry.title.as_ref();

    // Use book_title from URL - it's the notebook slug/title
    // TODO: Could fetch actual notebook record to get display title
    let notebook_title_str: String = book_title.to_string();

    let author_handle = book_entry.entry.authors.first()
        .map(|a| match &a.record.inner {
            ProfileDataViewInner::ProfileView(p) => p.handle.as_ref().to_string(),
            ProfileDataViewInner::ProfileViewDetailed(p) => p.handle.as_ref().to_string(),
            ProfileDataViewInner::TangledProfileView(p) => p.handle.as_ref().to_string(),
            _ => "unknown".to_string(),
        })
        .unwrap_or_else(|| "unknown".to_string());

    // Check for hero image in embeds
    let hero_image_data = if let Some(ref embeds) = entry.embeds {
        if let Some(ref images) = embeds.images {
            if let Some(first_image) = images.images.first() {
                // Get DID from the entry URI
                let did = book_entry.entry.uri.authority();

                let blob = first_image.image.blob();
                let cid = blob.cid();
                let mime = blob.mime_type.as_ref();
                let format = mime.strip_prefix("image/").unwrap_or("jpeg");

                // Build CDN URL
                let cdn_url = format!(
                    "https://cdn.bsky.app/img/feed_fullsize/plain/{}/{}@{}",
                    did.as_str(), cid.as_ref(), format
                );

                // Fetch the image
                match reqwest::get(&cdn_url).await {
                    Ok(response) if response.status().is_success() => {
                        match response.bytes().await {
                            Ok(bytes) => {
                                use base64::Engine;
                                let base64_str = base64::engine::general_purpose::STANDARD.encode(&bytes);
                                Some(format!("data:{};base64,{}", mime, base64_str))
                            }
                            Err(_) => None
                        }
                    }
                    _ => None
                }
            } else {
                None
            }
        } else {
            None
        }
    } else {
        None
    };

    // Extract content snippet - render markdown to HTML then strip tags
    let content_snippet: String = {
        let parser = markdown_weaver::Parser::new(entry.content.as_ref());
        let mut html = String::new();
        markdown_weaver::html::push_html(&mut html, parser);
        // Strip HTML tags
        regex_lite::Regex::new(r"<[^>]+>")
            .unwrap()
            .replace_all(&html, "")
            .replace("&amp;", "&")
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&quot;", "\"")
            .replace("&#39;", "'")
            .replace('\n', " ")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    };

    // Generate image - hero or text-only based on available data
    let png_bytes = if let Some(ref hero_data) = hero_image_data {
        match og::generate_hero_image(hero_data, title, &notebook_title_str, &author_handle) {
            Ok(bytes) => bytes,
            Err(e) => {
                tracing::error!("Failed to generate hero OG image: {:?}, falling back to text", e);
                og::generate_text_only(title, &content_snippet, &notebook_title_str, &author_handle)
                    .map_err(|e| {
                        tracing::error!("Failed to generate text OG image: {:?}", e);
                    })
                    .ok()
                    .unwrap_or_default()
            }
        }
    } else {
        match og::generate_text_only(title, &content_snippet, &notebook_title_str, &author_handle) {
            Ok(bytes) => bytes,
            Err(e) => {
                tracing::error!("Failed to generate OG image: {:?}", e);
                return Ok((StatusCode::INTERNAL_SERVER_ERROR, "Failed to generate image").into_response());
            }
        }
    };

    // Cache the generated image
    og::cache_image(cache_key, png_bytes.clone());

    Ok((
        [
            (CONTENT_TYPE, "image/png"),
            (CACHE_CONTROL, "public, max-age=3600"),
        ],
        png_bytes,
    ).into_response())
}

// #[server(endpoint = "static_routes", output = server_fn::codec::Json)]
// async fn static_routes() -> Result<Vec<String>, ServerFnError> {
//     // The `Routable` trait has a `static_routes` method that returns all static routes in the enum
//     Ok(Route::static_routes()
//         .iter()
//         .map(ToString::to_string)
//         .collect())
// }
