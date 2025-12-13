use std::time::Duration;

use clickhouse::inserter::{Inserter, Quantities};
use smol_str::{SmolStr, ToSmolStr};
use tracing::{debug, warn};

use super::schema::{RawEventDlq, RawRecordInsert, Tables};
use crate::error::{ClickHouseError, IndexError};

/// An inserter wrapper for RawRecordInsert that handles failures gracefully
/// by retrying individual rows and sending failures to a dead-letter queue.
///
/// This is specifically for raw record inserts since that's where untrusted
/// input (arbitrary JSON from the firehose) enters the system.
///
/// When a batch insert fails, this wrapper:
/// 1. Creates a fresh inserter (since the old one is poisoned after error)
/// 2. Retries each pending row individually
/// 3. Sends failures to the DLQ with error details and the original row data
/// 4. Continues processing without crashing
pub struct ResilientRecordInserter {
    client: clickhouse::Client,
    inner: Inserter<RawRecordInsert>,
    pending: Vec<RawRecordInsert>,
    dlq: Inserter<RawEventDlq>,
    config: InserterConfig,
}

/// Configuration for the inserter thresholds
#[derive(Clone)]
pub struct InserterConfig {
    pub max_rows: u64,
    pub max_bytes: u64,
    pub period: Option<Duration>,
    pub period_bias: f64,
}

impl Default for InserterConfig {
    fn default() -> Self {
        Self {
            max_rows: 1000,
            max_bytes: 1_048_576, // 1MB
            period: Some(Duration::from_secs(1)),
            period_bias: 0.1,
        }
    }
}

impl ResilientRecordInserter {
    /// Create a new resilient inserter for raw records
    pub fn new(client: clickhouse::Client, config: InserterConfig) -> Self {
        let inner = Self::create_inserter(&client, &config);
        let dlq = Self::create_dlq_inserter(&client, &config);

        Self {
            client,
            inner,
            pending: Vec::new(),
            dlq,
            config,
        }
    }

    fn create_inserter(
        client: &clickhouse::Client,
        config: &InserterConfig,
    ) -> Inserter<RawRecordInsert> {
        let mut inserter = client
            .inserter(Tables::RAW_RECORDS)
            .with_max_rows(config.max_rows)
            .with_max_bytes(config.max_bytes)
            .with_period_bias(config.period_bias);

        if let Some(period) = config.period {
            inserter = inserter.with_period(Some(period));
        }

        inserter
    }

    fn create_dlq_inserter(
        client: &clickhouse::Client,
        config: &InserterConfig,
    ) -> Inserter<RawEventDlq> {
        let mut inserter = client
            .inserter(Tables::RAW_EVENTS_DLQ)
            .with_max_rows(config.max_rows)
            .with_max_bytes(config.max_bytes)
            .with_period_bias(config.period_bias);

        if let Some(period) = config.period {
            inserter = inserter.with_period(Some(period));
        }

        inserter
    }

    /// Write a row to the inserter
    ///
    /// The row is buffered both in the underlying inserter and in our
    /// pending queue for retry on failure.
    pub async fn write(&mut self, row: RawRecordInsert) -> Result<(), IndexError> {
        self.inner
            .write(&row)
            .await
            .map_err(|e| ClickHouseError::Insert {
                message: "write failed".into(),
                source: e,
            })?;

        self.pending.push(row);
        Ok(())
    }

    /// Commit pending data if thresholds are met
    ///
    /// On success, clears the pending buffer if rows were actually flushed.
    /// On failure, retries rows individually and sends failures to DLQ.
    pub async fn commit(&mut self) -> Result<Quantities, IndexError> {
        match self.inner.commit().await {
            Ok(q) => {
                if q.rows > 0 {
                    debug!(
                        rows = q.rows,
                        bytes = q.bytes,
                        "batch committed successfully"
                    );
                    self.pending.clear();
                }
                Ok(q)
            }
            Err(e) => {
                warn!(
                    error = ?e,
                    pending = self.pending.len(),
                    "batch commit failed, retrying individually"
                );
                self.handle_batch_failure(e).await
            }
        }
    }

    /// Force commit all pending data
    ///
    /// Same semantics as commit() but unconditionally flushes.
    pub async fn force_commit(&mut self) -> Result<Quantities, IndexError> {
        match self.inner.force_commit().await {
            Ok(q) => {
                if q.rows > 0 {
                    debug!(
                        rows = q.rows,
                        bytes = q.bytes,
                        "batch force-committed successfully"
                    );
                    self.pending.clear();
                }
                Ok(q)
            }
            Err(e) => {
                warn!(
                    error = ?e,
                    pending = self.pending.len(),
                    "batch force-commit failed, retrying individually"
                );
                self.handle_batch_failure(e).await
            }
        }
    }

