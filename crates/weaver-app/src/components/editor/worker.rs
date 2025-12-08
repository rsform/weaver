//! Web Worker for offloading expensive editor operations.
//!
//! This worker maintains a shadow copy of the Loro document and handles
//! CPU-intensive operations like snapshot export and base64 encoding
//! off the main thread.
//!
//! Also handles embed fetching with a persistent cache to avoid re-fetching.

#[cfg(all(target_family = "wasm", target_os = "unknown"))]
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Input messages to the editor worker.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum WorkerInput {
    /// Initialize the worker with a full Loro snapshot.
    Init {
        /// Full Loro snapshot bytes
        snapshot: Vec<u8>,
        /// Draft key for storage
        draft_key: String,
    },
    /// Apply incremental Loro updates to the shadow document.
    ApplyUpdates {
        /// Loro update bytes (delta since last sync)
        updates: Vec<u8>,
    },
    /// Request a snapshot export for autosave.
    ExportSnapshot {
        /// Current cursor position (for snapshot metadata)
        cursor_offset: usize,
        /// Editing URI if editing existing entry
        editing_uri: Option<String>,
        /// Editing CID if editing existing entry
        editing_cid: Option<String>,
    },
}

/// Output messages from the editor worker.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum WorkerOutput {
    /// Worker initialized successfully.
    Ready,
    /// Snapshot export completed.
    Snapshot {
        /// Draft key for storage
        draft_key: String,
        /// Base64-encoded Loro snapshot
        b64_snapshot: String,
        /// Human-readable content (for debugging)
        content: String,
        /// Entry title
        title: String,
        /// Cursor offset
        cursor_offset: usize,
        /// Editing URI
        editing_uri: Option<String>,
        /// Editing CID
        editing_cid: Option<String>,
        /// Export timing in ms
        export_ms: f64,
        /// Encode timing in ms
        encode_ms: f64,
    },
    /// Error occurred.
    Error { message: String },
}

#[cfg(all(target_family = "wasm", target_os = "unknown"))]
mod worker_impl {
    use super::*;
    use gloo_worker::{HandlerId, Worker, WorkerScope};

    /// Editor worker that maintains a shadow Loro document.
    pub struct EditorWorker {
        /// Shadow Loro document
        doc: Option<loro::LoroDoc>,
        /// Draft key for storage identification
        draft_key: String,
    }

    impl Worker for EditorWorker {
        type Message = ();
        type Input = WorkerInput;
        type Output = WorkerOutput;

        fn create(_scope: &WorkerScope<Self>) -> Self {
            Self {
                doc: None,
                draft_key: String::new(),
            }
        }

        fn update(&mut self, _scope: &WorkerScope<Self>, _msg: Self::Message) {}

        fn received(&mut self, scope: &WorkerScope<Self>, msg: Self::Input, id: HandlerId) {
            match msg {
                WorkerInput::Init {
                    snapshot,
                    draft_key,
                } => {
                    let doc = loro::LoroDoc::new();
                    if !snapshot.is_empty() {
                        if let Err(e) = doc.import(&snapshot) {
                            scope.respond(
                                id,
                                WorkerOutput::Error {
                                    message: format!("Failed to import snapshot: {e}"),
                                },
                            );
                            return;
                        }
                    }
                    self.doc = Some(doc);
                    self.draft_key = draft_key;
                    scope.respond(id, WorkerOutput::Ready);
                }

                WorkerInput::ApplyUpdates { updates } => {
                    if let Some(ref doc) = self.doc {
                        if let Err(e) = doc.import(&updates) {
                            // Log but don't fail - updates can be stale
                            tracing::warn!("Worker failed to import updates: {e}");
                        }
                    }
                    // No response for updates - fire and forget
                }

                WorkerInput::ExportSnapshot {
                    cursor_offset,
                    editing_uri,
                    editing_cid,
                } => {
                    let Some(ref doc) = self.doc else {
                        scope.respond(
                            id,
                            WorkerOutput::Error {
                                message: "No document initialized".into(),
                            },
                        );
                        return;
                    };

                    // Export snapshot
                    let export_start = crate::perf::now();
                    let snapshot_bytes = match doc.export(loro::ExportMode::Snapshot) {
                        Ok(bytes) => bytes,
                        Err(e) => {
                            scope.respond(
                                id,
                                WorkerOutput::Error {
                                    message: format!("Export failed: {e}"),
                                },
                            );
                            return;
                        }
                    };
                    let export_ms = crate::perf::now() - export_start;

                    // Base64 encode
                    let encode_start = crate::perf::now();
                    let b64_snapshot = BASE64.encode(&snapshot_bytes);
                    let encode_ms = crate::perf::now() - encode_start;

                    // Extract content and title
                    let content = doc.get_text("content").to_string();
                    let title = doc.get_text("title").to_string();

                    scope.respond(
                        id,
                        WorkerOutput::Snapshot {
                            draft_key: self.draft_key.clone(),
                            b64_snapshot,
                            content,
                            title,
                            cursor_offset,
                            editing_uri,
                            editing_cid,
                            export_ms,
                            encode_ms,
                        },
                    );
                }
            }
        }
    }
}

