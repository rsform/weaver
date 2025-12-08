//! Presence types for main thread rendering.
//!
//! These types use String node IDs instead of EndpointId,
//! allowing them to be used without the iroh feature.

use serde::{Deserialize, Serialize};

/// A remote collaborator's cursor for rendering.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RemoteCursorInfo {
    /// Node ID as string (z-base32 encoded)
    pub node_id: String,
    /// Character offset in the document
    pub position: usize,
    /// Selection range (anchor, head) if any
    pub selection: Option<(usize, usize)>,
    /// Assigned colour (RGBA)
    pub color: u32,
}

/// Collaborator info for presence display.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CollaboratorInfo {
    /// Node ID as string
    pub node_id: String,
    /// The collaborator's DID
    pub did: String,
    /// Display name for UI
    pub display_name: String,
    /// Assigned colour (RGBA)
    pub color: u32,
    /// Current cursor position (if known)
    pub cursor_position: Option<usize>,
    /// Current selection (if any)
    pub selection: Option<(usize, usize)>,
}

/// Presence update sent from worker to main thread.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PresenceSnapshot {
    /// All known collaborators
    pub collaborators: Vec<CollaboratorInfo>,
    /// Number of connected peers
    pub peer_count: usize,
}

impl Default for PresenceSnapshot {
    fn default() -> Self {
        Self {
            collaborators: Vec::new(),
            peer_count: 0,
        }
    }
}
