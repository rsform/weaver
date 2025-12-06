//! CollabSession - per-resource gossip session for real-time collaboration.

use std::sync::Arc;

use iroh::EndpointId;
use iroh_gossip::api::{Event, GossipReceiver, GossipSender};
use miette::Diagnostic;
use n0_future::StreamExt;
use n0_future::boxed::BoxStream;
use n0_future::stream;

use super::{CollabMessage, CollabNode};

/// Topic ID for a gossip session - derived from resource URI.
pub type TopicId = iroh_gossip::TopicId;

/// Error type for session operations
#[derive(Debug, thiserror::Error, Diagnostic)]
#[diagnostic(code(weaver::transport::session))]
pub enum SessionError {
    #[error("failed to subscribe to topic")]
    Subscribe(#[source] Box<dyn std::error::Error + Send + Sync>),

    #[error("failed to broadcast message")]
    Broadcast(#[source] Box<dyn std::error::Error + Send + Sync>),

    #[error("failed to decode message")]
    Decode(#[source] Box<dyn std::error::Error + Send + Sync>),

    #[error("session closed")]
    Closed,
}

/// Events emitted by a collaboration session.
#[derive(Debug, Clone)]
pub enum SessionEvent {
    /// A collaborator joined the session
    PeerJoined(EndpointId),

    /// A collaborator left the session
    PeerLeft(EndpointId),

    /// Received a collaboration message from a peer
    Message {
        from: EndpointId,
        message: CollabMessage,
    },

    /// We successfully joined the gossip swarm
    Joined,
}

/// A collaboration session for a specific resource.
///
/// Each session manages gossip subscriptions for one resource (e.g., one notebook).
/// Create via `CollabSession::join()`.
pub struct CollabSession {
    topic: TopicId,
    sender: GossipSender,
    #[allow(dead_code)]
    node: Arc<CollabNode>,
}

impl CollabSession {
    /// Derive a topic ID from a resource identifier.
    ///
    /// We use blake3 hash of the AT-URI to get a stable 32-byte topic ID.
    /// Format: `at://{did}/{collection}/{rkey}`
    pub fn topic_from_uri(uri: &str) -> TopicId {
        let hash = blake3::hash(uri.as_bytes());
        TopicId::from_bytes(*hash.as_bytes())
    }

    /// Join a collaboration session for a resource.
    ///
    /// Returns the session handle and a stream for receiving events.
    /// Bootstrap peers are NodeIds of collaborators discovered from session records.
    pub async fn join(
        node: Arc<CollabNode>,
        topic: TopicId,
        bootstrap_peers: Vec<EndpointId>,
    ) -> Result<(Self, BoxStream<SessionEvent>), SessionError> {
        // Subscribe to the gossip topic
        let (sender, receiver) = node
            .gossip()
            .subscribe(topic, bootstrap_peers)
            .await
            .map_err(|e| SessionError::Subscribe(Box::new(e)))?
            .split();

        let session = Self {
            topic,
            sender,
            node: node.clone(),
        };

        // Create event stream from the gossip receiver
        let event_stream = Self::event_stream(receiver);

        Ok((session, event_stream))
    }

    /// Convert gossip receiver into a stream of session events.
    fn event_stream(receiver: GossipReceiver) -> BoxStream<SessionEvent> {
        let stream = stream::unfold(receiver, |mut receiver| async move {
            loop {
                match receiver.next().await {
                    Some(Ok(event)) => {
                        let session_event = match event {
                            Event::NeighborUp(peer) => SessionEvent::PeerJoined(peer),
                            Event::NeighborDown(peer) => SessionEvent::PeerLeft(peer),
                            Event::Received(msg) => match CollabMessage::from_bytes(&msg.content) {
                                Ok(message) => SessionEvent::Message {
                                    from: msg.delivered_from,
                                    message,
                                },
                                Err(e) => {
                                    tracing::warn!(?e, "failed to decode collab message");
                                    continue;
                                }
                            },
                            Event::Lagged => {
                                tracing::warn!("gossip receiver lagged, some messages may be lost");
                                continue;
                            }
                        };
                        return Some((session_event, receiver));
                    }
                    Some(Err(e)) => {
                        tracing::warn!(?e, "gossip receiver error");
                        continue;
                    }
                    None => return None,
                }
            }
        });

        Box::pin(stream)
    }

    /// Broadcast a message to all peers in the session.
    pub async fn broadcast(&self, message: &CollabMessage) -> Result<(), SessionError> {
        let bytes = message
            .to_bytes()
            .map_err(|e| SessionError::Broadcast(Box::new(e)))?;

        self.sender
            .broadcast(bytes.into())
            .await
            .map_err(|e| SessionError::Broadcast(Box::new(e)))?;

        Ok(())
    }

    /// Get the topic ID for this session.
    pub fn topic(&self) -> TopicId {
        self.topic
    }
}
