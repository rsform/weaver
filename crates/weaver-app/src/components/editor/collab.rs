//! Collab coordinator - bridges EditorWorker and authenticated PDS ops.
//!
//! This component handles the main-thread side of real-time collaboration:
//! - Spawns the editor worker and manages its lifecycle
//! - Performs authenticated PDS operations (session records, peer discovery)
//! - Forwards local Loro updates to the worker for gossip broadcast
//! - Receives remote updates from worker and applies to main document
//! - Provides CollabDebugState context for debug UI
//!
//! The worker handles all iroh/gossip networking off the main thread.

// Only compile for WASM - no-op stub provided at end

use super::document::EditorDocument;

use dioxus::prelude::*;

#[cfg(target_arch = "wasm32")]
use jacquard::types::string::AtUri;

use weaver_common::transport::PresenceSnapshot;

/// Session record TTL in minutes.
#[cfg(target_arch = "wasm32")]
const SESSION_TTL_MINUTES: u32 = 15;

/// How often to refresh session record (ms).
#[cfg(target_arch = "wasm32")]
const SESSION_REFRESH_INTERVAL_MS: u32 = 5 * 60 * 1000; // 5 minutes

/// How often to poll for new peers (ms).
#[cfg(target_arch = "wasm32")]
const PEER_DISCOVERY_INTERVAL_MS: u32 = 30 * 1000; // 30 seconds

/// Props for the CollabCoordinator component.
#[derive(Props, Clone, PartialEq)]
pub struct CollabCoordinatorProps {
    /// The editor document to sync
    pub document: EditorDocument,
    /// Resource URI for the document being edited
    pub resource_uri: String,
    /// Presence state signal (updated by coordinator)
    pub presence: Signal<PresenceSnapshot>,
    /// Children to render (this component wraps the editor)
    pub children: Element,
}

