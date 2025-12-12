use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::Utc;
use dashmap::DashMap;
use n0_future::StreamExt;
use smol_str::{SmolStr, ToSmolStr};
use tracing::{debug, info, trace, warn};

use chrono::DateTime;

use crate::clickhouse::{
    AccountRevState, Client, FirehoseCursor, RawAccountEvent, RawIdentityEvent, RawRecordInsert,
};
use crate::config::IndexerConfig;
use crate::config::TapConfig;
use crate::error::{ClickHouseError, IndexError, Result};
use crate::firehose::{
    Account, Commit, ExtractedRecord, FirehoseConsumer, Identity, MessageStream,
    SubscribeReposMessage, extract_records,
};
use crate::tap::{TapConfig as TapConsumerConfig, TapConsumer, TapEvent};

/// Default consumer ID for cursor tracking
const CONSUMER_ID: &str = "main";

/// Per-account revision state for deduplication
#[derive(Debug, Clone)]
pub struct RevState {
    pub last_rev: SmolStr,
    pub last_cid: SmolStr,
}

/// In-memory cache of per-account revision state
///
/// Used for fast deduplication without hitting ClickHouse on every event.
/// Populated from account_rev_state table on startup, updated as events are processed.
pub struct RevCache {
    inner: DashMap<SmolStr, RevState>,
}

impl RevCache {
    pub fn new() -> Self {
        Self {
            inner: DashMap::new(),
        }
    }

    /// Load cache from ClickHouse account_rev_state table
    pub async fn load_from_clickhouse(client: &Client) -> Result<Self> {
        let query = r#"
            SELECT
                did,
                argMaxMerge(last_rev) as last_rev,
                argMaxMerge(last_cid) as last_cid,
                maxMerge(last_seq) as last_seq,
                maxMerge(last_event_time) as last_event_time
            FROM account_rev_state
            GROUP BY did
        "#;

        let rows: Vec<AccountRevState> =
            client.inner().query(query).fetch_all().await.map_err(|e| {
                IndexError::ClickHouse(crate::error::ClickHouseError::Query {
                    message: "failed to load account rev state".into(),
                    source: e,
                })
            })?;

        let cache = Self::new();
        for row in rows {
            cache.inner.insert(
                SmolStr::new(&row.did),
                RevState {
                    last_rev: SmolStr::new(&row.last_rev),
                    last_cid: SmolStr::new(&row.last_cid),
                },
            );
        }

        info!(
            accounts = cache.inner.len(),
            "loaded rev cache from clickhouse"
        );
        Ok(cache)
    }

    /// Check if we should process this commit (returns false if already seen)
    pub fn should_process(&self, did: &str, rev: &str) -> bool {
        match self.inner.get(did) {
            Some(state) => rev > state.last_rev.as_str(),
            None => true, // new account, always process
        }
    }

    /// Update cache after processing a commit
    pub fn update(&self, did: &SmolStr, rev: &SmolStr, cid: &SmolStr) {
        self.inner.insert(
            did.clone(),
            RevState {
                last_rev: rev.clone(),
                last_cid: cid.clone(),
            },
        );
    }

    /// Get current cache size (number of accounts tracked)
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }
}

impl Default for RevCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Safety margin when resuming - back up this many sequence numbers
/// to ensure no gaps from incomplete batches or race conditions
const CURSOR_REWIND: i64 = 1000;

/// Load cursor from ClickHouse for resuming
///
/// Returns cursor with safety margin subtracted to ensure overlap
pub async fn load_cursor(client: &Client) -> Result<Option<i64>> {
    let query = format!(
        r#"
        SELECT consumer_id, seq, event_time
        FROM firehose_cursor FINAL
        WHERE consumer_id = '{}'
        LIMIT 1
        "#,
        CONSUMER_ID
    );

    let cursor: Option<FirehoseCursor> = client
        .inner()
        .query(&query)
        .fetch_optional()
        .await
        .map_err(|e| crate::error::ClickHouseError::Query {
            message: "failed to load cursor".into(),
            source: e,
        })?;

    if let Some(c) = &cursor {
        let resume_at = (c.seq as i64).saturating_sub(CURSOR_REWIND);
        info!(
            saved_seq = c.seq,
            resume_seq = resume_at,
            rewind = CURSOR_REWIND,
            "loaded cursor from clickhouse (with safety margin)"
        );
        Ok(Some(resume_at))
    } else {
        Ok(None)
    }
}

