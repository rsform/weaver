//! CollabSession - per-resource gossip session for real-time collaboration.

use std::sync::Arc;

use iroh::EndpointId;
use iroh_gossip::api::{Event, GossipReceiver, GossipSender};
use miette::Diagnostic;
use n0_future::StreamExt;
use n0_future::boxed::BoxStream;
use n0_future::stream;

use super::{CollabMessage, CollabNode, SignedMessage};

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
    ) -> Result<(Self, BoxStream<Result<SessionEvent, SessionError>>), SessionError> {
        tracing::info!(
            topic = ?topic,
            bootstrap_count = bootstrap_peers.len(),
            "CollabSession: joining topic"
        );

        for peer in &bootstrap_peers {
            tracing::debug!(peer = %peer, "CollabSession: bootstrap peer");
        }

        // Subscribe to the gossip topic
        let (sender, receiver) = node
            .gossip()
            .subscribe_and_join(topic, bootstrap_peers)
            .await
            .map_err(|e| SessionError::Subscribe(Box::new(e)))?
            .split();

        tracing::info!("CollabSession: subscribed to gossip topic");

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
    fn event_stream(receiver: GossipReceiver) -> BoxStream<Result<SessionEvent, SessionError>> {
        let stream = stream::try_unfold(receiver, |mut receiver| async move {
            loop {
                let Some(event) = receiver.try_next().await.map_err(|e| {
                    tracing::error!(?e, "CollabSession: gossip receiver error");
                    SessionError::Decode(Box::new(e))
                })?
                else {
                    tracing::debug!("CollabSession: gossip stream ended");
                    return Ok(None);
                };

                tracing::debug!(?event, "CollabSession: raw gossip event");
                let session_event = match event {
                    Event::NeighborUp(peer) => {
                        tracing::info!(peer = %peer, "CollabSession: neighbor up");
                        SessionEvent::PeerJoined(peer)
                    }
                    Event::NeighborDown(peer) => {
                        tracing::info!(peer = %peer, "CollabSession: neighbor down");
                        SessionEvent::PeerLeft(peer)
                    }
                    Event::Received(msg) => {
                        tracing::debug!(
                            from = %msg.delivered_from,
                            bytes = msg.content.len(),
                            "CollabSession: received message"
                        );
                        match SignedMessage::decode_and_verify(&msg.content) {
                            Ok(received) => {
                                // Verify claimed sender matches transport sender
                                if received.from != msg.delivered_from {
                                    tracing::warn!(
                                        claimed = %received.from,
                                        transport = %msg.delivered_from,
                                        "sender mismatch - possible spoofing attempt"
                                    );
                                    continue;
                                }
                                SessionEvent::Message {
                                    from: received.from,
                                    message: received.message,
                                }
                            }
                            Err(e) => {
                                tracing::warn!(?e, "failed to verify/decode signed message");
                                continue;
                            }
                        }
                    }
                    Event::Lagged => {
                        tracing::warn!("gossip receiver lagged, some messages may be lost");
                        continue;
                    }
                };
                break Ok(Some((session_event, receiver)));
            }
        });

        Box::pin(stream)
    }

    /// Broadcast a signed message to all peers in the session.
    pub async fn broadcast(&self, message: &CollabMessage) -> Result<(), SessionError> {
        let bytes = SignedMessage::sign_and_encode(&self.node.secret_key(), message)
            .map_err(|e| SessionError::Broadcast(Box::new(e)))?;

        tracing::debug!(
            bytes = bytes.len(),
            topic = ?self.topic,
            "CollabSession: broadcasting signed message"
        );

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

    /// Add new peers to the gossip session.
    ///
    /// Use this to add peers discovered after initial subscription.
    /// The gossip layer will attempt to connect to these peers.
    pub async fn join_peers(&self, peers: Vec<EndpointId>) -> Result<(), SessionError> {
        if peers.is_empty() {
            return Ok(());
        }
        tracing::info!(
            count = peers.len(),
            "CollabSession: joining additional peers"
        );
        for peer in &peers {
            tracing::debug!(peer = %peer, "CollabSession: adding peer");
        }
        self.sender
            .join_peers(peers)
            .await
            .map_err(|e| SessionError::Subscribe(Box::new(e)))?;
        Ok(())
    }
}
