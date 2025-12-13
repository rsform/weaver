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
            max_rows: 10000,
            max_bytes: 1_048_576 * 2, // 1MB
            period: Some(Duration::from_secs(2)),
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

    /// Get time remaining until the next scheduled flush
    pub fn time_left(&mut self) -> Option<std::time::Duration> {
        self.inner.time_left()
    }

    /// Handle a batch failure by retrying rows
    ///
    /// Attempts to extract the failing row number from the error message.
    /// If found, batches rows before/after the failure point for efficiency.
    /// Falls back to individual retries if row number unavailable or sub-batches fail.
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

        // Try to extract failing row number for smart retry
        if let Some(failing_row) = extract_failing_row(&original_error) {
            // Subtract 2 for safety margin (1-indexed to 0-indexed, plus buffer)
            let safe_row = failing_row.saturating_sub(2);

            if safe_row > 0 && safe_row < total {
                debug!(
                    failing_row,
                    safe_row, total, "extracted failing row, attempting smart retry"
                );
                return self.smart_retry(rows, safe_row, &original_error).await;
            }
        }

        // Fall back to individual retries
        debug!(total, "no row number found, retrying individually");
        self.retry_individually(rows).await
    }

    /// Smart retry: batch rows before failure, DLQ the bad row, batch rows after
    async fn smart_retry(
        &mut self,
        rows: Vec<RawRecordInsert>,
        failing_idx: usize,
        original_error: &clickhouse::error::Error,
    ) -> Result<Quantities, IndexError> {
        let total = rows.len();
        let mut succeeded = 0u64;
        let mut failed = 0u64;

        // Try to batch insert rows before the failure point
        if failing_idx > 0 {
            let before = &rows[..failing_idx];
            debug!(count = before.len(), "batch inserting rows before failure");

            match self.batch_insert(before).await {
                Ok(count) => {
                    succeeded += count;
                    debug!(count, "pre-failure batch succeeded");
                }
                Err(e) => {
                    // Sub-batch failed, fall back to individual for this chunk
                    warn!(error = ?e, "pre-failure batch failed, retrying individually");
                    let (s, f) = self.retry_individually_slice(before).await?;
                    succeeded += s;
                    failed += f;
                }
            }
        }

        // Send the failing row (and a couple around it) to DLQ
        let dlq_start = failing_idx;
        let dlq_end = (failing_idx + 3).min(total); // failing row + 2 more for safety
        for row in &rows[dlq_start..dlq_end] {
            warn!(
                did = %row.did,
                collection = %row.collection,
                rkey = %row.rkey,
                seq = row.seq,
                "sending suspected bad row to DLQ"
            );
            self.send_to_dlq(row, original_error).await?;
            failed += 1;
        }

        // Try to batch insert rows after the failure point
        if dlq_end < total {
            let after = &rows[dlq_end..];
            debug!(count = after.len(), "batch inserting rows after failure");

            match self.batch_insert(after).await {
                Ok(count) => {
                    succeeded += count;
                    debug!(count, "post-failure batch succeeded");
                }
                Err(e) => {
                    // Sub-batch failed, fall back to individual for this chunk
                    warn!(error = ?e, "post-failure batch failed, retrying individually");
                    let (s, f) = self.retry_individually_slice(after).await?;
                    succeeded += s;
                    failed += f;
                }
            }
        }

        debug!(total, succeeded, failed, "smart retry complete");

        Ok(Quantities {
            rows: succeeded,
            bytes: 0,
            transactions: 0,
        })
    }

    /// Batch insert a slice of rows using a fresh one-shot inserter
    async fn batch_insert(
        &mut self,
        rows: &[RawRecordInsert],
    ) -> Result<u64, clickhouse::error::Error> {
        batch_insert_rows(&self.client, rows).await
    }

    /// Retry a vec of rows individually, returning (succeeded, failed) counts
    async fn retry_individually(
        &mut self,
        rows: Vec<RawRecordInsert>,
    ) -> Result<Quantities, IndexError> {
        let (succeeded, failed) = self.retry_individually_slice(&rows).await?;

        if failed > 0 {
            warn!(
                succeeded,
                failed, "individual retry had failures sent to DLQ"
            );
        }

        Ok(Quantities {
            rows: succeeded,
            bytes: 0,
            transactions: 0,
        })
    }

    /// Retry a slice of rows individually, returning (succeeded, failed) counts
    async fn retry_individually_slice(
        &mut self,
        rows: &[RawRecordInsert],
    ) -> Result<(u64, u64), IndexError> {
        let total = rows.len();
        let mut succeeded = 0u64;
        let mut failed = 0u64;

        let client = self.client.clone();

        for (i, row) in rows.iter().enumerate() {
            debug!(i, total, did = %row.did, "retrying row individually");
            match try_single_insert(&client, row).await {
                Ok(()) => {
                    succeeded += 1;
                    debug!(i, "row succeeded");
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
                    debug!(i, "sending to DLQ");
                    self.send_to_dlq(row, &e).await?;
                    debug!(i, "DLQ write complete");
                }
            }
        }

        debug!(total, succeeded, failed, "individual retry complete");
        Ok((succeeded, failed))
    }

    /// Send a failed row to the dead-letter queue
    async fn send_to_dlq(
        &mut self,
        row: &RawRecordInsert,
        error: &clickhouse::error::Error,
    ) -> Result<(), IndexError> {
        let raw_data = serde_json::to_string(row)
            .unwrap_or_else(|e| format!("{{\"serialization_error\": \"{}\"}}", e));

        self.write_raw_to_dlq(row.operation.clone(), raw_data, error.to_string(), row.seq)
            .await
    }

    /// Write a pre-insert failure directly to the DLQ
    ///
    /// Use this for failures that happen before we even have a valid RawRecordInsert,
    /// like JSON serialization errors.
    pub async fn write_raw_to_dlq(
        &mut self,
        event_type: SmolStr,
        raw_data: String,
        error_message: String,
        seq: u64,
    ) -> Result<(), IndexError> {
        let dlq_row = RawEventDlq {
            event_type,
            raw_data: raw_data.to_smolstr(),
            error_message: error_message.to_smolstr(),
            seq,
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

/// Try to insert a single row using a fresh one-shot inserter
///
/// Free function to avoid &self borrow across await points (Sync issues)
async fn try_single_insert(
    client: &clickhouse::Client,
    row: &RawRecordInsert,
) -> Result<(), clickhouse::error::Error> {
    let mut inserter: Inserter<RawRecordInsert> =
        client.inserter(Tables::RAW_RECORDS).with_max_rows(1);

    inserter.write(row).await?;
    inserter.force_commit().await?;
    inserter.end().await?;
    Ok(())
}

/// Batch insert rows using a fresh inserter
///
/// Free function to avoid &self borrow across await points (Sync issues)
async fn batch_insert_rows(
    client: &clickhouse::Client,
    rows: &[RawRecordInsert],
) -> Result<u64, clickhouse::error::Error> {
    let mut inserter: Inserter<RawRecordInsert> = client
        .inserter(Tables::RAW_RECORDS)
        .with_max_rows(rows.len() as u64);

    for row in rows {
        inserter.write(row).await?;
    }
    inserter.end().await?;
    Ok(rows.len() as u64)
}

/// Extract the failing row number from a ClickHouse error message
///
/// Looks for patterns like "(at row 791)" in the error text.
/// Returns 1-indexed row number if found.
fn extract_failing_row(error: &clickhouse::error::Error) -> Option<usize> {
    let msg = error.to_string();
    // Look for "(at row N)"
    if let Some(start) = msg.find("(at row ") {
        let rest = &msg[start + 8..];
        if let Some(end) = rest.find(')') {
            return rest[..end].parse().ok();
        }
    }
    None
}

#[cfg(test)]
mod tests {

    #[test]
    fn test_extract_failing_row() {
        // Simulate the error message format from ClickHouse
        let msg = "Code: 117. DB::Exception: Cannot parse JSON object here: : (at row 791)\n: While executing BinaryRowInputFormat.";

        // We can't easily construct a clickhouse::error::Error, but we can test the parsing logic
        assert!(msg.contains("(at row "));
        let start = msg.find("(at row ").unwrap();
        let rest = &msg[start + 8..];
        let end = rest.find(')').unwrap();
        let row: usize = rest[..end].parse().unwrap();
        assert_eq!(row, 791);
    }
}
