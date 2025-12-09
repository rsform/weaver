//! Web Worker for offloading expensive editor operations.
//!
//! This worker maintains a shadow copy of the Loro document and handles
//! CPU-intensive operations like snapshot export and base64 encoding
//! off the main thread.
//!
//! When the `collab-worker` feature is enabled, also handles iroh P2P
//! networking for real-time collaboration.
//!
//! Also handles embed fetching with a persistent cache to avoid re-fetching.

#[cfg(all(target_family = "wasm", target_os = "unknown"))]
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use weaver_common::transport::PresenceSnapshot;

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
    /// Start collab session (worker will spawn CollabNode)
    StartCollab {
        /// blake3 hash of resource URI (32 bytes)
        topic: [u8; 32],
        /// Bootstrap peer node IDs (z-base32 strings)
        bootstrap_peers: Vec<String>,
    },
    /// Loro updates from local edits (forward to gossip)
    BroadcastUpdate {
        /// Loro update bytes
        data: Vec<u8>,
    },
    /// New peers discovered by main thread
    AddPeers {
        /// Node ID strings
        peers: Vec<String>,
    },
    /// Announce ourselves to peers (sent after AddPeers)
    BroadcastJoin {
        /// Our DID
        did: String,
        /// Our display name
        display_name: String,
    },
    /// Local cursor position changed
    BroadcastCursor {
        /// Cursor position
        position: usize,
        /// Selection range if any
        selection: Option<(usize, usize)>,
    },
    /// Stop collab session
    StopCollab,
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
    /// Collab node ready, here's info for session record
    CollabReady {
        /// Node ID (z-base32 string)
        node_id: String,
        /// Relay URL for browser connectivity
        relay_url: Option<String>,
    },
    /// Collab session joined successfully
    CollabJoined,
    /// Remote updates to merge into main doc
    RemoteUpdates {
        /// Loro update bytes
        data: Vec<u8>,
    },
    /// Presence state changed
    PresenceUpdate(PresenceSnapshot),
    /// Collab session ended
    CollabStopped,
    /// A new peer connected (coordinator should send BroadcastJoin)
    PeerConnected,
}

#[cfg(all(target_family = "wasm", target_os = "unknown"))]
mod worker_impl {
    use super::*;
    use futures_util::sink::SinkExt;
    use futures_util::stream::StreamExt;
    use gloo_worker::reactor::{reactor, ReactorScope};
    use weaver_common::transport::CollaboratorInfo;

    #[cfg(feature = "collab-worker")]
    use std::sync::Arc;
    #[cfg(feature = "collab-worker")]
    use weaver_common::transport::{
        CollabMessage, CollabNode, CollabSession, PresenceTracker, SessionEvent, TopicId,
        parse_node_id,
    };

    /// Internal event from gossip handler task to main reactor loop.
    #[cfg(feature = "collab-worker")]
    enum CollabEvent {
        RemoteUpdates { data: Vec<u8> },
        PresenceChanged(PresenceSnapshot),
        PeerConnected,
    }