/// Firehose indexer that consumes AT Protocol firehose and writes to ClickHouse
pub struct FirehoseIndexer {
    client: Arc<Client>,
    consumer: FirehoseConsumer,
    rev_cache: RevCache,
    config: IndexerConfig,
}

impl FirehoseIndexer {
    /// Create a new firehose indexer
    pub async fn new(
        client: Client,
        consumer: FirehoseConsumer,
        config: IndexerConfig,
    ) -> Result<Self> {
        let client = Arc::new(client);

        // Load rev cache from ClickHouse
        let rev_cache = RevCache::load_from_clickhouse(&client).await?;

        Ok(Self {
            client,
            consumer,
            rev_cache,
            config,
        })
    }

    /// Save cursor to ClickHouse
    async fn save_cursor(&self, seq: u64, event_time: DateTime<Utc>) -> Result<()> {
        let query = format!(
            "INSERT INTO firehose_cursor (consumer_id, seq, event_time) VALUES ('{}', {}, {})",
            CONSUMER_ID,
            seq,
            event_time.timestamp_millis()
        );

        self.client.execute(&query).await?;
        debug!(seq, "saved cursor");
        Ok(())
    }

    /// Run the indexer loop
    pub async fn run(&self) -> Result<()> {
        info!("connecting to firehose...");
        let mut stream: MessageStream = self.consumer.connect().await?;

        // Inserters handle batching internally based on config
        let mut records = self.client.inserter::<RawRecordInsert>("raw_records");
        let mut identities = self
            .client
            .inserter::<RawIdentityEvent>("raw_identity_events");
        let mut accounts = self
            .client
            .inserter::<RawAccountEvent>("raw_account_events");

        // Stats and cursor tracking
        let mut processed: u64 = 0;
        let mut skipped: u64 = 0;
        let mut last_seq: u64 = 0;
        let mut last_event_time = Utc::now();
        let mut last_stats = Instant::now();
        let mut last_cursor_save = Instant::now();

        info!("starting indexer loop");

        loop {
            // Get time until next required flush - must commit before socket timeout (30s)
            let records_time = records.time_left().unwrap_or(Duration::from_secs(10));
            let identities_time = identities.time_left().unwrap_or(Duration::from_secs(10));
            let accounts_time = accounts.time_left().unwrap_or(Duration::from_secs(10));
            let time_left = records_time.min(identities_time).min(accounts_time);

            let result =
                match tokio::time::timeout(time_left, stream.next()).await {
                    Ok(Some(result)) => result,
                    Ok(None) => {
                        // Stream ended
                        break;
                    }
                    Err(_) => {
                        // Timeout - flush inserters to keep INSERT alive
                        debug!("flush timeout, committing inserters");
                        records.commit().await.map_err(|e| {
                            crate::error::ClickHouseError::Query {
                                message: "periodic records commit failed".into(),
                                source: e,
                            }
                        })?;
                        identities.commit().await.map_err(|e| {
                            crate::error::ClickHouseError::Query {
                                message: "periodic identities commit failed".into(),
                                source: e,
                            }
                        })?;
                        accounts.commit().await.map_err(|e| {
                            crate::error::ClickHouseError::Query {
                                message: "periodic accounts commit failed".into(),
                                source: e,
                            }
                        })?;
                        continue;
                    }
                };

            let msg = match result {
                Ok(msg) => msg,
                Err(e) => {
                    warn!(error = ?e, "firehose stream error");
                    continue;
                }
            };

            // Track seq from any message type that has it
            match &msg {
                SubscribeReposMessage::Commit(c) => {
                    last_seq = c.seq as u64;
                    last_event_time = c.time.as_ref().with_timezone(&Utc);
                }
                SubscribeReposMessage::Identity(i) => {
                    last_seq = i.seq as u64;
                    last_event_time = i.time.as_ref().with_timezone(&Utc);
                }
                SubscribeReposMessage::Account(a) => {
                    last_seq = a.seq as u64;
                    last_event_time = a.time.as_ref().with_timezone(&Utc);
                }
                _ => {}
            }

            match msg {
                SubscribeReposMessage::Commit(commit) => {
                    if self
                        .process_commit(&commit, &mut records, &mut skipped)
                        .await?
                    {
                        processed += 1;
                    }
                }
                SubscribeReposMessage::Identity(identity) => {
                    write_identity(&identity, &mut identities).await?;
                }
                SubscribeReposMessage::Account(account) => {
                    write_account(&account, &mut accounts).await?;
                }
                SubscribeReposMessage::Sync(_) => {
                    debug!("received sync (tooBig) event, skipping");
                }
                _ => {}
            }

            // commit() flushes if internal thresholds met, otherwise no-op
            records
                .commit()
                .await
                .map_err(|e| crate::error::ClickHouseError::Query {
                    message: "commit failed".into(),
                    source: e,
                })?;

            // Periodic stats and cursor save (every 10s)
            if last_stats.elapsed() >= Duration::from_secs(10) {
                info!(
                    processed,
                    skipped,
                    last_seq,
                    rev_cache_size = self.rev_cache.len(),
                    "indexer stats"
                );
                last_stats = Instant::now();
            }

            // Save cursor every 30s
            if last_cursor_save.elapsed() >= Duration::from_secs(30) && last_seq > 0 {
                if let Err(e) = self.save_cursor(last_seq, last_event_time).await {
                    warn!(error = ?e, "failed to save cursor");
                }
                last_cursor_save = Instant::now();
            }
        }

        // Final flush
        records
            .end()
            .await
            .map_err(|e| crate::error::ClickHouseError::Query {
                message: "final flush failed".into(),
                source: e,
            })?;
        identities
            .end()
            .await
            .map_err(|e| crate::error::ClickHouseError::Query {
                message: "final flush failed".into(),
                source: e,
            })?;
        accounts
            .end()
            .await
            .map_err(|e| crate::error::ClickHouseError::Query {
                message: "final flush failed".into(),
                source: e,
            })?;

        // Final cursor save
        if last_seq > 0 {
            self.save_cursor(last_seq, last_event_time).await?;
        }

        info!(last_seq, "firehose stream ended");
        Ok(())
    }

