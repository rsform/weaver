use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::Utc;
use smol_str::{SmolStr, ToSmolStr};
use tokio::task::JoinHandle;
use tracing::{debug, error, info, trace, warn};

use crate::clickhouse::migrations::Migrator;
use crate::clickhouse::{
    Client, InserterConfig, RawIdentityEvent, RawRecordInsert, ResilientRecordInserter,
};
use crate::config::{IndexerConfig, TapConfig};
use crate::error::{ClickHouseError, Result};
use crate::tap::{TapConfig as TapConsumerConfig, TapConsumer, TapEvent};

/// TAP indexer with multiple parallel websocket connections
///
/// Each worker maintains its own websocket connection to TAP and its own
/// ClickHouse inserter. TAP distributes events across connected clients,
/// and its ack-gating mechanism ensures per-DID ordering is preserved
/// regardless of which worker handles which events.
pub struct TapIndexer {
    client: Arc<Client>,
    tap_config: TapConfig,
    inserter_config: InserterConfig,
    config: Arc<IndexerConfig>,
    num_workers: usize,
    /// Tracks whether backfill has been triggered (first live event seen)
    backfill_triggered: Arc<AtomicBool>,
}

impl TapIndexer {
    pub fn new(
        client: Client,
        tap_config: TapConfig,
        inserter_config: InserterConfig,
        config: IndexerConfig,
        num_workers: usize,
    ) -> Self {
        Self {
            client: Arc::new(client),
            tap_config,
            inserter_config,
            config: Arc::new(config),
            num_workers,
            backfill_triggered: Arc::new(AtomicBool::new(false)),
        }
    }

    pub async fn run(&self) -> Result<()> {
        info!(
            num_workers = self.num_workers,
            url = %self.tap_config.url,
            "starting parallel tap indexer"
        );

        let mut handles: Vec<JoinHandle<Result<()>>> = Vec::with_capacity(self.num_workers);

        for worker_id in 0..self.num_workers {
            let client = self.client.clone();
            let tap_config = self.tap_config.clone();
            let inserter_config = self.inserter_config.clone();
            let config = self.config.clone();
            let backfill_triggered = self.backfill_triggered.clone();

            let handle = tokio::spawn(async move {
                run_tap_worker(
                    worker_id,
                    client,
                    tap_config,
                    inserter_config,
                    config,
                    backfill_triggered,
                )
                .await
            });

            handles.push(handle);
        }

        // Wait for all workers
        // TODO: Implement proper supervision - restart failed workers instead of propagating
        for (i, handle) in handles.into_iter().enumerate() {
            match handle.await {
                Ok(Ok(())) => {
                    info!(worker_id = i, "tap worker finished cleanly");
                }
                Ok(Err(e)) => {
                    error!(worker_id = i, error = ?e, "tap worker failed");
                    return Err(e);
                }
                Err(e) => {
                    error!(worker_id = i, error = ?e, "tap worker panicked");
                    return Err(crate::error::FirehoseError::Stream {
                        message: format!("worker {} panicked: {}", i, e),
                    }
                    .into());
                }
            }
        }

        Ok(())
    }
}

