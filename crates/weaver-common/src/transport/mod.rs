//! Real-time collaboration transport layer using iroh P2P networking.
//!
//! This module provides the infrastructure for real-time collaborative editing:
//! - `CollabNode`: iroh endpoint + gossip router (one per app instance)
//! - `CollabSession`: per-resource session management
//! - `CollabMessage`: wire protocol for CRDT updates, cursors, presence
//! - `discovery`: utilities for parsing NodeIds from session records

mod discovery;
mod messages;
mod node;
mod presence;
mod session;

pub use discovery::{node_id_to_string, parse_node_id, DiscoveredPeer, DiscoveryError};
pub use iroh::EndpointId;
pub use messages::{CollabMessage, ReceivedMessage, SignedMessage, SignedMessageError};
pub use node::{CollabNode, TransportError};
pub use presence::{Collaborator, PresenceTracker, RemoteCursor};
pub use session::{CollabSession, SessionError, SessionEvent, TopicId};
