use serde::{Deserialize, Serialize};
use smol_str::SmolStr;

/// Event received from tap's websocket channel
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum TapEvent {
    Record(TapRecordEnvelope),
    Identity(TapIdentityEnvelope),
}

impl TapEvent {
    pub fn id(&self) -> u64 {
        match self {
            TapEvent::Record(r) => r.id,
            TapEvent::Identity(i) => i.id,
        }
    }
}

/// Envelope for record events
#[derive(Debug, Clone, Deserialize)]
pub struct TapRecordEnvelope {
    pub id: u64,
    pub record: TapRecordEvent,
}

/// Record event from tap
#[derive(Debug, Clone, Deserialize)]
pub struct TapRecordEvent {
    /// Whether this is a live event (true) or backfill (false)
    pub live: bool,
    /// Repository revision
    pub rev: SmolStr,
    /// DID of the account
    pub did: SmolStr,
    /// Collection NSID (e.g., "app.bsky.feed.post")
    pub collection: SmolStr,
    /// Record key
    pub rkey: SmolStr,
    /// Operation: create, update, delete
    pub action: RecordAction,
    /// Content identifier
    pub cid: Option<SmolStr>,
    /// The actual record data (only present for create/update)
    #[serde(default)]
    pub record: Option<serde_json::Value>,
}

/// Record operation type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RecordAction {
    Create,
    Update,
    Delete,
}

impl RecordAction {
    pub fn as_str(&self) -> &'static str {
        match self {
            RecordAction::Create => "create",
            RecordAction::Update => "update",
            RecordAction::Delete => "delete",
        }
    }
}

/// Envelope for identity events
#[derive(Debug, Clone, Deserialize)]
pub struct TapIdentityEnvelope {
    pub id: u64,
    pub identity: TapIdentityEvent,
}

/// Identity event from tap (handle or status changes)
#[derive(Debug, Clone, Deserialize)]
pub struct TapIdentityEvent {
    pub did: SmolStr,
    pub handle: SmolStr,
    pub is_active: bool,
    pub status: AccountStatus,
}

/// Account status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AccountStatus {
    Active,
    Deactivated,
    Suspended,
    Deleted,
    Takendown,
    #[serde(other)]
    Unknown,
}

/// Ack message to send back to tap
#[derive(Debug, Clone, Serialize)]
pub struct TapAck {
    #[serde(rename = "type")]
    pub msg_type: &'static str,
    pub id: u64,
}

impl TapAck {
    pub fn new(id: u64) -> Self {
        Self {
            msg_type: "ack",
            id,
        }
    }
}
