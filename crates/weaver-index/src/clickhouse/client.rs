use std::time::Duration;

use crate::config::ClickHouseConfig;
use crate::error::{ClickHouseError, IndexError};
use clickhouse::Row;
use clickhouse::inserter::Inserter;
use serde::Deserialize;

/// ClickHouse client wrapper with connection pooling and batched inserts
pub struct Client {
    inner: clickhouse::Client,
}

impl Client {
    /// Create a new client from configuration
    pub fn new(config: &ClickHouseConfig) -> Result<Self, IndexError> {
        let inner = clickhouse::Client::default()
            .with_url(config.url.as_str())
            .with_database(&config.database)
            .with_user(&config.user)
            .with_password(&config.password)
            // Enable JSON type support (treated as string at transport level)
            .with_option("allow_experimental_json_type", "1")
            .with_option("input_format_binary_read_json_as_string", "1")
            .with_option("output_format_binary_write_json_as_string", "1")
            .with_option("send_timeout", "120")
            .with_option("receive_timeout", "120");

        Ok(Self { inner })
    }

    /// Execute a DDL query (CREATE TABLE, etc.)
    pub async fn execute(&self, query: &str) -> Result<(), IndexError> {
        self.inner
            .query(query)
            .execute()
            .await
            .map_err(|e| ClickHouseError::Query {
                message: "DDL execution failed".into(),
                source: e,
            })?;
        Ok(())
    }

    /// Create a batched inserter for a table
    ///
    /// The inserter accumulates rows and flushes them in batches for efficiency.
    pub fn inserter<T: Row>(&self, table: &str) -> Inserter<T> {
        self.inner
            .inserter(table)
            .with_max_rows(1000)
            .with_period_bias(0.1)
            .with_period(Some(Duration::from_secs(1)))
            .with_max_bytes(1_048_576)
    }

    /// Query table sizes from system.parts
    ///
    /// Returns (table_name, compressed_bytes, uncompressed_bytes, row_count)
    pub async fn table_sizes(&self, tables: &[&str]) -> Result<Vec<TableSize>, IndexError> {
        let table_list = tables
            .iter()
            .map(|t| format!("'{}'", t))
            .collect::<Vec<_>>()
            .join(", ");

        let query = format!(
            r#"
            SELECT
                table,
                sum(bytes_on_disk) as compressed_bytes,
                sum(data_uncompressed_bytes) as uncompressed_bytes,
                sum(rows) as row_count
            FROM system.parts
            WHERE table IN ({})
              AND active
            GROUP BY table
            "#,
            table_list
        );

        let rows = self
            .inner
            .query(&query)
            .fetch_all::<TableSize>()
            .await
            .map_err(|e| ClickHouseError::Query {
                message: "failed to query table sizes".into(),
                source: e,
            })?;

        Ok(rows)
    }

    /// Get reference to inner client for advanced operations
    pub fn inner(&self) -> &clickhouse::Client {
        &self.inner
    }

    /// Get a single record by (did, collection, rkey)
    ///
    /// Returns the latest non-deleted version from raw_records.
    pub async fn get_record(
        &self,
        did: &str,
        collection: &str,
        rkey: &str,
    ) -> Result<Option<RecordRow>, IndexError> {
        // FINAL ensures ReplacingMergeTree deduplication is applied
        // Order by event_time first (firehose data wins), then indexed_at as tiebreaker
        // Include deletes so we can return not-found for deleted records
        let query = r#"
            SELECT cid, record, operation
            FROM raw_records FINAL
            WHERE did = ?
              AND collection = ?
              AND rkey = ?
            ORDER BY event_time DESC, indexed_at DESC
            LIMIT 1
        "#;

        let row = self
            .inner
            .query(query)
            .bind(did)
            .bind(collection)
            .bind(rkey)
            .fetch_optional::<RecordRow>()
            .await
            .map_err(|e| ClickHouseError::Query {
                message: "failed to get record".into(),
                source: e,
            })?;

        Ok(row)
    }

