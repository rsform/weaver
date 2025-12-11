use bytes::Bytes;
use chrono::{DateTime, Utc};
use clap::Parser;
use clickhouse::Row;
use n0_future::StreamExt;
use smol_str::SmolStr;
use std::time::{Duration, Instant};
use tracing::{info, warn};
use weaver_index::clickhouse::Client;
use weaver_index::config::{ClickHouseConfig, FirehoseConfig};
use weaver_index::firehose::{FirehoseConsumer, SubscribeReposMessage, extract_records};

// =============================================================================
// Benchmark-specific schema (not part of production)
// =============================================================================

const TABLE_JSON: &str = "raw_records_json";
const TABLE_CBOR: &str = "raw_records_cbor";

/// Row type for JSON benchmark records
#[derive(Debug, Clone, Row, serde::Serialize, serde::Deserialize)]
struct RawRecordJson {
    did: SmolStr,
    collection: SmolStr,
    rkey: SmolStr,
    cid: String,
    record: String,
    operation: SmolStr,
    seq: u64,
    #[serde(with = "clickhouse::serde::chrono::datetime64::millis")]
    event_time: DateTime<Utc>,
}

/// Row type for CBOR benchmark records
#[derive(Debug, Clone, Row, serde::Serialize, serde::Deserialize)]
struct RawRecordCbor {
    did: SmolStr,
    collection: SmolStr,
    rkey: SmolStr,
    cid: String,
    #[serde(with = "jacquard::serde_bytes_helper")]
    record: Bytes,
    operation: SmolStr,
    seq: u64,
    #[serde(with = "clickhouse::serde::chrono::datetime64::millis")]
    event_time: DateTime<Utc>,
}

async fn create_benchmark_tables(client: &Client) -> miette::Result<()> {
    client
        .execute(&format!(
            r#"
            CREATE TABLE IF NOT EXISTS {} (
                did String,
                collection LowCardinality(String),
                rkey String,
                cid String,
                record JSON,
                operation LowCardinality(String),
                seq UInt64,
                event_time DateTime64(3),
                indexed_at DateTime64(3) DEFAULT now64(3)
            )
            ENGINE = MergeTree()
            ORDER BY (collection, did, rkey, indexed_at)
            "#,
            TABLE_JSON
        ))
        .await?;

    client
        .execute(&format!(
            r#"
            CREATE TABLE IF NOT EXISTS {} (
                did String,
                collection LowCardinality(String),
                rkey String,
                cid String,
                record String,
                operation LowCardinality(String),
                seq UInt64,
                event_time DateTime64(3),
                indexed_at DateTime64(3) DEFAULT now64(3)
            )
            ENGINE = MergeTree()
            ORDER BY (collection, did, rkey, indexed_at)
            "#,
            TABLE_CBOR
        ))
        .await?;

    Ok(())
}

async fn drop_benchmark_tables(client: &Client) -> miette::Result<()> {
    client
        .execute(&format!("DROP TABLE IF EXISTS {}", TABLE_JSON))
        .await?;
    client
        .execute(&format!("DROP TABLE IF EXISTS {}", TABLE_CBOR))
        .await?;
    Ok(())
}

// =============================================================================
// Benchmark logic
// =============================================================================

/// Tracks firehose lag to detect if we're falling behind
#[derive(Default)]
struct LagStats {
    min_ms: Option<i64>,
    max_ms: Option<i64>,
    current_ms: i64,
    sample_count: u64,
}

impl LagStats {
    fn update(&mut self, event_time_ms: i64) {
        let now_ms = Utc::now().timestamp_millis();
        let lag = now_ms - event_time_ms;

        self.current_ms = lag;
        self.sample_count += 1;

        self.min_ms = Some(self.min_ms.map_or(lag, |m| m.min(lag)));
        self.max_ms = Some(self.max_ms.map_or(lag, |m| m.max(lag)));
    }

    fn reset_window(&mut self) {
        // Keep current but reset min/max for next reporting window
        self.min_ms = Some(self.current_ms);
        self.max_ms = Some(self.current_ms);
    }
}

#[derive(Parser)]
#[command(name = "storage-benchmark")]
#[command(about = "Benchmark CBOR vs JSON storage in ClickHouse")]
struct Args {
    /// Duration to run the benchmark in minutes
    #[arg(short, long, default_value = "60")]
    duration_minutes: u64,

    /// Batch size for ClickHouse inserts
    #[arg(short, long, default_value = "1000")]
    batch_size: usize,

    /// Report interval in seconds
    #[arg(short, long, default_value = "30")]
    report_interval_secs: u64,