    async fn process_commit(
        &self,
        commit: &Commit<'_>,
        inserter: &mut clickhouse::inserter::Inserter<RawRecordInsert>,
        skipped: &mut u64,
    ) -> Result<bool> {
        let did = commit.repo.as_ref();
        let rev = commit.rev.as_ref();

        // Dedup check
        if !self.rev_cache.should_process(did, rev) {
            *skipped += 1;
            return Ok(false);
        }

        // Extract and write records
        for record in extract_records(commit).await? {
            // Collection filter - skip early before JSON conversion
            if !self.config.collections.matches(&record.collection) {
                continue;
            }

            let json = record.to_json()?.unwrap_or_else(|| "{}".to_string());

            // Fire and forget delete handling
            if record.operation == "delete" {
                let client = self.client.clone();
                let record_clone = record.clone();
                tokio::spawn(async move {
                    if let Err(e) = handle_delete(&client, record_clone).await {
                        warn!(error = ?e, "delete handling failed");
                    }
                });
            }

            inserter
                .write(&RawRecordInsert {
                    did: record.did.clone(),
                    collection: record.collection.clone(),
                    rkey: record.rkey.clone(),
                    cid: record.cid.clone(),
                    rev: record.rev.clone(),
                    record: json.to_smolstr(),
                    operation: record.operation.clone(),
                    seq: record.seq as u64,
                    event_time: record.event_time,
                    is_live: true,
                })
                .await
                .map_err(|e| crate::error::ClickHouseError::Query {
                    message: "write failed".into(),
                    source: e,
                })?;
        }

        // Update rev cache
        self.rev_cache.update(
            &SmolStr::new(did),
            &SmolStr::new(rev),
            &commit.commit.0.to_smolstr(),
        );

        Ok(true)
    }
}

