//! Wire protocol for collaborative editing messages.

use jacquard::smol_str::SmolStr;
use serde::{Deserialize, Serialize};

/// Messages exchanged between collaborators over gossip.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CollabMessage {
    /// Loro CRDT update - incremental changes
    LoroUpdate {
        /// Serialized Loro update bytes
        data: Vec<u8>,
        /// Version vector for ordering/deduplication
        version: Vec<(u64, u64)>,
    },

    /// Cursor position update (presence)
    Cursor {
        /// Cursor position in document
        position: usize,
        /// Optional selection range (anchor, head)
        selection: Option<(usize, usize)>,
        /// Assigned collaborator colour (RGBA)
        color: u32,
    },

    /// Collaborator joined the session
    Join {
        /// DID of the joining user
        did: SmolStr,
        /// Display name for presence UI
        display_name: SmolStr,
    },

    /// Collaborator left the session
    Leave {
        /// DID of the leaving user
        did: SmolStr,
    },

    /// Request sync from peers (late joiner)
    SyncRequest {
        /// Version vector of what we already have
        have_version: Vec<(u64, u64)>,
    },

    /// Response to sync request
    SyncResponse {
        /// Loro update/snapshot bytes
        data: Vec<u8>,
        /// True if this is a full snapshot, false if incremental
        is_snapshot: bool,
    },
}

impl CollabMessage {
    /// Serialize message to postcard bytes for wire transmission.
    pub fn to_bytes(&self) -> Result<Vec<u8>, postcard::Error> {
        postcard::to_stdvec(self)
    }

    /// Deserialize message from postcard bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, postcard::Error> {
        postcard::from_bytes(bytes)
    }
}

// ============================================================================
// Signed message wrapper (requires iroh for crypto)
// ============================================================================

#[cfg(feature = "iroh")]
mod signed {
    use super::*;
    use iroh::{PublicKey, SecretKey, Signature};

    /// A signed message wrapper for authenticated transport.
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct SignedMessage {
        /// Sender's public key (also their EndpointId).
        pub from: PublicKey,
        /// The serialized TimestampedMessage (postcard bytes).
        pub data: Vec<u8>,
        /// Ed25519 signature over data.
        pub signature: Signature,
    }

    /// Versioned wire format with timestamp.
    #[derive(Debug, Clone, Serialize, Deserialize)]
    enum WireMessage {
        V0 { timestamp: u64, message: CollabMessage },
    }

    /// A verified message with sender and timestamp info.
    #[derive(Debug, Clone)]
    pub struct ReceivedMessage {
        /// Sender's public key.
        pub from: PublicKey,
        /// When the message was sent (micros since epoch).
        pub timestamp: u64,
        /// The decoded message.
        pub message: CollabMessage,
    }

    /// Error type for signed message operations.
    #[derive(Debug, thiserror::Error)]
    pub enum SignedMessageError {
        #[error("serialization failed: {0}")]
        Serialization(#[from] postcard::Error),
        #[error("signature verification failed")]
        InvalidSignature,
    }

    impl SignedMessage {
        /// Sign a message and encode to bytes for wire transmission.
        pub fn sign_and_encode(
            secret_key: &SecretKey,
            message: &CollabMessage,
        ) -> Result<Vec<u8>, SignedMessageError> {
            use web_time::SystemTime;

            let timestamp = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_micros() as u64;
            let wire = WireMessage::V0 {
                timestamp,
                message: message.clone(),
            };
            let data = postcard::to_stdvec(&wire)?;
            let signature = secret_key.sign(&data);
            let from = secret_key.public();
            let signed = Self {
                from,
                data,
                signature,
            };
            Ok(postcard::to_stdvec(&signed)?)
        }

        /// Decode from bytes and verify signature.
        pub fn decode_and_verify(bytes: &[u8]) -> Result<ReceivedMessage, SignedMessageError> {
            let signed: Self = postcard::from_bytes(bytes)?;
            signed
                .from
                .verify(&signed.data, &signed.signature)
                .map_err(|_| SignedMessageError::InvalidSignature)?;
            let wire: WireMessage = postcard::from_bytes(&signed.data)?;
            let WireMessage::V0 { timestamp, message } = wire;
            Ok(ReceivedMessage {
                from: signed.from,
                timestamp,
                message,
            })
        }
    }
}

#[cfg(feature = "iroh")]
pub use signed::{ReceivedMessage, SignedMessage, SignedMessageError};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_roundtrip_loro_update() {
        let msg = CollabMessage::LoroUpdate {
            data: vec![1, 2, 3, 4],
            version: vec![(1, 10), (2, 5)],
        };
        let bytes = msg.to_bytes().unwrap();
        let decoded = CollabMessage::from_bytes(&bytes).unwrap();

        match decoded {
            CollabMessage::LoroUpdate { data, version } => {
                assert_eq!(data, vec![1, 2, 3, 4]);
                assert_eq!(version, vec![(1, 10), (2, 5)]);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn test_roundtrip_join() {
        let msg = CollabMessage::Join {
            did: "did:plc:abc123".into(),
            display_name: "Alice".into(),
        };
        let bytes = msg.to_bytes().unwrap();
        let decoded = CollabMessage::from_bytes(&bytes).unwrap();

        match decoded {
            CollabMessage::Join { did, display_name } => {
                assert_eq!(did, "did:plc:abc123");
                assert_eq!(display_name, "Alice");
            }
            _ => panic!("wrong variant"),
        }
    }
}
