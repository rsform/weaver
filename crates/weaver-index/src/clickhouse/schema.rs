use chrono::{DateTime, Utc};
use clickhouse::Row;
use smol_str::SmolStr;

/// Table names for production schema
pub struct Tables;

impl Tables {
    pub const RAW_RECORDS: &'static str = "raw_records";
    pub const RAW_IDENTITY_EVENTS: &'static str = "raw_identity_events";
    pub const RAW_ACCOUNT_EVENTS: &'static str = "raw_account_events";
    pub const RAW_EVENTS_DLQ: &'static str = "raw_events_dlq";
    pub const FIREHOSE_CURSOR: &'static str = "firehose_cursor";
    pub const ACCOUNT_REV_STATE: &'static str = "account_rev_state";
    pub const ACCOUNT_REV_STATE_MV: &'static str = "account_rev_state_mv";
    pub const MIGRATIONS: &'static str = "_migrations";

    /// All tables and views in drop order (MVs before their source tables)
    pub const ALL: &'static [&'static str] = &[
        Self::ACCOUNT_REV_STATE_MV, // MV first, depends on raw_records
        Self::ACCOUNT_REV_STATE,
        Self::RAW_RECORDS,
        Self::RAW_IDENTITY_EVENTS,
        Self::RAW_ACCOUNT_EVENTS,
        Self::RAW_EVENTS_DLQ,
        Self::FIREHOSE_CURSOR,
        Self::MIGRATIONS,
    ];
}

/// Validation states for records
pub mod validation {
    #[allow(dead_code)]
    pub const UNCHECKED: &str = "unchecked";
    #[allow(dead_code)]
    pub const VALID: &str = "valid";
    #[allow(dead_code)]
    pub const INVALID_REV: &str = "invalid_rev";
    #[allow(dead_code)]
    pub const INVALID_GAP: &str = "invalid_gap";
    #[allow(dead_code)]
    pub const INVALID_ACCOUNT: &str = "invalid_account";
}

/// Row type for raw_records table
/// Schema defined in migrations/clickhouse/001_raw_records.sql
#[derive(Debug, Clone, Row, serde::Serialize, serde::Deserialize)]
pub struct RawRecordInsert {
    pub did: SmolStr,
    pub collection: SmolStr,
    pub rkey: SmolStr,
    pub cid: SmolStr,
    pub rev: SmolStr,
    pub record: SmolStr, // JSON string - ClickHouse JSON type accepts string
    pub operation: SmolStr,
    pub seq: u64,
    #[serde(with = "clickhouse::serde::chrono::datetime64::millis")]
    pub event_time: DateTime<Utc>,
    /// Whether this came from live firehose (true) or backfill (false)
    pub is_live: bool,
    // Note: indexed_at has DEFAULT now64(3), omit from insert
    // Note: validation_state has DEFAULT 'unchecked', omit from insert
}

/// Row type for raw_identity_events table
#[derive(Debug, Clone, Row, serde::Serialize, serde::Deserialize)]
pub struct RawIdentityEvent {
    pub did: SmolStr,
    pub handle: SmolStr,
    pub seq: u64,
    #[serde(with = "clickhouse::serde::chrono::datetime64::millis")]
    pub event_time: DateTime<Utc>,
}

/// Row type for raw_account_events table
#[derive(Debug, Clone, Row, serde::Serialize, serde::Deserialize)]
pub struct RawAccountEvent {
    pub did: SmolStr,
    pub active: u8,
    pub status: SmolStr,
    pub seq: u64,
    #[serde(with = "clickhouse::serde::chrono::datetime64::millis")]
    pub event_time: DateTime<Utc>,
}

/// Row type for raw_events_dlq table
#[derive(Debug, Clone, Row, serde::Serialize, serde::Deserialize)]
pub struct RawEventDlq {
    pub event_type: SmolStr,
    pub raw_data: SmolStr, // JSON string
    pub error_message: SmolStr,
    pub seq: u64,
}

/// Row type for firehose_cursor table
#[derive(Debug, Clone, Row, serde::Serialize, serde::Deserialize)]
pub struct FirehoseCursor {
    pub consumer_id: SmolStr,
    pub seq: u64,
    #[serde(with = "clickhouse::serde::chrono::datetime64::millis")]
    pub event_time: DateTime<Utc>,
}

/// Row type for reading finalized account_rev_state
/// Query with: SELECT did, argMaxMerge(last_rev), argMaxMerge(last_cid), maxMerge(last_seq), maxMerge(last_event_time) FROM account_rev_state GROUP BY did
#[derive(Debug, Clone, Row, serde::Serialize, serde::Deserialize)]
pub struct AccountRevState {
    pub did: SmolStr,
    pub last_rev: SmolStr,
    pub last_cid: SmolStr,
    pub last_seq: u64,
    #[serde(with = "clickhouse::serde::chrono::datetime64::millis")]
    pub last_event_time: DateTime<Utc>,
}