    /// Editor reactor that maintains a shadow Loro document and handles collab.
    #[reactor]
    pub async fn EditorReactor(mut scope: ReactorScope<WorkerInput, WorkerOutput>) {
        let mut doc: Option<loro::LoroDoc> = None;
        let mut draft_key = String::new();

        // Collab state (only used when collab-worker feature enabled)
        #[cfg(feature = "collab-worker")]
        let mut collab_node: Option<Arc<CollabNode>> = None;
        #[cfg(feature = "collab-worker")]
        let mut collab_session: Option<Arc<CollabSession>> = None;
        #[cfg(feature = "collab-worker")]
        let mut collab_event_rx: Option<tokio::sync::mpsc::UnboundedReceiver<CollabEvent>> = None;
        #[cfg(feature = "collab-worker")]
        const OUR_COLOR: u32 = 0x4ECDC4FF;

        // Helper enum for racing coordinator messages vs collab events
        #[cfg(feature = "collab-worker")]
        enum RaceResult {
            CoordinatorMsg(Option<WorkerInput>),
            CollabEvent(Option<CollabEvent>),
        }

        loop {
            // Race between coordinator messages and collab events
            #[cfg(feature = "collab-worker")]
            let race_result = if let Some(ref mut event_rx) = collab_event_rx {
                use n0_future::FutureExt;
                let coord_fut = async { RaceResult::CoordinatorMsg(scope.next().await) };
                let collab_fut = async { RaceResult::CollabEvent(event_rx.recv().await) };
                coord_fut.race(collab_fut).await
            } else {
                RaceResult::CoordinatorMsg(scope.next().await)
            };

            #[cfg(feature = "collab-worker")]
            match race_result {
                RaceResult::CollabEvent(Some(event)) => {
                    match event {
                        CollabEvent::RemoteUpdates { data } => {
                            if let Err(e) = scope.send(WorkerOutput::RemoteUpdates { data }).await {
                                tracing::error!("Failed to send RemoteUpdates to coordinator: {e}");
                            }
                        }
                        CollabEvent::PresenceChanged(snapshot) => {
                            if let Err(e) = scope.send(WorkerOutput::PresenceUpdate(snapshot)).await {
                                tracing::error!("Failed to send PresenceUpdate to coordinator: {e}");
                            }
                        }
                        CollabEvent::PeerConnected => {
                            if let Err(e) = scope.send(WorkerOutput::PeerConnected).await {
                                tracing::error!("Failed to send PeerConnected to coordinator: {e}");
                            }
                        }
                    }
                    continue; // Go back to racing
                }
                RaceResult::CollabEvent(None) => {
                    // Collab channel closed, continue with just coordinator messages
                    collab_event_rx = None;
                    continue;
                }
                RaceResult::CoordinatorMsg(None) => break, // Coordinator closed
                RaceResult::CoordinatorMsg(Some(msg)) => {
                    // Fall through to message handling below
                    tracing::debug!(?msg, "Worker: received message");
                    match msg {
                WorkerInput::Init {
                    snapshot,
                    draft_key: key,
                } => {
                    let new_doc = loro::LoroDoc::new();
                    if !snapshot.is_empty() {
                        if let Err(e) = new_doc.import(&snapshot) {
                            if let Err(send_err) = scope
                                .send(WorkerOutput::Error {
                                    message: format!("Failed to import snapshot: {e}"),
                                })
                                .await
                            {
                                tracing::error!("Failed to send Error to coordinator: {send_err}");
                            }
                            continue;
                        }
                    }
                    doc = Some(new_doc);
                    draft_key = key;
                    if let Err(e) = scope.send(WorkerOutput::Ready).await {
                        tracing::error!("Failed to send Ready to coordinator: {e}");
                    }
                }

                WorkerInput::ApplyUpdates { updates } => {
                    if let Some(ref doc) = doc {
                        if let Err(e) = doc.import(&updates) {
                            tracing::warn!("Worker failed to import updates: {e}");
                        }
                    }
                }

                WorkerInput::ExportSnapshot {
                    cursor_offset,
                    editing_uri,
                    editing_cid,
                } => {
                    let Some(ref doc) = doc else {
                        if let Err(e) = scope
                            .send(WorkerOutput::Error {
                                message: "No document initialized".into(),
                            })
                            .await
                        {
                            tracing::error!("Failed to send Error to coordinator: {e}");
                        }
                        continue;
                    };

                    let export_start = crate::perf::now();
                    let snapshot_bytes = match doc.export(loro::ExportMode::Snapshot) {
                        Ok(bytes) => bytes,
                        Err(e) => {
                            if let Err(send_err) = scope
                                .send(WorkerOutput::Error {
                                    message: format!("Export failed: {e}"),
                                })
                                .await
                            {
                                tracing::error!("Failed to send Error to coordinator: {send_err}");
                            }
                            continue;
                        }
                    };
                    let export_ms = crate::perf::now() - export_start;

                    let encode_start = crate::perf::now();
                    let b64_snapshot = BASE64.encode(&snapshot_bytes);
                    let encode_ms = crate::perf::now() - encode_start;

                    let content = doc.get_text("content").to_string();
                    let title = doc.get_text("title").to_string();

                    if let Err(e) = scope
                        .send(WorkerOutput::Snapshot {
                            draft_key: draft_key.clone(),
                            b64_snapshot,
                            content,
                            title,
                            cursor_offset,
                            editing_uri,
                            editing_cid,
                            export_ms,
                            encode_ms,
                        })
                        .await
                    {
                        tracing::error!("Failed to send Snapshot to coordinator: {e}");
                    }
                }

                // ============================================================
                // Collab handlers - full impl when collab-worker feature enabled
                // ============================================================
                #[cfg(feature = "collab-worker")]
                WorkerInput::StartCollab {
                    topic,
                    bootstrap_peers,
                } => {
                    // Spawn CollabNode
                    let node = match CollabNode::spawn(None).await {
                        Ok(n) => n,
                        Err(e) => {
                            if let Err(send_err) = scope
                                .send(WorkerOutput::Error {
                                    message: format!("Failed to spawn CollabNode: {e}"),
                                })
                                .await
                            {
                                tracing::error!("Failed to send Error to coordinator: {send_err}");
                            }
                            continue;
                        }
                    };

                    // Wait for relay connection
                    let relay_url = node.wait_for_relay().await;
                    let node_id = node.node_id_string();

                    // Send ready so main thread can create session record
                    if let Err(e) = scope
                        .send(WorkerOutput::CollabReady {
                            node_id,
                            relay_url: Some(relay_url),
                        })
                        .await
                    {
                        tracing::error!("Failed to send CollabReady to coordinator: {e}");
                    }

                    collab_node = Some(node.clone());

                    // Parse bootstrap peers
                    let peers: Vec<_> = bootstrap_peers
                        .iter()
                        .filter_map(|s| parse_node_id(s).ok())
                        .collect();

                    // Join gossip session
                    let topic_id = TopicId::from_bytes(topic);
                    match CollabSession::join(node, topic_id, peers).await {
                        Ok((session, mut events)) => {
                            let session = Arc::new(session);
                            collab_session = Some(session.clone());
                            if let Err(e) = scope.send(WorkerOutput::CollabJoined).await {
                                tracing::error!("Failed to send CollabJoined to coordinator: {e}");
                            }

                            // NOTE: Don't broadcast Join here - wait for BroadcastJoin message
                            // after peers have been added via AddPeers

                            // Create channel for events from spawned task
                            let (event_tx, event_rx) = tokio::sync::mpsc::unbounded_channel();
                            collab_event_rx = Some(event_rx);

                            // Spawn event handler task that sends via channel
                            wasm_bindgen_futures::spawn_local(async move {
                                let mut presence = PresenceTracker::new();

                                while let Some(Ok(event)) = events.next().await {
                                    match event {
                                        SessionEvent::Message { from, message } => {
                                            match message {
                                                CollabMessage::LoroUpdate { data, .. } => {
                                                    if event_tx.send(CollabEvent::RemoteUpdates { data }).is_err() {
                                                        tracing::warn!("Collab event channel closed");
                                                        return;
                                                    }
                                                }
                                                CollabMessage::Join { did, display_name } => {
                                                    tracing::info!(%from, %did, %display_name, "Received Join message");
                                                    presence.add_collaborator(from, did, display_name);
                                                    if event_tx.send(CollabEvent::PresenceChanged(
                                                        presence_to_snapshot(&presence),
                                                    )).is_err() {
                                                        tracing::warn!("Collab event channel closed");
                                                        return;
                                                    }
                                                }
                                                CollabMessage::Leave { .. } => {
                                                    presence.remove_collaborator(&from);
                                                    if event_tx.send(CollabEvent::PresenceChanged(
                                                        presence_to_snapshot(&presence),
                                                    )).is_err() {
                                                        tracing::warn!("Collab event channel closed");
                                                        return;
                                                    }
                                                }
                                                CollabMessage::Cursor {
                                                    position,
                                                    selection,
                                                    ..
                                                } => {
                                                    // Note: cursor updates require the collaborator to exist
                                                    // (added via Join message)
                                                    let exists = presence.contains(&from);
                                                    tracing::debug!(%from, position, ?selection, exists, "Received Cursor message");
                                                    presence.update_cursor(&from, position, selection);
                                                    if event_tx.send(CollabEvent::PresenceChanged(
                                                        presence_to_snapshot(&presence),
                                                    )).is_err() {
                                                        tracing::warn!("Collab event channel closed");
                                                        return;
                                                    }
                                                }
                                                _ => {}
                                            }
                                        }
                                        SessionEvent::PeerJoined(peer) => {
                                            tracing::info!(%peer, "PeerJoined - notifying coordinator");
                                            // Notify coordinator so it can send BroadcastJoin
                                            // Don't add to presence yet - wait for their Join message
                                            if event_tx.send(CollabEvent::PeerConnected).is_err() {
                                                tracing::warn!("Collab event channel closed");
                                                return;
                                            }
                                        }
                                        SessionEvent::PeerLeft(peer) => {
                                            presence.remove_collaborator(&peer);
                                            if event_tx.send(CollabEvent::PresenceChanged(
                                                presence_to_snapshot(&presence),
                                            )).is_err() {
                                                tracing::warn!("Collab event channel closed");
                                                return;
                                            }
                                        }
                                        SessionEvent::Joined => {}
                                    }
                                }
                            });
                        }
                        Err(e) => {
                            if let Err(send_err) = scope
                                .send(WorkerOutput::Error {
                                    message: format!("Failed to join session: {e}"),
                                })
                                .await
                            {
                                tracing::error!("Failed to send Error to coordinator: {send_err}");
                            }
                        }
                    }
                }

                #[cfg(feature = "collab-worker")]
                WorkerInput::BroadcastUpdate { data } => {
                    if let Some(ref session) = collab_session {
                        let msg = CollabMessage::LoroUpdate {
                            data,
                            version: vec![],
                        };
                        if let Err(e) = session.broadcast(&msg).await {
                            tracing::warn!("Broadcast failed: {e}");
                        }
                    }
                }

                #[cfg(feature = "collab-worker")]
                WorkerInput::BroadcastCursor { position, selection } => {
                    if let Some(ref session) = collab_session {
                        tracing::debug!(position, ?selection, "Worker: broadcasting cursor");
                        let msg = CollabMessage::Cursor {
                            position,
                            selection,
                            color: OUR_COLOR,
                        };
                        if let Err(e) = session.broadcast(&msg).await {
                            tracing::warn!("Cursor broadcast failed: {e}");
                        }
                    } else {
                        tracing::debug!(position, ?selection, "Worker: BroadcastCursor but no session");
                    }
                }

                #[cfg(feature = "collab-worker")]
                WorkerInput::AddPeers { peers } => {
                    tracing::info!(count = peers.len(), "Worker: received AddPeers");
                    if let Some(ref session) = collab_session {
                        let peer_ids: Vec<_> = peers
                            .iter()
                            .filter_map(|s| {
                                match parse_node_id(s) {
                                    Ok(id) => Some(id),
                                    Err(e) => {
                                        tracing::warn!(node_id = %s, error = %e, "Failed to parse node_id");
                                        None
                                    }
                                }
                            })
                            .collect();
                        tracing::info!(parsed_count = peer_ids.len(), "Worker: joining peers");
                        if let Err(e) = session.join_peers(peer_ids).await {
                            tracing::warn!("Failed to add peers: {e}");
                        }
                    } else {
                        tracing::warn!("Worker: AddPeers but no collab_session");
                    }
                }

                #[cfg(feature = "collab-worker")]
                WorkerInput::BroadcastJoin { did, display_name } => {
                    if let Some(ref session) = collab_session {
                        let join_msg = CollabMessage::Join { did, display_name };
                        if let Err(e) = session.broadcast(&join_msg).await {
                            tracing::warn!("Failed to broadcast Join: {e}");
                        }
                    }
                }

                #[cfg(feature = "collab-worker")]
                WorkerInput::StopCollab => {
                    collab_session = None;
                    collab_node = None;
                    collab_event_rx = None;
                    if let Err(e) = scope.send(WorkerOutput::CollabStopped).await {
                        tracing::error!("Failed to send CollabStopped to coordinator: {e}");
                    }
                }

                    } // end match msg
                } // end RaceResult::CoordinatorMsg(Some(msg))
            } // end match race_result

            // Non-collab-worker: simple message loop
            #[cfg(not(feature = "collab-worker"))]
            {
                let Some(msg) = scope.next().await else { break };
                tracing::debug!(?msg, "Worker: received message");
                match msg {
                    WorkerInput::Init { snapshot, draft_key: key } => {
                        let new_doc = loro::LoroDoc::new();
                        if !snapshot.is_empty() {
                            if let Err(e) = new_doc.import(&snapshot) {
                                if let Err(send_err) = scope
                                    .send(WorkerOutput::Error {
                                        message: format!("Failed to import snapshot: {e}"),
                                    })
                                    .await
                                {
                                    tracing::error!("Failed to send Error to coordinator: {send_err}");
                                }
                                continue;
                            }
                        }
                        doc = Some(new_doc);
                        draft_key = key;
                        if let Err(e) = scope.send(WorkerOutput::Ready).await {
                            tracing::error!("Failed to send Ready to coordinator: {e}");
                        }
                    }
                    WorkerInput::ApplyUpdates { updates } => {
                        if let Some(ref doc) = doc {
                            if let Err(e) = doc.import(&updates) {
                                tracing::warn!("Worker failed to import updates: {e}");
                            }
                        }
                    }
                    WorkerInput::ExportSnapshot { cursor_offset, editing_uri, editing_cid } => {
                        let Some(ref doc) = doc else {
                            if let Err(e) = scope.send(WorkerOutput::Error { message: "No document initialized".into() }).await {
                                tracing::error!("Failed to send Error to coordinator: {e}");
                            }
                            continue;
                        };
                        let export_start = crate::perf::now();
                        let snapshot_bytes = match doc.export(loro::ExportMode::Snapshot) {
                            Ok(bytes) => bytes,
                            Err(e) => {
                                if let Err(send_err) = scope.send(WorkerOutput::Error { message: format!("Export failed: {e}") }).await {
                                    tracing::error!("Failed to send Error to coordinator: {send_err}");
                                }
                                continue;
                            }
                        };
                        let export_ms = crate::perf::now() - export_start;
                        let encode_start = crate::perf::now();
                        let b64_snapshot = BASE64.encode(&snapshot_bytes);
                        let encode_ms = crate::perf::now() - encode_start;
                        let content = doc.get_text("content").to_string();
                        let title = doc.get_text("title").to_string();
                        if let Err(e) = scope.send(WorkerOutput::Snapshot {
                            draft_key: draft_key.clone(), b64_snapshot, content, title,
                            cursor_offset, editing_uri, editing_cid, export_ms, encode_ms,
                        }).await {
                            tracing::error!("Failed to send Snapshot to coordinator: {e}");
                        }
                    }
                    // Collab stubs for non-collab-worker build
                    WorkerInput::StartCollab { .. } => {
                        if let Err(e) = scope.send(WorkerOutput::Error { message: "Collab not enabled".into() }).await {
                            tracing::error!("Failed to send Error to coordinator: {e}");
                        }
                    }
                    WorkerInput::BroadcastUpdate { .. } => {}
                    WorkerInput::AddPeers { .. } => {}
                    WorkerInput::BroadcastJoin { .. } => {}
                    WorkerInput::BroadcastCursor { .. } => {}
                    WorkerInput::StopCollab => {
                        if let Err(e) = scope.send(WorkerOutput::CollabStopped).await {
                            tracing::error!("Failed to send CollabStopped to coordinator: {e}");
                        }
                    }
                }
            }
        }
    }

    /// Convert PresenceTracker to serializable PresenceSnapshot.
    #[cfg(feature = "collab-worker")]
    fn presence_to_snapshot(tracker: &PresenceTracker) -> PresenceSnapshot {
        let collaborators = tracker
            .collaborators()
            .map(|c| CollaboratorInfo {
                node_id: c.node_id.to_string(),
                did: c.did.clone(),
                display_name: c.display_name.clone(),
                color: c.color,
                cursor_position: c.cursor.as_ref().map(|cur| cur.position),
                selection: c.cursor.as_ref().and_then(|cur| cur.selection),
            })
            .collect();

        PresenceSnapshot {
            collaborators,
            peer_count: tracker.len(),
        }
    }
}

#[cfg(all(target_family = "wasm", target_os = "unknown"))]
pub use worker_impl::EditorReactor;

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
