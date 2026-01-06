//! Host-side management for the embed worker.
//!
//! Provides `EmbedWorkerHost` for spawning and communicating with the embed
//! worker from the main thread. This centralizes worker lifecycle management
//! so consuming code just needs to provide a callback for results.

use crate::{EmbedWorkerInput, EmbedWorkerOutput};
use gloo_worker::{Spawnable, WorkerBridge};

/// Host-side manager for the embed worker.
///
/// Handles spawning the worker and sending messages. The callback provided
/// at construction receives all worker outputs.
///
/// # Example
///
/// ```ignore
/// let host = EmbedWorkerHost::spawn("/embed_worker.js", |output| {
///     match output {
///         EmbedWorkerOutput::Embeds { results, errors, fetch_ms } => {
///             // Handle fetched embeds
///         }
///         EmbedWorkerOutput::CacheCleared => {}
///     }
/// });
///
/// host.fetch_embeds(vec!["at://did:plc:xxx/app.bsky.feed.post/yyy".into()]);
/// ```
pub struct EmbedWorkerHost {
    bridge: WorkerBridge<crate::EmbedWorker>,
}

impl EmbedWorkerHost {
    /// Spawn the embed worker with a callback for outputs.
    ///
    /// The `worker_url` should point to the compiled worker JS file,
    /// typically "/embed_worker.js".
    pub fn spawn(worker_url: &str, on_output: impl Fn(EmbedWorkerOutput) + 'static) -> Self {
        let bridge = crate::EmbedWorker::spawner()
            .callback(on_output)
            .spawn(worker_url);
        Self { bridge }
    }

    /// Request embeds for a list of AT URIs.
    ///
    /// The worker will check its cache first, then fetch any missing embeds.
    /// Results arrive via the callback provided at construction.
    pub fn fetch_embeds(&self, uris: Vec<String>) {
        if uris.is_empty() {
            return;
        }
        self.bridge.send(EmbedWorkerInput::FetchEmbeds { uris });
    }

    /// Clear the worker's embed cache.
    pub fn clear_cache(&self) {
        self.bridge.send(EmbedWorkerInput::ClearCache);
    }
}
