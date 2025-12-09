//! Real-time collaboration debug state.
//!
//! This module provides CollabDebugState which is set as context by
//! the CollabCoordinator component for display in the editor debug panel.

use dioxus::prelude::*;

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

/// Hook to get the collab debug state signal.
/// Returns None if called outside CollabCoordinator.
pub fn try_use_collab_debug() -> Option<Signal<CollabDebugState>> {
    try_use_context::<Signal<CollabDebugState>>()
}