    /// Drop and recreate tables before starting
    #[arg(long)]
    reset_tables: bool,
}

#[tokio::main]
async fn main() -> miette::Result<()> {
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("weaver_index=info".parse().unwrap())
                .add_directive("storage_benchmark=info".parse().unwrap()),
        )
        .init();

    let args = Args::parse();

    info!("Storage Benchmark: CBOR vs JSON in ClickHouse");
    info!("Duration: {} minutes", args.duration_minutes);
    info!("Batch size: {}", args.batch_size);

    // Load configs
    let ch_config = ClickHouseConfig::from_env()?;
    let firehose_config = FirehoseConfig::from_env()?;

    info!(
        "Connecting to ClickHouse at {} (database: {})",
        ch_config.url, ch_config.database
    );
    let client = Client::new(&ch_config)?;

    // Reset tables if requested
    if args.reset_tables {
        info!("Dropping existing benchmark tables...");
        drop_benchmark_tables(&client).await?;
    }

    // Create tables
    info!("Creating benchmark tables...");
    create_benchmark_tables(&client).await?;

    // Create inserters
    let mut json_inserter = client.inserter::<RawRecordJson>(TABLE_JSON);
    let mut cbor_inserter = client.inserter::<RawRecordCbor>(TABLE_CBOR);

    // Connect to firehose
    info!("Connecting to firehose at {}", firehose_config.relay_url);
    let consumer = FirehoseConsumer::new(firehose_config);
    let mut stream = consumer.connect().await?;

    // Tracking
    let start = Instant::now();
    let duration = Duration::from_secs(args.duration_minutes * 60);
    let report_interval = Duration::from_secs(args.report_interval_secs);
    let mut last_report = Instant::now();
    let mut total_records = 0u64;
    let mut total_commits = 0u64;
    let mut errors = 0u64;
    let mut lag_stats = LagStats::default();

    info!("Starting benchmark...");

    while start.elapsed() < duration {
        // Check for report interval
        if last_report.elapsed() >= report_interval {
            // Flush inserters so size measurements are accurate
            match json_inserter.commit().await {
                Ok(stats) => info!(
                    "  JSON flush: {} rows, {} transactions",
                    stats.rows, stats.transactions
                ),
                Err(e) => warn!("Failed to flush JSON inserter: {}", e),
            }
            match cbor_inserter.commit().await {
                Ok(stats) => info!(
                    "  CBOR flush: {} rows, {} transactions",
                    stats.rows, stats.transactions
                ),
                Err(e) => warn!("Failed to flush CBOR inserter: {}", e),
            }

            report_progress(
                &client,
                total_records,
                total_commits,
                errors,
                start.elapsed(),
                &lag_stats,
            )
            .await;
            lag_stats.reset_window();
            last_report = Instant::now();
        }

        // Get next message with timeout
        let msg = tokio::time::timeout(Duration::from_secs(30), stream.next()).await;

        let msg = match msg {
            Ok(Some(Ok(msg))) => msg,
            Ok(Some(Err(e))) => {
                warn!("Stream error: {}", e);
                errors += 1;
                continue;
            }
            Ok(None) => {
                warn!("Stream ended unexpectedly");
                break;
            }
            Err(_) => {
                warn!("Timeout waiting for message");
                continue;
            }
        };

        // Only process commits
        let commit = match msg {
            SubscribeReposMessage::Commit(c) => c,
            _ => continue,
        };

        total_commits += 1;

        // Track lag
        lag_stats.update(commit.time.as_ref().timestamp_millis());

        // Extract records from the commit
        let records = match extract_records(&commit).await {
            Ok(r) => r,
            Err(e) => {
                warn!("Record extraction error: {}", e);
                errors += 1;
                continue;
            }
        };

        // Insert to both tables
        for record in records {
            // Skip deletes (no record data)
            let Some(cbor_bytes) = &record.cbor_bytes else {
                continue;
            };

            // JSON table: decode CBOR to JSON
            let json_str = match record.to_json() {
                Ok(Some(j)) => j,
                Ok(None) => continue,
                Err(e) => {
                    warn!("JSON encode error: {}", e);
                    errors += 1;
                    continue;
                }
            };

            // Insert JSON record
            json_inserter
                .write(&RawRecordJson {
                    did: record.did.clone(),
                    collection: record.collection.clone(),
                    rkey: record.rkey.clone(),
                    cid: record.cid.to_string(),
                    record: json_str,
                    operation: record.operation.clone(),
                    seq: record.seq as u64,
                    event_time: record.event_time,
                })
                .await
                .map_err(|e| weaver_index::error::ClickHouseError::Insert {
                    message: "json insert failed".into(),
                    source: e,
                })?;

            // Insert CBOR record (raw bytes, no base64)
            cbor_inserter
                .write(&RawRecordCbor {
                    did: record.did,
                    collection: record.collection,
                    rkey: record.rkey,
                    cid: record.cid.to_string(),
                    record: cbor_bytes.clone(),
                    operation: record.operation,
                    seq: record.seq as u64,
                    event_time: record.event_time,
                })
                .await
                .map_err(|e| weaver_index::error::ClickHouseError::Insert {
                    message: "cbor insert failed".into(),
                    source: e,
                })?;

            match json_inserter.commit().await {
                Ok(_) => {}
                Err(e) => warn!("Failed to flush JSON inserter: {}", e),
            }
            match cbor_inserter.commit().await {
                Ok(_) => {}
                Err(e) => warn!("Failed to flush CBOR inserter: {}", e),
            }
            total_records += 1;
        }
    }

    // Final flush
    info!("Flushing remaining records...");
    json_inserter
        .end()
        .await
        .map_err(|e| weaver_index::error::ClickHouseError::Insert {
            message: "json flush failed".into(),
            source: e,
        })?;
    cbor_inserter
        .end()
        .await
        .map_err(|e| weaver_index::error::ClickHouseError::Insert {
            message: "cbor flush failed".into(),
            source: e,
        })?;

    // Final report
    info!("\n========== FINAL RESULTS ==========");
    report_progress(
        &client,
        total_records,
        total_commits,
        errors,
        start.elapsed(),
        &lag_stats,
    )
    .await;

    // Detailed size comparison
    info!("\nStorage Comparison:");
    let sizes = client.table_sizes(&[TABLE_JSON, TABLE_CBOR]).await?;

    for size in &sizes {
        info!(
            "  {}: {} compressed, {} uncompressed, {:.2}x ratio, {} rows",
            size.table,
            size.compressed_human(),
            size.uncompressed_human(),
            size.compression_ratio(),
            size.row_count
        );
    }

    if sizes.len() == 2 {
        let json_size = sizes.iter().find(|s| s.table == TABLE_JSON);
        let cbor_size = sizes.iter().find(|s| s.table == TABLE_CBOR);

        if let (Some(json), Some(cbor)) = (json_size, cbor_size) {
            let compressed_diff = json.compressed_bytes as f64 / cbor.compressed_bytes as f64;
            let uncompressed_diff = json.uncompressed_bytes as f64 / cbor.uncompressed_bytes as f64;

            info!("\nJSON vs CBOR:");
            info!(
                "  Compressed: JSON is {:.2}x the size of CBOR",
                compressed_diff
            );
            info!(
                "  Uncompressed: JSON is {:.2}x the size of CBOR",
                uncompressed_diff
            );

            if compressed_diff < 1.0 {
                info!(
                    "  Winner (compressed): JSON ({:.1}% smaller)",
                    (1.0 - compressed_diff) * 100.0
                );
            } else {
                info!(
                    "  Winner (compressed): CBOR ({:.1}% smaller)",
                    (1.0 - 1.0 / compressed_diff) * 100.0
                );
            }
        }
    }

    info!("\nBenchmark complete!");

    Ok(())
}

