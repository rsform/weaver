//! Web worker for fetching and caching AT Protocol embeds.
//!
//! This crate provides a web worker that fetches and renders AT Protocol
//! record embeds off the main thread, with TTL-based caching.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Input messages to the embed worker.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum EmbedWorkerInput {
    /// Request embeds for a list of AT URIs.
    /// Worker returns cached results immediately and fetches missing ones.
    FetchEmbeds {
        /// AT URIs to fetch (e.g., "at://did:plc:xxx/app.bsky.feed.post/yyy")
        uris: Vec<String>,
    },
    /// Clear the cache (e.g., on session change).
    ClearCache,
}

/// Output messages from the embed worker.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum EmbedWorkerOutput {
    /// Embed results (may be partial if some failed).
    Embeds {
        /// Successfully fetched/cached embeds: uri -> rendered HTML.
        results: HashMap<String, String>,
        /// URIs that failed to fetch.
        errors: HashMap<String, String>,
        /// Timing info in milliseconds.
        fetch_ms: f64,
    },
    /// Cache was cleared.
    CacheCleared,
}

#[cfg(all(target_family = "wasm", target_os = "unknown"))]
mod worker_impl {
    use super::*;
    use gloo_worker::{HandlerId, Worker, WorkerScope};
    use jacquard::IntoStatic;
    use jacquard::client::UnauthenticatedSession;
    use jacquard::identity::JacquardResolver;
    use jacquard::prelude::*;
    use jacquard::types::string::AtUri;
    use std::time::Duration;
    use weaver_common::cache;

    /// Embed worker with persistent cache.
    pub struct EmbedWorker {
        /// Cached rendered embeds with TTL and max capacity.
        cache: cache::Cache<AtUri<'static>, String>,
        /// Unauthenticated session for public API calls.
        session: UnauthenticatedSession<JacquardResolver>,
    }

    impl Worker for EmbedWorker {
        type Message = ();
        type Input = EmbedWorkerInput;
        type Output = EmbedWorkerOutput;

        fn create(_scope: &WorkerScope<Self>) -> Self {
            Self {
                // Cache up to 500 embeds, TTL of 1 hour.
                cache: cache::new_cache(500, Duration::from_secs(3600)),
                session: UnauthenticatedSession::default(),
            }
        }

        fn update(&mut self, _scope: &WorkerScope<Self>, _msg: Self::Message) {}

        fn received(&mut self, scope: &WorkerScope<Self>, msg: Self::Input, id: HandlerId) {
            match msg {
                EmbedWorkerInput::FetchEmbeds { uris } => {
                    let mut results = HashMap::new();
                    let mut errors = HashMap::new();
                    let mut to_fetch = Vec::new();

                    // Parse URIs and check cache.
                    for uri_str in uris {
                        let at_uri = match AtUri::new_owned(uri_str.clone()) {
                            Ok(u) => u,
                            Err(e) => {
                                errors.insert(uri_str, format!("Invalid AT URI: {e}"));
                                continue;
                            }
                        };

                        if let Some(html) = cache::get(&self.cache, &at_uri) {
                            results.insert(uri_str, html);
                        } else {
                            to_fetch.push((uri_str, at_uri));
                        }
                    }

                    // If nothing to fetch, respond immediately.
                    if to_fetch.is_empty() {
                        scope.respond(
                            id,
                            EmbedWorkerOutput::Embeds {
                                results,
                                errors,
                                fetch_ms: 0.0,
                            },
                        );
                        return;
                    }

                    // Fetch missing embeds asynchronously.
                    let session = self.session.clone();
                    let worker_cache = self.cache.clone();
                    let scope = scope.clone();

                    wasm_bindgen_futures::spawn_local(async move {
                        // Use weaver-index when use-index feature is enabled.
                        #[cfg(feature = "use-index")]
                        {
                            use jacquard::xrpc::XrpcClient;
                            use jacquard::url::Url;
                            if let Ok(url) = Url::parse("https://index.weaver.sh") {
                                session.set_base_uri(url).await;
                            }
                        }

                        let fetch_start = weaver_common::perf::now();

                        for (uri_str, at_uri) in to_fetch {
                            match weaver_renderer::atproto::fetch_and_render(&at_uri, &session)
                                .await
                            {
                                Ok(html) => {
                                    cache::insert(&worker_cache, at_uri, html.clone());
                                    results.insert(uri_str, html);
                                }
                                Err(e) => {
                                    errors.insert(uri_str, format!("{:?}", e));
                                }
                            }
                        }

                        let fetch_ms = weaver_common::perf::now() - fetch_start;
                        scope.respond(
                            id,
                            EmbedWorkerOutput::Embeds {
                                results,
                                errors,
                                fetch_ms,
                            },
                        );
                    });
                }

                EmbedWorkerInput::ClearCache => {
                    // mini-moka doesn't have a clear method, so we just respond.
                    // The cache will naturally expire entries via TTL.
                    scope.respond(id, EmbedWorkerOutput::CacheCleared);
                }
            }
        }
    }
}

#[cfg(all(target_family = "wasm", target_os = "unknown"))]
pub use worker_impl::EmbedWorker;