    /// Insert a single record (for cache-on-miss)
    ///
    /// Used when fetching a record from upstream that wasn't in our cache.
    pub async fn insert_record(
        &self,
        did: &str,
        collection: &str,
        rkey: &str,
        cid: &str,
        record_json: &str,
    ) -> Result<(), IndexError> {
        use crate::clickhouse::schema::RawRecordInsert;
        use chrono::DateTime;
        use smol_str::SmolStr;

        let row = RawRecordInsert {
            did: SmolStr::new(did),
            collection: SmolStr::new(collection),
            rkey: SmolStr::new(rkey),
            cid: SmolStr::new(cid),
            rev: SmolStr::new_static(""), // Unknown from upstream fetch
            record: SmolStr::new(record_json),
            operation: SmolStr::new_static("cache"), // Distinguish from firehose ops
            seq: 0,                                  // Not from firehose
            event_time: DateTime::UNIX_EPOCH,        // Sort behind canonical firehose data
            is_live: false,                          // Fetched on-demand, not from firehose
        };

        let mut insert = self
            .inner
            .insert::<RawRecordInsert>("raw_records")
            .await
            .map_err(|e| ClickHouseError::Insert {
                message: "failed to create insert".into(),
                source: e,
            })?;

        insert
            .write(&row)
            .await
            .map_err(|e| ClickHouseError::Insert {
                message: "failed to write record".into(),
                source: e,
            })?;

        insert.end().await.map_err(|e| ClickHouseError::Insert {
            message: "failed to flush insert".into(),
            source: e,
        })?;

        Ok(())
    }

    /// List records for a repo+collection
    ///
    /// Returns non-deleted records ordered by rkey, with cursor-based pagination.
    pub async fn list_records(
        &self,
        did: &str,
        collection: &str,
        limit: u32,
        cursor: Option<&str>,
        reverse: bool,
    ) -> Result<Vec<RecordListRow>, IndexError> {
        let order = if reverse { "DESC" } else { "ASC" };
        let cursor_op = if reverse { "<" } else { ">" };

        // Build query with optional cursor
        let query = if cursor.is_some() {
            format!(
                r#"
                SELECT rkey, cid, record
                FROM raw_records FINAL
                WHERE did = ?
                  AND collection = ?
                  AND rkey {cursor_op} ?
                  AND operation != 'delete'
                ORDER BY rkey {order}
                LIMIT ?
                "#,
            )
        } else {
            format!(
                r#"
                SELECT rkey, cid, record
                FROM raw_records FINAL
                WHERE did = ?
                  AND collection = ?
                  AND operation != 'delete'
                ORDER BY rkey {order}
                LIMIT ?
                "#,
            )
        };

        let mut q = self.inner.query(&query).bind(did).bind(collection);

        if let Some(cursor_rkey) = cursor {
            q = q.bind(cursor_rkey);
        }

        let rows = q
            .bind(limit)
            .fetch_all::<RecordListRow>()
            .await
            .map_err(|e| ClickHouseError::Query {
                message: "failed to list records".into(),
                source: e,
            })?;

        Ok(rows)
    }
}

/// Table size statistics from system.parts
#[derive(Debug, Clone, Row, serde::Deserialize)]
pub struct TableSize {
    pub table: String,
    pub compressed_bytes: u64,
    pub uncompressed_bytes: u64,
    pub row_count: u64,
}

impl TableSize {
    /// Format compressed size as human-readable string
    pub fn compressed_human(&self) -> String {
        humansize::format_size(self.compressed_bytes, humansize::BINARY)
    }

    /// Format uncompressed size as human-readable string
    pub fn uncompressed_human(&self) -> String {
        humansize::format_size(self.uncompressed_bytes, humansize::BINARY)
    }

    /// Compression ratio (uncompressed / compressed)
    pub fn compression_ratio(&self) -> f64 {
        if self.compressed_bytes == 0 {
            0.0
        } else {
            self.uncompressed_bytes as f64 / self.compressed_bytes as f64
        }
    }
}

/// Single record from raw_records (for getRecord)
#[derive(Debug, Clone, Row, Deserialize)]
pub struct RecordRow {
    pub cid: String,
    pub record: String, // JSON string
    pub operation: String,
}

/// Record with rkey from raw_records (for listRecords)
#[derive(Debug, Clone, Row, Deserialize)]
pub struct RecordListRow {
    pub rkey: String,
    pub cid: String,
    pub record: String, // JSON string
}
