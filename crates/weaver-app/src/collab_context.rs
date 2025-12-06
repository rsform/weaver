//! Real-time collaboration context for P2P editing sessions.
//!
//! This module provides the CollabNode as a Dioxus context, allowing editor
//! components to join gossip sessions for real-time collaboration.
//!
//! The CollabNode is only active in WASM builds where iroh works via relays.

use dioxus::prelude::*;
use std::sync::Arc;
use weaver_common::transport::CollabNode;

/// Debug state for the collab session, displayed in editor debug panel.
#[derive(Clone, Default)]
pub struct CollabDebugState {
    /// Our node ID
    pub node_id: Option<String>,
    /// Our relay URL
    pub relay_url: Option<String>,
    /// URI of our published session record
    pub session_record_uri: Option<String>,
    /// Number of discovered peers
    pub discovered_peers: usize,
    /// Number of connected peers
    pub connected_peers: usize,
    /// Whether we've joined the gossip swarm
    pub is_joined: bool,
    /// Last error message
    pub last_error: Option<String>,
}

/// Context state for the collaboration node.
///
/// This is provided as a Dioxus context and can be accessed by editor components
/// to join/leave collaborative editing sessions.
#[derive(Clone)]
pub struct CollabContext {
    /// The collaboration node, if successfully spawned.
    /// None while loading or if spawn failed.
    pub node: Option<Arc<CollabNode>>,
    /// Error message if spawn failed.
    pub error: Option<String>,
}

impl Default for CollabContext {
    fn default() -> Self {
        Self {
            node: None,
            error: None,
        }
    }
}

/// Provider component that spawns the CollabNode and provides it as context.
///
/// Should be placed near the root of the app, wrapping any components that
/// need access to real-time collaboration.
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
#[component]
pub fn CollabProvider(children: Element) -> Element {
    let mut collab_ctx = use_signal(CollabContext::default);
    let debug_state = use_signal(CollabDebugState::default);

    // Spawn the CollabNode on mount
    let _spawn_result = use_resource(move || async move {
        tracing::info!("Spawning CollabNode...");

        match CollabNode::spawn(None).await {
            Ok(node) => {
                tracing::info!(node_id = %node.node_id_string(), "CollabNode spawned");
                collab_ctx.set(CollabContext {
                    node: Some(node),
                    error: None,
                });
            }
            Err(e) => {
                tracing::error!("Failed to spawn CollabNode: {}", e);
                collab_ctx.set(CollabContext {
                    node: None,
                    error: Some(e.to_string()),
                });
            }
        }
    });

    // Provide the contexts
    use_context_provider(|| collab_ctx);
    use_context_provider(|| debug_state);

    rsx! { {children} }
}

/// No-op provider for non-WASM builds.
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
#[component]
pub fn CollabProvider(children: Element) -> Element {
    // On server/native, provide an empty context (collab happens in browser)
    let collab_ctx = use_signal(CollabContext::default);
    let debug_state = use_signal(CollabDebugState::default);
    use_context_provider(|| collab_ctx);
    use_context_provider(|| debug_state);
    rsx! { {children} }
}

/// Hook to get the CollabNode from context.
///
/// Returns None if the node hasn't spawned yet or failed to spawn.
pub fn use_collab_node() -> Option<Arc<CollabNode>> {
    let ctx = use_context::<Signal<CollabContext>>();
    ctx.read().node.clone()
}

/// Hook to check if collab is available.
pub fn use_collab_available() -> bool {
    let ctx = use_context::<Signal<CollabContext>>();
    ctx.read().node.is_some()
}

/// Hook to get the collab debug state signal.
/// Returns None if called outside CollabProvider.
pub fn try_use_collab_debug() -> Option<Signal<CollabDebugState>> {
    try_use_context::<Signal<CollabDebugState>>()
}