    /// End the inserter, flushing all remaining data
    ///
    /// Consumes self. On failure, retries rows individually.
    pub async fn end(mut self) -> Result<Quantities, IndexError> {
        // Take ownership of inner to end it
        let inner_result = self.inner.end().await;

        match inner_result {
            Ok(q) => {
                debug!(
                    rows = q.rows,
                    bytes = q.bytes,
                    "inserter ended successfully"
                );
                // Flush DLQ too
                self.dlq.end().await.map_err(|e| ClickHouseError::Insert {
                    message: "DLQ end failed".into(),
                    source: e,
                })?;
                Ok(q)
            }
            Err(e) => {
                warn!(
                    error = ?e,
                    pending = self.pending.len(),
                    "inserter end failed, retrying individually"
                );
                // Need a fresh inserter for recovery since old one is consumed
                self.inner = Self::create_inserter(&self.client, &self.config);
                let result = self.handle_batch_failure(e).await;
                // Flush DLQ
                self.dlq.end().await.map_err(|e| ClickHouseError::Insert {
                    message: "DLQ end failed".into(),
                    source: e,
                })?;
                result
            }
        }
    }

    /// Get statistics on pending (unbuffered) data in the underlying inserter
    pub fn pending(&self) -> &Quantities {
        self.inner.pending()
    }

    /// Get count of rows in our retry buffer
    pub fn pending_retry_count(&self) -> usize {
        self.pending.len()
    }

    /// Handle a batch failure by retrying rows individually
    async fn handle_batch_failure(
        &mut self,
        original_error: clickhouse::error::Error,
    ) -> Result<Quantities, IndexError> {
        // Take pending rows
        let rows = std::mem::take(&mut self.pending);
        let total = rows.len();

        if rows.is_empty() {
            // Nothing to retry, just propagate the error context
            return Err(ClickHouseError::Insert {
                message: "batch failed with no pending rows".into(),
                source: original_error,
            }
            .into());
        }

        // Create fresh inserter (old one is poisoned after error)
        self.inner = Self::create_inserter(&self.client, &self.config);

        let mut succeeded = 0u64;
        let mut failed = 0u64;

        for row in rows {
            match self.try_single_insert(&row).await {
                Ok(()) => {
                    succeeded += 1;
                }
                Err(e) => {
                    failed += 1;
                    warn!(
                        did = %row.did,
                        collection = %row.collection,
                        rkey = %row.rkey,
                        seq = row.seq,
                        error = ?e,
                        "row insert failed, sending to DLQ"
                    );
                    self.send_to_dlq(&row, &e).await?;
                }
            }
        }

        debug!(total, succeeded, failed, "batch failure recovery complete");

        Ok(Quantities {
            rows: succeeded,
            bytes: 0,
            transactions: 0,
        })
    }

    /// Try to insert a single row using a fresh one-shot inserter
    async fn try_single_insert(
        &self,
        row: &RawRecordInsert,
    ) -> Result<(), clickhouse::error::Error> {
        let mut inserter: Inserter<RawRecordInsert> =
            self.client.inserter(Tables::RAW_RECORDS).with_max_rows(1);

        inserter.write(row).await?;
        inserter.end().await?;
        Ok(())
    }

    /// Send a failed row to the dead-letter queue
    async fn send_to_dlq(
        &mut self,
        row: &RawRecordInsert,
        error: &clickhouse::error::Error,
    ) -> Result<(), IndexError> {
        let raw_data = serde_json::to_string(row)
            .unwrap_or_else(|e| format!("{{\"serialization_error\": \"{}\"}}", e));

        let dlq_row = RawEventDlq {
            event_type: row.operation.clone(),
            raw_data: raw_data.to_smolstr(),
            error_message: error.to_smolstr(),
            seq: row.seq,
        };

        self.dlq
            .write(&dlq_row)
            .await
            .map_err(|e| ClickHouseError::Insert {
                message: "DLQ write failed".into(),
                source: e,
            })?;

        // Force commit DLQ to ensure failures are persisted immediately
        self.dlq
            .force_commit()
            .await
            .map_err(|e| ClickHouseError::Insert {
                message: "DLQ commit failed".into(),
                source: e,
            })?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    // TODO: Add tests with mock clickhouse client
}
