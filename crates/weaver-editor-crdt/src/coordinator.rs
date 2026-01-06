//! Collab coordinator types and helpers.
//!
//! Provides shared types for collab coordination that can be used by both
//! Rust UI frameworks (Dioxus) and JS bindings.

use smol_str::SmolStr;

/// Session record TTL in minutes.
pub const SESSION_TTL_MINUTES: u32 = 15;

/// How often to refresh session record (ms).
pub const SESSION_REFRESH_INTERVAL_MS: u32 = 5 * 60 * 1000; // 5 minutes

/// How often to poll for new peers (ms).
pub const PEER_DISCOVERY_INTERVAL_MS: u32 = 30 * 1000; // 30 seconds

/// Coordinator state machine states.
///
/// Tracks the lifecycle of a collab session from initialization through
/// active collaboration. UI can use this to show appropriate status indicators.
#[derive(Debug, Clone, PartialEq)]
pub enum CoordinatorState {
    /// Initial state - waiting for worker to be ready.
    Initializing,
    /// Creating session record on PDS.
    CreatingSession {
        /// The iroh node ID for this session.
        node_id: SmolStr,
        /// Optional relay URL for NAT traversal.
        relay_url: Option<SmolStr>,
    },
    /// Active collab session.
    Active {
        /// The AT URI of the session record on PDS.
        session_uri: SmolStr,
    },
    /// Error state.
    Error(SmolStr),
}

impl Default for CoordinatorState {
    fn default() -> Self {
        Self::Initializing
    }
}

impl CoordinatorState {
    /// Returns true if the coordinator is in an error state.
    pub fn is_error(&self) -> bool {
        matches!(self, Self::Error(_))
    }

    /// Returns true if the coordinator is actively collaborating.
    pub fn is_active(&self) -> bool {
        matches!(self, Self::Active { .. })
    }

    /// Returns the error message if in error state.
    pub fn error_message(&self) -> Option<&str> {
        match self {
            Self::Error(msg) => Some(msg.as_str()),
            _ => None,
        }
    }

    /// Returns the session URI if active.
    pub fn session_uri(&self) -> Option<&str> {
        match self {
            Self::Active { session_uri } => Some(session_uri.as_str()),
            _ => None,
        }
    }
}

/// Compute the gossip topic hash for a resource URI.
///
/// The topic is a blake3 hash of the resource URI bytes, used to identify
/// the gossip swarm for collaborative editing of that resource.
pub fn compute_collab_topic(resource_uri: &str) -> [u8; 32] {
    let hash = weaver_common::blake3::hash(resource_uri.as_bytes());
    *hash.as_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_coordinator_state_default() {
        assert_eq!(CoordinatorState::default(), CoordinatorState::Initializing);
    }

    #[test]
    fn test_coordinator_state_is_error() {
        assert!(!CoordinatorState::Initializing.is_error());
        assert!(CoordinatorState::Error("test".into()).is_error());
    }

    #[test]
    fn test_coordinator_state_is_active() {
        assert!(!CoordinatorState::Initializing.is_active());
        assert!(CoordinatorState::Active {
            session_uri: "at://test".into()
        }
        .is_active());
    }

    #[test]
    fn test_compute_collab_topic_deterministic() {
        let topic1 = compute_collab_topic("at://did:plc:test/app.weaver.notebook.entry/abc");
        let topic2 = compute_collab_topic("at://did:plc:test/app.weaver.notebook.entry/abc");
        assert_eq!(topic1, topic2);
    }

    #[test]
    fn test_compute_collab_topic_different_uris() {
        let topic1 = compute_collab_topic("at://did:plc:test/app.weaver.notebook.entry/abc");
        let topic2 = compute_collab_topic("at://did:plc:test/app.weaver.notebook.entry/def");
        assert_ne!(topic1, topic2);
    }
}