/// Coordinator state machine states.
#[cfg(target_arch = "wasm32")]
#[derive(Debug, Clone, PartialEq)]
enum CoordinatorState {
    /// Initial state - waiting for worker to be ready
    Initializing,
    /// Creating session record on PDS
    CreatingSession {
        node_id: String,
        relay_url: Option<String>,
    },
    /// Active collab session
    Active { session_uri: AtUri<'static> },
    /// Error state
    Error(String),
}

/// Coordinator component that bridges worker and PDS.
///
/// This is a wrapper component that:
/// 1. Provides CollabDebugState context
/// 2. Manages collab lifecycle (worker, PDS records, peer discovery)
/// 3. Renders children
///
/// Lifecycle:
/// 1. Worker spawned on mount, sends CollabReady with node_id
/// 2. Coordinator creates session record on PDS
/// 3. Coordinator discovers existing peers
/// 4. Worker joins gossip session
/// 5. Local updates forwarded to worker via subscribe_local_update
/// 6. Remote updates from worker applied to main document
/// 7. Session record deleted on unmount
#[component]
pub fn CollabCoordinator(props: CollabCoordinatorProps) -> Element {
    #[cfg(target_arch = "wasm32")]
    {
        use super::worker::{WorkerInput, WorkerOutput};
        use crate::collab_context::CollabDebugState;
        use crate::fetch::Fetcher;
        use futures_util::stream::SplitSink;
        use futures_util::{SinkExt, StreamExt};
        use gloo_worker::Spawnable;
        use gloo_worker::reactor::ReactorBridge;
        use jacquard::IntoStatic;
        use weaver_common::WeaverExt;

        use super::worker::EditorReactor;

        let fetcher = use_context::<Fetcher>();

        // Provide debug state context
        let mut debug_state = use_signal(CollabDebugState::default);
        use_context_provider(|| debug_state);

        // Coordinator state
        let mut state: Signal<CoordinatorState> = use_signal(|| CoordinatorState::Initializing);

        // Worker sink for sending messages - Signal persists across renders
        type WorkerSink = SplitSink<ReactorBridge<EditorReactor>, WorkerInput>;
        let mut worker_sink: Signal<Option<WorkerSink>> = use_signal(|| None);

        // Session record URI for cleanup
        let mut session_uri: Signal<Option<AtUri<'static>>> = use_signal(|| None);

        // Loro subscription handle (keep alive)
        let mut loro_sub: Signal<Option<loro::Subscription>> = use_signal(|| None);

        // Clone for closures
        let resource_uri = props.resource_uri.clone();
        let mut doc = props.document.clone();
        let mut presence = props.presence;

        // Spawn worker and set up message handling
        let fetcher_for_spawn = fetcher.clone();
        let resource_uri_for_spawn = resource_uri.clone();
        use_effect(move || {
            let mut worker_sink = worker_sink;
            let fetcher = fetcher_for_spawn.clone();
            let resource_uri = resource_uri_for_spawn.clone();
            // Channel for local updates (Loro callback is Send+Sync, but ReactorBridge isn't)
            let (local_update_tx, mut local_update_rx) =
                tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();

            let tx = local_update_tx.clone();

            // Subscribe to local Loro updates - just send to channel (Send+Sync)
            let sub = doc
                .loro_doc()
                .subscribe_local_update(Box::new(move |update| {
                    let _ = tx.send(update.to_vec());
                    true // Keep subscription active
                }));

            loro_sub.set(Some(sub));

            // Spawn the reactor
            let bridge = EditorReactor::spawner().spawn("/editor_worker.js");
            let (sink, mut stream) = bridge.split();
            worker_sink.set(Some(sink));

            // Initialize worker with current document snapshot
            let snapshot = doc.export_snapshot();
            let draft_key = resource_uri.clone(); // Use resource URI as the key
            spawn(async move {
                if let Some(ref mut sink) = *worker_sink.write() {
                    if let Err(e) = sink
                        .send(WorkerInput::Init {
                            snapshot,
                            draft_key,
                        })
                        .await
                    {
                        tracing::error!("Failed to send Init to worker: {e}");
                    }
                }
            });

            // Task 1: Forward local updates from channel to worker
            spawn(async move {
                while let Some(data) = local_update_rx.recv().await {
                    if let Some(ref mut s) = *worker_sink.write() {
                        if let Err(e) = s.send(WorkerInput::BroadcastUpdate { data }).await {
                            tracing::warn!("Failed to send BroadcastUpdate to worker: {e}");
                        }
                    }
                }
            });

            // Task 2: Handle worker output messages
            let doc_for_handler = doc.clone();
            spawn(async move {
                let mut doc = doc_for_handler;
                while let Some(output) = stream.next().await {
                    match output {
                        WorkerOutput::Ready => {
                            tracing::info!("CollabCoordinator: worker ready, starting collab");

                            // Compute topic from resource URI
                            let hash = weaver_common::blake3::hash(resource_uri.as_bytes());
                            let topic: [u8; 32] = *hash.as_bytes();

                            // Send StartCollab to worker immediately (no blocking on profile fetch)
                            if let Some(ref mut s) = *worker_sink.write() {
                                if let Err(e) = s
                                    .send(WorkerInput::StartCollab {
                                        topic,
                                        bootstrap_peers: vec![],
                                    })
                                    .await
                                {
                                    tracing::error!("Failed to send StartCollab to worker: {e}");
                                }
                            }
                        }

                        WorkerOutput::CollabReady { node_id, relay_url } => {
                            tracing::info!(
                                node_id = %node_id,
                                relay_url = ?relay_url,
                                "CollabCoordinator: collab node ready"
                            );

                            // Update debug state
                            debug_state.with_mut(|ds| {
                                ds.node_id = Some(node_id.clone());
                                ds.relay_url = relay_url.clone();
                            });

                            state.set(CoordinatorState::CreatingSession {
                                node_id: node_id.clone(),
                                relay_url: relay_url.clone(),
                            });

                            // Create session record on PDS
                            let fetcher = fetcher.clone();
                            let resource_uri = resource_uri.clone();

                            spawn(async move {
                                // Parse resource URI to get StrongRef
                                let uri = match AtUri::new(&resource_uri) {
                                    Ok(u) => u.into_static(),
                                    Err(e) => {
                                        let err = format!("Invalid resource URI: {e}");
                                        debug_state
                                            .with_mut(|ds| ds.last_error = Some(err.clone()));
                                        state.set(CoordinatorState::Error(err));
                                        return;
                                    }
                                };

                                // Get StrongRef for the resource
                                let strong_ref = match fetcher.confirm_record_ref(&uri).await {
                                    Ok(r) => r,
                                    Err(e) => {
                                        let err = format!("Failed to get resource ref: {e}");
                                        debug_state
                                            .with_mut(|ds| ds.last_error = Some(err.clone()));
                                        state.set(CoordinatorState::Error(err));
                                        return;
                                    }
                                };

                                // Create session record
                                match fetcher
                                    .create_collab_session(
                                        &strong_ref,
                                        &node_id,
                                        relay_url.as_deref(),
                                        Some(SESSION_TTL_MINUTES),
                                    )
                                    .await
                                {
                                    Ok(session_record_uri) => {
                                        tracing::info!(
                                            uri = %session_record_uri,
                                            "CollabCoordinator: session record created"
                                        );
                                        session_uri.set(Some(session_record_uri.clone()));
                                        debug_state.with_mut(|ds| {
                                            ds.session_record_uri =
                                                Some(session_record_uri.to_string());
                                        });

                                        // Discover existing peers
                                        let bootstrap_peers = match fetcher
                                            .find_session_peers(&uri)
                                            .await
                                        {
                                            Ok(peers) => {
                                                tracing::info!(
                                                    count = peers.len(),
                                                    "CollabCoordinator: found peers"
                                                );
                                                debug_state.with_mut(|ds| {
                                                    ds.discovered_peers = peers.len();
                                                });
                                                peers
                                                    .into_iter()
                                                    .map(|p| p.node_id)
                                                    .collect::<Vec<_>>()
                                            }
                                            Err(e) => {
                                                tracing::warn!(
                                                    "CollabCoordinator: peer discovery failed: {e}"
                                                );
                                                vec![]
                                            }
                                        };

                                        // Send discovered peers to worker
                                        if !bootstrap_peers.is_empty() {
                                            tracing::info!(
                                                count = bootstrap_peers.len(),
                                                peers = ?bootstrap_peers,
                                                "CollabCoordinator: sending AddPeers to worker"
                                            );
                                            if let Some(ref mut s) = *worker_sink.write() {
                                                if let Err(e) = s
                                                    .send(WorkerInput::AddPeers {
                                                        peers: bootstrap_peers,
                                                    })
                                                    .await
                                                {
                                                    tracing::error!("CollabCoordinator: AddPeers send failed: {e}");
                                                }
                                            } else {
                                                tracing::error!("CollabCoordinator: sink is None!");
                                            }
                                        } else {
                                            tracing::info!("CollabCoordinator: no peers to add");
                                        }

                                        state.set(CoordinatorState::Active {
                                            session_uri: session_record_uri,
                                        });
                                    }
                                    Err(e) => {
                                        let err = format!("Failed to create session: {e}");
                                        debug_state
                                            .with_mut(|ds| ds.last_error = Some(err.clone()));
                                        state.set(CoordinatorState::Error(err));
                                    }
                                }
                            });
                        }

                        WorkerOutput::CollabJoined => {
                            tracing::info!("CollabCoordinator: joined gossip session");
                            debug_state.with_mut(|ds| ds.is_joined = true);
                        }

                        WorkerOutput::RemoteUpdates { data } => {
                            if let Err(e) = doc.import_updates(&data) {
                                tracing::warn!(
                                    "CollabCoordinator: failed to import updates: {:?}",
                                    e
                                );
                            }
                        }

                        WorkerOutput::PresenceUpdate(snapshot) => {
                            debug_state.with_mut(|ds| {
                                ds.connected_peers = snapshot.peer_count;
                            });
                            presence.set(snapshot);
                        }

                        WorkerOutput::CollabStopped => {
                            tracing::info!("CollabCoordinator: collab stopped");
                            debug_state.with_mut(|ds| {
                                ds.is_joined = false;
                                ds.connected_peers = 0;
                            });
                        }

                        WorkerOutput::PeerConnected => {
                            tracing::info!("CollabCoordinator: peer connected, sending our Join");
                            use weaver_api::sh_weaver::actor::ProfileDataViewInner;

                            let fetcher = fetcher.clone();

                            // Get our profile info and send BroadcastJoin
                            let (our_did, our_display_name) = match fetcher.current_did().await {
                                Some(did) => {
                                    let display_name = match fetcher.fetch_profile(&did.clone().into()).await {
                                        Ok(profile) => {
                                            match &profile.inner {
                                                ProfileDataViewInner::ProfileView(p) => {
                                                    p.display_name.as_ref().map(|s| s.to_string()).unwrap_or_else(|| did.to_string())
                                                }
                                                ProfileDataViewInner::ProfileViewDetailed(p) => {
                                                    p.display_name.as_ref().map(|s| s.to_string()).unwrap_or_else(|| did.to_string())
                                                }
                                                ProfileDataViewInner::TangledProfileView(p) => {
                                                    p.handle.to_string()
                                                }
                                                _ => did.to_string(),
                                            }
                                        }
                                        Err(_) => did.to_string(),
                                    };
                                    (did.to_string(), display_name)
                                }
                                None => {
                                    tracing::warn!("CollabCoordinator: no current DID for Join message");
                                    ("unknown".to_string(), "Anonymous".to_string())
                                }
                            };

                            if let Some(ref mut s) = *worker_sink.write() {
                                if let Err(e) = s
                                    .send(WorkerInput::BroadcastJoin {
                                        did: our_did,
                                        display_name: our_display_name,
                                    })
                                    .await
                                {
                                    tracing::error!("CollabCoordinator: BroadcastJoin send failed: {e}");
                                }
                            }
                        }

                        WorkerOutput::Error { message } => {
                            tracing::error!("CollabCoordinator: worker error: {message}");
                            debug_state.with_mut(|ds| ds.last_error = Some(message.clone()));
                            state.set(CoordinatorState::Error(message));
                        }

                        WorkerOutput::Snapshot { .. } => {}
                    }
                }
                tracing::info!("CollabCoordinator: worker stream ended");
            });

            tracing::info!("CollabCoordinator: spawned worker");
        });

        // Forward cursor updates to worker - memo re-runs when cursor/selection signals change
        let cursor_signal = props.document.cursor;
        let selection_signal = props.document.selection;

        let _cursor_broadcaster = use_memo(move || {
            let cursor = cursor_signal.read();
            let selection = *selection_signal.read();
            let position = cursor.offset;
            let sel = selection.map(|s| (s.anchor, s.head));

            tracing::debug!(position, ?sel, "CollabCoordinator: cursor changed, broadcasting");

            spawn(async move {
                if let Some(ref mut s) = *worker_sink.write() {
                    tracing::debug!(position, "CollabCoordinator: sending BroadcastCursor to worker");
                    if let Err(e) = s
                        .send(WorkerInput::BroadcastCursor {
                            position,
                            selection: sel,
                        })
                        .await
                    {
                        tracing::warn!("Failed to send BroadcastCursor to worker: {e}");
                    }
                } else {
                    tracing::debug!(position, "CollabCoordinator: worker sink not ready, skipping cursor broadcast");
                }
            });
        });

        // Periodic peer discovery
        let fetcher_for_discovery = fetcher.clone();
        let resource_uri_for_discovery = resource_uri.clone();
        dioxus_sdk::time::use_interval(
            std::time::Duration::from_millis(PEER_DISCOVERY_INTERVAL_MS as u64),
            move |_| {
                let fetcher = fetcher_for_discovery.clone();
                let resource_uri = resource_uri_for_discovery.clone();

                spawn(async move {
                    let uri = match AtUri::new(&resource_uri) {
                        Ok(u) => u,
                        Err(_) => return,
                    };

                    match fetcher.find_session_peers(&uri).await {
                        Ok(peers) => {
                            debug_state.with_mut(|ds| ds.discovered_peers = peers.len());
                            if !peers.is_empty() {
                                let peer_ids: Vec<String> =
                                    peers.into_iter().map(|p| p.node_id).collect();

                                if let Some(ref mut s) = *worker_sink.write() {
                                    if let Err(e) =
                                        s.send(WorkerInput::AddPeers { peers: peer_ids }).await
                                    {
                                        tracing::warn!("Periodic AddPeers send failed: {e}");
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            tracing::debug!("Peer discovery failed: {e}");
                        }
                    }
                });
            },
        );

        // Periodic session refresh
        let fetcher_for_refresh = fetcher.clone();
        dioxus_sdk::time::use_interval(
            std::time::Duration::from_millis(SESSION_REFRESH_INTERVAL_MS as u64),
            move |_| {
                let fetcher = fetcher_for_refresh.clone();

                if let Some(ref uri) = *session_uri.peek() {
                    let uri = uri.clone();
                    spawn(async move {
                        match fetcher
                            .refresh_collab_session(&uri, SESSION_TTL_MINUTES)
                            .await
                        {
                            Ok(_) => {
                                tracing::debug!("Session refreshed");
                            }
                            Err(e) => {
                                tracing::warn!("Session refresh failed: {e}");
                            }
                        }
                    });
                }
            },
        );

        // Cleanup on unmount
        let fetcher_for_cleanup = fetcher.clone();
        use_drop(move || {
            // Stop collab in worker
            spawn(async move {
                if let Some(ref mut s) = *worker_sink.write() {
                    if let Err(e) = s.send(WorkerInput::StopCollab).await {
                        tracing::warn!("Failed to send StopCollab to worker: {e}");
                    }
                }
            });

            // Delete session record
            if let Some(uri) = session_uri.peek().clone() {
                let fetcher = fetcher_for_cleanup.clone();
                spawn(async move {
                    if let Err(e) = fetcher.delete_collab_session(&uri).await {
                        tracing::warn!("Failed to delete session record: {e}");
                    }
                });
            }
        });
    }
    // Render children - this component is a wrapper that provides context
    rsx! { {props.children} }
}
