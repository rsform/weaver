use chrono::{DateTime, Utc};
use clickhouse::Row;

/// Table names for production schema
pub struct Tables;

impl Tables {
    pub const RAW_RECORDS: &'static str = "raw_records";
    pub const RAW_IDENTITY_EVENTS: &'static str = "raw_identity_events";
    pub const RAW_ACCOUNT_EVENTS: &'static str = "raw_account_events";
    pub const RAW_EVENTS_DLQ: &'static str = "raw_events_dlq";
    pub const FIREHOSE_CURSOR: &'static str = "firehose_cursor";
}

/// Row type for raw_records table
/// Schema defined in migrations/clickhouse/001_raw_records.sql
#[derive(Debug, Clone, Row, serde::Serialize, serde::Deserialize)]
pub struct RawRecord {
    pub did: String,
    pub collection: String,
    pub rkey: String,
    pub cid: String,
    pub record: String, // JSON string - ClickHouse JSON type accepts string
    pub operation: String,
    pub seq: u64,
    #[serde(with = "clickhouse::serde::chrono::datetime64::millis")]
    pub event_time: DateTime<Utc>,
}

/// Row type for raw_identity_events table
#[derive(Debug, Clone, Row, serde::Serialize, serde::Deserialize)]
pub struct RawIdentityEvent {
    pub did: String,
    pub handle: String,
    pub seq: u64,
    #[serde(with = "clickhouse::serde::chrono::datetime64::millis")]
    pub event_time: DateTime<Utc>,
}

/// Row type for raw_account_events table
#[derive(Debug, Clone, Row, serde::Serialize, serde::Deserialize)]
pub struct RawAccountEvent {
    pub did: String,
    pub active: u8,
    pub status: String,
    pub seq: u64,
    #[serde(with = "clickhouse::serde::chrono::datetime64::millis")]
    pub event_time: DateTime<Utc>,
}

/// Row type for raw_events_dlq table
#[derive(Debug, Clone, Row, serde::Serialize, serde::Deserialize)]
pub struct RawEventDlq {
    pub event_type: String,
    pub raw_data: String, // JSON string
    pub error_message: String,
    pub seq: u64,
}

/// Row type for firehose_cursor table
#[derive(Debug, Clone, Row, serde::Serialize, serde::Deserialize)]
pub struct FirehoseCursor {
    pub consumer_id: String,
    pub seq: u64,
    #[serde(with = "clickhouse::serde::chrono::datetime64::millis")]
    pub event_time: DateTime<Utc>,
}