async fn write_identity(
    identity: &Identity<'_>,
    inserter: &mut clickhouse::inserter::Inserter<RawIdentityEvent>,
) -> Result<()> {
    inserter
        .write(&RawIdentityEvent {
            did: identity.did.to_smolstr(),
            handle: identity
                .handle
                .as_ref()
                .map(|h| h.as_ref().to_smolstr())
                .unwrap_or_default(),
            seq: identity.seq as u64,
            event_time: identity.time.as_ref().with_timezone(&Utc),
        })
        .await
        .map_err(|e| crate::error::ClickHouseError::Query {
            message: "write failed".into(),
            source: e,
        })?;
    Ok(())
}

async fn write_account(
    account: &Account<'_>,
    inserter: &mut clickhouse::inserter::Inserter<RawAccountEvent>,
) -> Result<()> {
    inserter
        .write(&RawAccountEvent {
            did: account.did.to_smolstr(),
            active: if account.active { 1 } else { 0 },
            status: account
                .status
                .as_ref()
                .map(|s| s.as_ref().to_smolstr())
                .unwrap_or_default(),
            seq: account.seq as u64,
            event_time: account.time.as_ref().with_timezone(&Utc),
        })
        .await
        .map_err(|e| crate::error::ClickHouseError::Query {
            message: "write failed".into(),
            source: e,
        })?;
    Ok(())
}

/// Handle a delete event with poll-then-stub logic
///
/// For deletes, we need to look up the original record to know what was deleted
/// (e.g., which notebook a like was for). If the record doesn't exist yet
/// (out-of-order events), we poll for up to 15 seconds before creating a stub tombstone.
/// Minimal struct for delete lookups - just the fields we need to process the delete
#[derive(Debug, Clone, clickhouse::Row, serde::Deserialize)]
struct LookupRawRecord {
    #[allow(dead_code)]
    did: SmolStr,
    #[allow(dead_code)]
    collection: SmolStr,
    #[allow(dead_code)]
    rkey: SmolStr,
    #[allow(dead_code)]
    record: SmolStr, // JSON string of the original record
}