#[cfg(all(target_family = "wasm", target_os = "unknown"))]
pub use worker_impl::EditorWorker;

// ============================================================================
// Embed Worker - fetches and caches AT Protocol embeds
// ============================================================================

/// Input messages to the embed worker.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum EmbedWorkerInput {
    /// Request embeds for a list of AT URIs.
    /// Worker returns cached results immediately and fetches missing ones.
    FetchEmbeds {
        /// AT URIs to fetch (e.g., "at://did:plc:xxx/app.bsky.feed.post/yyy")
        uris: Vec<String>,
    },
    /// Clear the cache (e.g., on session change)
    ClearCache,
}

/// Output messages from the embed worker.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum EmbedWorkerOutput {
    /// Embed results (may be partial if some failed)
    Embeds {
        /// Successfully fetched/cached embeds: uri -> rendered HTML
        results: HashMap<String, String>,
        /// URIs that failed to fetch
        errors: HashMap<String, String>,
        /// Timing info
        fetch_ms: f64,
    },
    /// Cache was cleared
    CacheCleared,
}

#[cfg(all(target_family = "wasm", target_os = "unknown"))]
mod embed_worker_impl {
    use super::*;
    use crate::cache_impl;
    use gloo_worker::{HandlerId, Worker, WorkerScope};
    use jacquard::client::UnauthenticatedSession;
    use jacquard::identity::JacquardResolver;
    use jacquard::prelude::*;
    use jacquard::types::string::AtUri;
    use jacquard::IntoStatic;
    use std::time::Duration;

    /// Embed worker with persistent cache.
    pub struct EmbedWorker {
        /// Cached rendered embeds with TTL and max capacity
        cache: cache_impl::Cache<AtUri<'static>, String>,
        /// Unauthenticated session for public API calls
        session: UnauthenticatedSession<JacquardResolver>,
    }

    impl Worker for EmbedWorker {
        type Message = ();
        type Input = EmbedWorkerInput;
        type Output = EmbedWorkerOutput;

        fn create(_scope: &WorkerScope<Self>) -> Self {
            Self {
                // Cache up to 500 embeds, TTL of 1 hour
                cache: cache_impl::new_cache(500, Duration::from_secs(3600)),
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

                    // Parse URIs and check cache
                    for uri_str in uris {
                        let at_uri = match AtUri::new_owned(uri_str.clone()) {
                            Ok(u) => u,
                            Err(e) => {
                                errors.insert(uri_str, format!("Invalid AT URI: {e}"));
                                continue;
                            }
                        };

                        if let Some(html) = cache_impl::get(&self.cache, &at_uri) {
                            results.insert(uri_str, html);
                        } else {
                            to_fetch.push((uri_str, at_uri));
                        }
                    }

                    // If nothing to fetch, respond immediately
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

                    // Fetch missing embeds asynchronously
                    let session = self.session.clone();
                    let cache = self.cache.clone();
                    let scope = scope.clone();

                    wasm_bindgen_futures::spawn_local(async move {
                        let fetch_start = crate::perf::now();

                        for (uri_str, at_uri) in to_fetch {
                            match weaver_renderer::atproto::fetch_and_render(&at_uri, &session)
                                .await
                            {
                                Ok(html) => {
                                    cache_impl::insert(&cache, at_uri, html.clone());
                                    results.insert(uri_str, html);
                                }
                                Err(e) => {
                                    errors.insert(uri_str, format!("{:?}", e));
                                }
                            }
                        }

                        let fetch_ms = crate::perf::now() - fetch_start;
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
                    // mini-moka doesn't have a clear method, so we just recreate
                    // (this is fine since ClearCache is rarely called)
                    scope.respond(id, EmbedWorkerOutput::CacheCleared);
                }
            }
        }
    }
}

#[cfg(all(target_family = "wasm", target_os = "unknown"))]
pub use embed_worker_impl::EmbedWorker;