async fn report_progress(
    client: &Client,
    total_records: u64,
    total_commits: u64,
    errors: u64,
    elapsed: Duration,
    lag: &LagStats,
) {
    let records_per_sec = total_records as f64 / elapsed.as_secs_f64();

    info!(
        "Progress: {} records from {} commits in {:.1}s ({:.1}/s), {} errors",
        total_records,
        total_commits,
        elapsed.as_secs_f64(),
        records_per_sec,
        errors
    );

    // Lag info - critical for detecting if we're falling behind
    if lag.sample_count > 0 {
        info!(
            "  Lag: current={:.1}s, min={:.1}s, max={:.1}s (window)",
            lag.current_ms as f64 / 1000.0,
            lag.min_ms.unwrap_or(0) as f64 / 1000.0,
            lag.max_ms.unwrap_or(0) as f64 / 1000.0,
        );
    }

    // Try to get current sizes
    match client.table_sizes(&[TABLE_JSON, TABLE_CBOR]).await {
        Ok(sizes) => {
            for size in sizes {
                info!(
                    "  {}: {} compressed ({} rows)",
                    size.table,
                    size.compressed_human(),
                    size.row_count
                );
            }
        }
        Err(e) => {
            warn!("Failed to query table sizes: {}", e);
        }
    }
}