async fn run_tap_worker(
    worker_id: usize,
    client: Arc<Client>,
    tap_config: TapConfig,
    inserter_config: InserterConfig,
    config: Arc<IndexerConfig>,
    backfill_triggered: Arc<AtomicBool>,
) -> Result<()> {
    info!(worker_id, url = %tap_config.url, "tap worker starting");

    let consumer_config =
        TapConsumerConfig::new(tap_config.url.clone()).with_acks(tap_config.send_acks);
    let consumer = TapConsumer::new(consumer_config);

    let (mut events, ack_tx) = consumer.connect().await?;

    // Each worker has its own resilient inserter
    let mut records = ResilientRecordInserter::new(client.inner().clone(), inserter_config);
    let mut identities = client.inserter::<RawIdentityEvent>("raw_identity_events");

    let mut processed: u64 = 0;
    let mut last_stats = Instant::now();

    info!(worker_id, "tap worker connected, starting event loop");

    loop {
        // Get time until next required flush
        let records_time = records.time_left().unwrap_or(Duration::from_secs(10));
        let identities_time = identities.time_left().unwrap_or(Duration::from_secs(10));
        let time_left = records_time.min(identities_time);

        let event = match tokio::time::timeout(time_left, events.recv()).await {
            Ok(Some(event)) => event,
            Ok(None) => {
                info!(worker_id, "tap channel closed, exiting");
                break;
            }
            Err(_) => {
                // Timeout - flush inserters
                trace!(worker_id, "flush timeout, committing inserters");
                records.commit().await?;
                identities
                    .commit()
                    .await
                    .map_err(|e| ClickHouseError::Query {
                        message: "periodic identities commit failed".into(),
                        source: e,
                    })?;
                continue;
            }
        };

        let event_id = event.id();

        match event {
            TapEvent::Record(envelope) => {
                let record = &envelope.record;

                // Collection filter
                if !config.collections.matches(&record.collection) {
                    let _ = ack_tx.send(event_id).await;
                    continue;
                }

                // Serialize record
                let json = match &record.record {
                    Some(v) => match serde_json::to_string(v) {
                        Ok(s) => s,
                        Err(e) => {
                            warn!(
                                worker_id,
                                did = %record.did,
                                collection = %record.collection,
                                rkey = %record.rkey,
                                error = ?e,
                                "failed to serialize record, sending to DLQ"
                            );
                            let raw_data = format!(
                                r#"{{"did":"{}","collection":"{}","rkey":"{}","cid":"{}","error":"serialization_failed"}}"#,
                                record.did, record.collection, record.rkey, record.cid
                            );
                            records
                                .write_raw_to_dlq(
                                    record.action.as_str().to_smolstr(),
                                    raw_data,
                                    e.to_string(),
                                    event_id,
                                )
                                .await?;
                            let _ = ack_tx.send(event_id).await;
                            continue;
                        }
                    },
                    None => "{}".to_string(),
                };

                debug!(
                    worker_id,
                    op = record.action.as_str(),
                    id = event_id,
                    len = json.len(),
                    "writing record"
                );

                records
                    .write(RawRecordInsert {
                        did: record.did.clone(),
                        collection: record.collection.clone(),
                        rkey: record.rkey.clone(),
                        cid: record.cid.clone(),
                        rev: record.rev.clone(),
                        record: json.to_smolstr(),
                        operation: record.action.as_str().to_smolstr(),
                        seq: event_id,
                        event_time: Utc::now(),
                        is_live: record.live,
                        // records from tap are pre-validated
                        validation_state: SmolStr::new_static("valid"),
                    })
                    .await?;
                records.commit().await?;

                // Ack after successful processing
                let _ = ack_tx.send(event_id).await;

                processed += 1;

                // Trigger backfill on first live event
                // compare_exchange ensures only one worker triggers this
                if record.live
                    && backfill_triggered
                        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                        .is_ok()
                {
                    info!(
                        worker_id,
                        "first live event received, scheduling backfill"
                    );
                    let backfill_client = client.clone();
                    tokio::spawn(async move {
                        run_backfill(backfill_client).await;
                    });
                }
            }
            TapEvent::Identity(envelope) => {
                let identity = &envelope.identity;

                identities
                    .write(&RawIdentityEvent {
                        did: identity.did.clone(),
                        handle: identity.handle.clone(),
                        seq: event_id,
                        event_time: Utc::now(),
                    })
                    .await
                    .map_err(|e| ClickHouseError::Query {
                        message: "identity write failed".into(),
                        source: e,
                    })?;
                identities
                    .commit()
                    .await
                    .map_err(|e| ClickHouseError::Query {
                        message: "identity commit failed".into(),
                        source: e,
                    })?;

                let _ = ack_tx.send(event_id).await;
            }
        }

        // Periodic stats
        if last_stats.elapsed() > Duration::from_secs(30) {
            info!(worker_id, processed, "tap worker stats");
            last_stats = Instant::now();
        }
    }

    // Clean shutdown
    records.end().await?;
    identities.end().await.map_err(|e| ClickHouseError::Query {
        message: "identities end failed".into(),
        source: e,
    })?;

    info!(worker_id, processed, "tap worker finished");
    Ok(())
}

/// Run backfill queries for incremental MVs
///
/// Called once when the first live event is received, indicating historical
/// data load is complete. Waits briefly to let in-flight inserts settle,
/// then runs INSERT queries to populate target tables for incremental MVs.
async fn run_backfill(client: Arc<Client>) {
    // Wait for in-flight inserts to settle
    info!("backfill: waiting 10s for in-flight inserts to settle");
    tokio::time::sleep(Duration::from_secs(10)).await;

    let mvs = Migrator::incremental_mvs();
    if mvs.is_empty() {
        info!("backfill: no incremental MVs found, nothing to do");
        return;
    }

    info!(count = mvs.len(), "backfill: starting incremental MV backfill");

    for mv in mvs {
        info!(
            mv = %mv.name,
            table = %mv.target_table,
            "backfill: running backfill query"
        );

        let query = mv.backfill_query();
        debug!(query = %query, "backfill query");

        match client.execute(&query).await {
            Ok(()) => {
                info!(mv = %mv.name, "backfill: completed successfully");
            }
            Err(e) => {
                error!(mv = %mv.name, error = ?e, "backfill: query failed");
            }
        }
    }

    info!("backfill: all incremental MVs processed");
}