async fn handle_delete(client: &Client, record: ExtractedRecord) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(15);

    loop {
        // Try to find the record by CID
        let query = format!(
            r#"
            SELECT did, collection, rkey, record
            FROM raw_records
            WHERE did = '{}' AND cid = '{}'
            ORDER BY event_time DESC
            LIMIT 1
            "#,
            record.did, record.cid
        );

        let original: Option<LookupRawRecord> = client
            .inner()
            .query(&query)
            .fetch_optional()
            .await
            .map_err(|e| crate::error::ClickHouseError::Query {
                message: "delete lookup failed".into(),
                source: e,
            })?;

        if let Some(_original) = original {
            // Found the record - the main insert path already handles creating
            // the delete row, so we're done. In phase 2, this is where we'd
            // parse original.record and insert count deltas for denormalized tables.
            debug!(did = %record.did, cid = %record.cid, "delete found original record");
            return Ok(());
        }

        if Instant::now() > deadline {
            // Gave up - create stub tombstone
            // The record will be inserted via the main batch path with operation='delete'
            // and empty record content, which serves as our stub tombstone
            warn!(
                did = %record.did,
                cid = %record.cid,
                "delete timeout, stub tombstone will be created"
            );
            return Ok(());
        }

        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

// ============================================================================
// TapIndexer - consumes from tap websocket
// ============================================================================

/// Consumer ID for tap cursor tracking
const TAP_CONSUMER_ID: &str = "tap";

/// Tap indexer that consumes from tap websocket and writes to ClickHouse
pub struct TapIndexer {
    client: Arc<Client>,
    tap_config: TapConfig,
    config: IndexerConfig,
}

impl TapIndexer {
    /// Create a new tap indexer
    pub fn new(client: Client, tap_config: TapConfig, config: IndexerConfig) -> Self {
        Self {
            client: Arc::new(client),
            tap_config,
            config,
        }
    }

    /// Save tap cursor to ClickHouse for visibility
    async fn save_cursor(&self, seq: u64) -> Result<()> {
        let query = format!(
            "INSERT INTO firehose_cursor (consumer_id, seq, event_time) VALUES ('{}', {}, now64(3))",
            TAP_CONSUMER_ID, seq
        );

        self.client.execute(&query).await?;
        debug!(seq, "saved tap cursor");
        Ok(())
    }

    /// Run the tap indexer loop
    pub async fn run(&self) -> Result<()> {
        info!(url = %self.tap_config.url, "connecting to tap...");

        let consumer_config = TapConsumerConfig::new(self.tap_config.url.clone())
            .with_acks(self.tap_config.send_acks);
        let consumer = TapConsumer::new(consumer_config);

        let (mut events, ack_tx) = consumer.connect().await?;

        let mut records = self.client.inserter::<RawRecordInsert>("raw_records");
        let mut identities = self
            .client
            .inserter::<RawIdentityEvent>("raw_identity_events");

        let mut processed: u64 = 0;
        let mut last_seq: u64 = 0;
        let mut last_stats = Instant::now();
        let mut last_cursor_save = Instant::now();

        info!("starting tap indexer loop");

        loop {
            // Get time until next required flush - must commit before socket timeout (30s)
            let records_time = records.time_left().unwrap_or(Duration::from_secs(10));
            let identities_time = identities.time_left().unwrap_or(Duration::from_secs(10));
            let time_left = records_time.min(identities_time);

            let event = match tokio::time::timeout(time_left, events.recv()).await {
                Ok(Some(event)) => event,
                Ok(None) => {
                    // Channel closed, exit loop
                    break;
                }
                Err(_) => {
                    // Timeout - flush inserters to keep INSERT alive
                    trace!("flush timeout, committing inserters");
                    records.commit().await.map_err(|e| ClickHouseError::Query {
                        message: "periodic records commit failed".into(),
                        source: e,
                    })?;
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
            last_seq = event_id;

            match event {
                TapEvent::Record(envelope) => {
                    let record = &envelope.record;

                    // Collection filter
                    if !self.config.collections.matches(&record.collection) {
                        // Still ack even if filtered
                        let _ = ack_tx.send(event_id).await;
                        continue;
                    }

                    let json = record
                        .record
                        .as_ref()
                        .map(|v| serde_json::to_string(v).unwrap_or_default())
                        .unwrap_or_default();

                    debug!(
                        op = record.action.as_str(),
                        id = event_id,
                        len = json.len(),
                        "writing record"
                    );

                    records
                        .write(&RawRecordInsert {
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
                        })
                        .await
                        .map_err(|e| ClickHouseError::Query {
                            message: "record write failed".into(),
                            source: e,
                        })?;
                    records.commit().await.map_err(|e| ClickHouseError::Query {
                        message: format!("record commit failed for id {}", event_id),
                        source: e,
                    })?;

                    processed += 1;
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
                }
            }

            // Send ack after successful write+commit
            let _ = ack_tx.send(event_id).await;

            // Periodic stats
            if last_stats.elapsed() >= Duration::from_secs(10) {
                info!(processed, last_seq, "tap indexer stats");
                last_stats = Instant::now();
            }

            // Save cursor every 30s for visibility
            if last_cursor_save.elapsed() >= Duration::from_secs(30) && last_seq > 0 {
                if let Err(e) = self.save_cursor(last_seq).await {
                    warn!(error = ?e, "failed to save tap cursor");
                }
                last_cursor_save = Instant::now();
            }
        }

        // Final flush
        records.end().await.map_err(|e| ClickHouseError::Query {
            message: "final records flush failed".into(),
            source: e,
        })?;
        identities.end().await.map_err(|e| ClickHouseError::Query {
            message: "final identities flush failed".into(),
            source: e,
        })?;

        // Final cursor save
        if last_seq > 0 {
            self.save_cursor(last_seq).await?;
        }

        info!(last_seq, "tap stream ended");
        Ok(())
    }
}
