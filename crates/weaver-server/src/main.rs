// The dioxus prelude contains a ton of common items used in dioxus apps. It's a good idea to import wherever you
// need dioxus
use components::{Entry, Repository, RepositoryIndex};
#[allow(unused)]
use dioxus::{
    fullstack::{response::Extension, FullstackContext},
    prelude::*,
    CapturedError,
};
#[allow(unused)]
use jacquard::{
    client::BasicClient,
    smol_str::SmolStr,
    types::{cid::Cid, string::AtIdentifier},
};

use std::sync::Arc;
#[allow(unused)]
use views::{Home, Navbar, Notebook, NotebookIndex, NotebookPage};

#[cfg(feature = "server")]
mod blobcache;
mod cache_impl;
/// Define a components module that contains all shared components for our app.
mod components;
mod fetch;
/// Define a views module that contains the UI for all Layouts and Routes for our app.
mod views;

/// The Route enum is used to define the structure of internal routes in our app. All route enums need to derive
/// the [`Routable`] trait, which provides the necessary methods for the router to work.
///
/// Each variant represents a different URL pattern that can be matched by the router. If that pattern is matched,
/// the components for that route will be rendered.
#[derive(Debug, Clone, Routable, PartialEq)]
#[rustfmt::skip]
enum Route {
    // The layout attribute defines a wrapper for all routes under the layout. Layouts are great for wrapping
    // many routes with a common UI like a navbar.
    #[layout(Navbar)]
        // The route attribute defines the URL pattern that a specific route matches. If that pattern matches the URL,
        // the component for that route will be rendered. The component name that is rendered defaults to the variant name.
        #[route("/")]
        Home {},
        #[layout(ErrorLayout)]
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

// We can import assets in dioxus with the `asset!` macro. This macro takes a path to an asset relative to the crate root.
// The macro returns an `Asset` type that will display as the path to the asset in the browser or a local path in desktop bundles.
const FAVICON: Asset = asset!("/assets/weaver_photo_sm.jpg");
// The asset macro also minifies some assets like CSS and JS to make bundled smaller
const MAIN_CSS: Asset = asset!("/assets/styling/main.css");

fn main() {
    // Set up better panic messages for wasm
    #[cfg(target_arch = "wasm32")]
    console_error_panic_hook::set_once();

    // Run `serve()` on the server only
    #[cfg(feature = "server")]
    dioxus::serve(|| async move {
        use crate::blobcache::BlobCache;
        use crate::fetch::CachedFetcher;
        use axum::{
            extract::{Extension, Request},
            middleware,
            middleware::Next,
        };
        use std::convert::Infallible;
        use std::sync::Arc;

        // Create shared state
        let fetcher = Arc::new(CachedFetcher::new(Arc::new(BasicClient::unauthenticated())));
        let blob_cache = Arc::new(BlobCache::new(Arc::new(BasicClient::unauthenticated())));

        // Create a new router for our app using the `router` function
        let router = dioxus::server::router(App).layer(middleware::from_fn({
            let fetcher = fetcher.clone();
            let blob_cache = blob_cache.clone();
            move |mut req: Request, next: Next| {
                let fetcher = fetcher.clone();
                let blob_cache = blob_cache.clone();
                async move {
                    // Attach extensions for dioxus server functions
                    req.extensions_mut().insert(fetcher);
                    req.extensions_mut().insert(blob_cache);

                    // And then return the response with `next.run()
                    Ok::<_, Infallible>(next.run(req).await)
                }
            }
        }));
        // And then return the router
        Ok(router)
    });

    // When not on the server, just run `launch()` like normal
    #[cfg(not(feature = "server"))]
    dioxus::launch(App);
}

/// App is the main component of our app. Components are the building blocks of dioxus apps. Each component is a function
/// that takes some props and returns an Element. In this case, App takes no props because it is the root of our app.
///
/// Components should be annotated with `#[component]` to support props, better error messages, and autocomplete
#[component]
fn App() -> Element {
    // The `rsx!` macro lets us define HTML inside of rust. It expands to an Element with all of our HTML inside.
    use_context_provider(|| fetch::CachedFetcher::new(Arc::new(BasicClient::unauthenticated())));
    rsx! {
        document::Link { rel: "icon", href: FAVICON }
        document::Link { rel: "stylesheet", href: MAIN_CSS }
        Router::<Route> {}
    }
}

// And then our Outlet is wrapped in a fallback UI
#[component]
fn ErrorLayout() -> Element {
    rsx! {
        ErrorBoundary {
            handle_error: move |err: ErrorContext| {
                let http_error = FullstackContext::commit_error_status(err.error().unwrap());
                match http_error.status {
                    StatusCode::NOT_FOUND => rsx! { div { "404 - Page not found" } },
                    _ => rsx! { div { "An unknown error occurred" } },
                }
            },
            Outlet::<Route> {}
        }
    }
}

#[get("/{notebook}/image/{name}", blob_cache: Extension<Arc<crate::blobcache::BlobCache>>)]
pub async fn image_named(
    notebook: SmolStr,
    name: SmolStr,
) -> Result<dioxus_fullstack::response::Response> {
    use axum::response::IntoResponse;
    use mime_sniffer::MimeTypeSniffer;
    if let Some(bytes) = blob_cache.get_named(&name) {
        let blob = bytes.clone();
        let mime = blob.sniff_mime_type().unwrap_or("application/octet-stream");
        Ok((
            [(dioxus_fullstack::http::header::CONTENT_TYPE, mime)],
            bytes,
        )
            .into_response())
    } else {
        Err(CapturedError::from_display("no image"))
    }
}

#[get("/{notebook}/blob/{cid}", blob_cache: Extension<Arc<crate::blobcache::BlobCache>>)]
pub async fn blob(notebook: SmolStr, cid: SmolStr) -> Result<dioxus_fullstack::response::Response> {
    use axum::response::IntoResponse;
    use mime_sniffer::MimeTypeSniffer;
    if let Some(bytes) = blob_cache.get_cid(&Cid::new_owned(cid.as_bytes())?) {
        let blob = bytes.clone();
        let mime = blob.sniff_mime_type().unwrap_or("application/octet-stream");
        Ok((
            [(dioxus_fullstack::http::header::CONTENT_TYPE, mime)],
            bytes,
        )
            .into_response())
    } else {
        Err(CapturedError::from_display("no blob"))
    }
}
