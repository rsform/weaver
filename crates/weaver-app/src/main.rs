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

/// Extract subdomain from host if it matches base domain pattern.
#[cfg(feature = "server")]
fn extract_subdomain<'a>(host: &'a str, base: &str) -> Option<&'a str> {
    let suffix = format!(".{}", base);
    if host.ends_with(&suffix) && host.len() > suffix.len() {
        Some(&host[..host.len() - suffix.len()])
    } else {
        None
    }
}

/// Look up notebook by global path.
///
/// Returns SubdomainContext if a notebook with publishGlobal=true exists for this path.
#[cfg(feature = "server")]
async fn lookup_global_notebook(
    fetcher: &Arc<fetch::Fetcher>,
    path: &str,
) -> Option<SubdomainContext> {
    use jacquard::IntoStatic;
    use jacquard::smol_str::SmolStr;
    use jacquard::smol_str::ToSmolStr;
    use jacquard::types::string::Did;
    use jacquard::xrpc::XrpcClient;
    use weaver_api::sh_weaver::notebook::resolve_global_notebook::ResolveGlobalNotebook;

    let request = ResolveGlobalNotebook::new().path(path).build();

    match fetcher.send(request).await {
        Ok(response) => {
            let output = response.into_output().ok()?;
            let notebook = output.notebook;

            let owner = notebook.uri.authority().clone().into_static();
            let rkey = notebook.uri.rkey()?.0.to_smolstr();
            let notebook_path = notebook
                .path
                .map(|p| SmolStr::new(p.as_ref()))
                .unwrap_or_else(|| SmolStr::new(path));

            Some(SubdomainContext {
                owner,
                notebook_path,
                notebook_rkey: rkey,
                notebook_title: notebook.title.clone().unwrap_or_default().to_smolstr(),
            })
        }
        Err(e) => {
            tracing::debug!(path = path, error = %e, "Global notebook lookup failed");
            None
        }
    }
}
