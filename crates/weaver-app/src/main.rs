//! Weaver App main binary.

#[allow(unused)]
use dioxus::prelude::*;
#[cfg(target_arch = "wasm32")]
use lol_alloc::{FreeListAllocator, LockedAllocator};
#[cfg(feature = "server")]
use std::sync::Arc;
#[cfg(feature = "server")]
use tower::Service;
#[allow(unused)]
use weaver_app::{App, CONFIG, SubdomainApp, SubdomainContext, fetch};
#[cfg(target_arch = "wasm32")]
#[global_allocator]
static ALLOCATOR: LockedAllocator<FreeListAllocator> =
    LockedAllocator::new(FreeListAllocator::new());

/// Base domain for subdomain extraction.
#[cfg(feature = "server")]
const BASE_DOMAIN: &str = weaver_app::env::WEAVER_APP_DOMAIN;

/// Reserved subdomains that should not be used for notebooks.
#[cfg(feature = "server")]
const RESERVED_SUBDOMAINS: &[&str] = &[
    "www", "api", "admin", "app", "auth", "cdn", "alpha", "beta", "staging", "index",
];

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
        use tracing_subscriber::filter::EnvFilter;
        use tracing_subscriber::layer::SubscriberExt;

        let console_level = if cfg!(debug_assertions) {
            Level::TRACE
        } else {
            Level::INFO
        };

        let wasm_layer = tracing_wasm::WASMLayer::new(
            tracing_wasm::WASMLayerConfigBuilder::new()
                .set_max_level(console_level)
                .build(),
        );

        // Filter out noisy crates
        // Use weaver_app=trace for detailed editor debugging
        let filter = EnvFilter::new(
            "debug,weaver_app=trace,loro_internal=warn,jacquard_identity=info,jacquard_common=info,iroh=info",
        );

        let reg = Registry::default()
            .with(filter)
            .with(wasm_layer)
            .with(weaver_app::components::editor::LogCaptureLayer);

        let _ = set_global_default(reg);
    }

    #[cfg(feature = "server")]
    std::panic::set_hook(Box::new(|panic_info| {
        tracing::error!("PANIC: {:?}", panic_info);
    }));

    // Run `serve()` on the server only
    #[cfg(feature = "server")]
    dioxus::serve(|| async move {
        #[cfg(feature = "fullstack-server")]
        use axum::middleware;
        use axum::middleware::Next;
        use axum::{Router, body::Body, extract::Request, response::Response, routing::get};
        use axum_extra::extract::Host;
        use jacquard::oauth::{client::OAuthClient, session::ClientData};
        use std::convert::Infallible;
        use weaver_app::auth::AuthStore;
        use weaver_app::blobcache::BlobCache;

        #[cfg(not(feature = "fullstack-server"))]
        let router = { Router::new().merge(dioxus::server::router(App)) };

        #[cfg(feature = "fullstack-server")]
        let router = {
            let fetcher = Arc::new(fetch::Fetcher::new(OAuthClient::new(
                AuthStore::new(),
                ClientData::new_public(CONFIG.oauth.clone()),
            )));

            let blob_cache = Arc::new(BlobCache::new(fetcher.clone()));
            axum::Router::new()
                .route("/favicon.ico", get(weaver_app::favicon))
                .serve_dioxus_application(ServeConfig::builder(), App)
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
