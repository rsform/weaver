//! Real-time collaboration transport layer.
//!
//! Core message types are always available. iroh-based networking
//! requires the `iroh` feature.

mod messages;
mod presence_types;

#[cfg(feature = "iroh")]
mod discovery;
#[cfg(feature = "iroh")]
mod node;
#[cfg(feature = "iroh")]
mod presence;
#[cfg(feature = "iroh")]
mod session;

// Always available - wire protocol
pub use messages::CollabMessage;
pub use presence_types::{CollaboratorInfo, PresenceSnapshot, RemoteCursorInfo};

// iroh feature - networking
#[cfg(feature = "iroh")]
pub use discovery::{node_id_to_string, parse_node_id, DiscoveredPeer, DiscoveryError};
#[cfg(feature = "iroh")]
pub use iroh::EndpointId;
#[cfg(feature = "iroh")]
pub use messages::{ReceivedMessage, SignedMessage, SignedMessageError};
#[cfg(feature = "iroh")]
pub use node::{CollabNode, TransportError};
#[cfg(feature = "iroh")]
pub use presence::{Collaborator, PresenceTracker, RemoteCursor};
#[cfg(feature = "iroh")]
pub use session::{CollabSession, SessionError, SessionEvent, TopicId};
