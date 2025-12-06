//! Wire protocol for collaborative editing messages.

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
        did: String,
        /// Display name for presence UI
        display_name: String,
    },

    /// Collaborator left the session
    Leave {
        /// DID of the leaving user
        did: String,
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
    /// Serialize message to CBOR bytes for wire transmission
    pub fn to_bytes(&self) -> Result<Vec<u8>, postcard::Error> {
        postcard::to_stdvec(self)
    }

    /// Deserialize message from CBOR bytes
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, postcard::Error> {
        postcard::from_bytes(bytes)
    }
}

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
            did: "did:plc:abc123".to_string(),
            display_name: "Alice".to_string(),
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
