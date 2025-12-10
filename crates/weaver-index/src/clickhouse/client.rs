use crate::config::ClickHouseConfig;
use crate::error::{ClickHouseError, IndexError};
use clickhouse::Row;
use clickhouse::inserter::Inserter;

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
            .with_password(&config.password);

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
