use std::path::PathBuf;

use clap::{Parser, Subcommand};
use tracing::{error, info, warn};
use jacquard::client::UnauthenticatedSession;
use weaver_index::clickhouse::InserterConfig;
use weaver_index::clickhouse::{Client, Migrator};
use weaver_index::config::{
    ClickHouseConfig, FirehoseConfig, IndexerConfig, ShardConfig, SourceMode, TapConfig,
};
use weaver_index::firehose::FirehoseConsumer;
use weaver_index::server::{AppState, ServerConfig, TelemetryConfig, telemetry};
use weaver_index::{
    DraftTitleTaskConfig, FirehoseIndexer, ServiceIdentity, TapIndexer, load_cursor,
    run_draft_title_task,
};

#[derive(Parser)]
#[command(name = "indexer")]
#[command(about = "Weaver index service - firehose ingestion and query serving")]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run database migrations
    Migrate {
        /// Show what would be run without executing
        #[arg(long)]
        dry_run: bool,

        /// Drop all tables before running migrations (for testing)
        #[arg(long)]
        reset: bool,
    },

    /// Check database connectivity
    Health,

    /// Start the full service (indexer + HTTP server)
    Run,

    /// Start only the HTTP server (no indexing)
    Serve,

    /// Start only the indexer (no HTTP server)
    Index,
}

#[tokio::main]
async fn main() -> miette::Result<()> {
    dotenvy::dotenv().ok();

    // Initialize telemetry (metrics + tracing with optional Loki)
    let telemetry_config = TelemetryConfig::from_env("weaver-index");
    telemetry::init(telemetry_config).await;

    let args = Args::parse();

    match args.command {
        Command::Migrate { dry_run, reset } => run_migrate(dry_run, reset).await,
        Command::Health => run_health().await,
        Command::Run => run_full().await,
        Command::Serve => run_server_only().await,
        Command::Index => run_indexer_only().await,
    }
}

async fn run_migrate(dry_run: bool, reset: bool) -> miette::Result<()> {
    let config = ClickHouseConfig::from_env()?;
    info!(
        "Connecting to ClickHouse at {} (database: {})",
        config.url, config.database
    );

    let client = Client::new(&config)?;

    if reset {
        let objects = Migrator::all_objects();
        if dry_run {
            info!("Would drop {} objects:", objects.len());
            for obj in &objects {
                info!("  - {} ({:?})", obj.name, obj.object_type);
            }
        } else {
            info!("Dropping all tables and views...");
            for obj in &objects {
                let query = obj.drop_statement();
                match client.execute(&query).await {
                    Ok(_) => info!("  dropped {} ({:?})", obj.name, obj.object_type),
                    Err(e) => warn!("  failed to drop {}: {}", obj.name, e),
                }
            }
        }
    }

    let migrator = Migrator::new(&client);

    if dry_run {
        let pending = migrator.pending().await?;
        if pending.is_empty() {
            info!("No pending migrations");
        } else {
            info!("Pending migrations:");
            for name in pending {
                info!("  - {}", name);
            }
        }
    } else {
        let result = migrator.run().await?;
        info!("{}", result);
    }

    Ok(())
}

async fn run_health() -> miette::Result<()> {
    let config = ClickHouseConfig::from_env()?;
    info!(
        "Connecting to ClickHouse at {} (database: {})",
        config.url, config.database
    );

    let client = Client::new(&config)?;

    // Simple connectivity check
    client.execute("SELECT 1").await?;
    info!("ClickHouse connection OK");

    Ok(())
}

/// Run both indexer and HTTP server concurrently (production mode)
async fn run_full() -> miette::Result<()> {
    let ch_config = ClickHouseConfig::from_env()?;
    let shard_config = ShardConfig::from_env();
    let server_config = ServerConfig::from_env();
    let indexer_config = IndexerConfig::from_env();
    let source_mode = SourceMode::from_env();

    info!(
        "Connecting to ClickHouse at {} (database: {})",
        ch_config.url, ch_config.database
    );
    info!("SQLite shards at {}", shard_config.base_path.display());

    // Load or generate service identity keypair
    let key_path = std::env::var("SERVICE_KEY_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("./data/service.key"));
    let identity = ServiceIdentity::load_or_generate(&key_path)?;
    info!(
        public_key = %identity.public_key_multibase(),
        "Service identity loaded"
    );

    // Generate DID document with service endpoint
    let service_endpoint = std::env::var("SERVICE_ENDPOINT").unwrap_or_else(|_| {
        format!(
            "https://{}",
            server_config
                .service_did
                .as_str()
                .strip_prefix("did:web:")
                .unwrap_or("index.weaver.sh")
        )
    });
    let did_doc = identity.did_document_with_service(&server_config.service_did, &service_endpoint);

    // Create separate clients for indexer, server, and background tasks
    let indexer_client = Client::new(&ch_config)?;
    let server_client = Client::new(&ch_config)?;
    let task_client = std::sync::Arc::new(Client::new(&ch_config)?);

    // Build AppState for server
    let state = AppState::new(
        server_client,
        shard_config,
        server_config.service_did.clone(),
    );

    // Spawn the indexer task
    let indexer_handle = match source_mode {
        SourceMode::Firehose => {
            let mut firehose_config = FirehoseConfig::from_env()?;
            if firehose_config.cursor.is_none() {
                if let Some(cursor) = load_cursor(&indexer_client).await? {
                    firehose_config.cursor = Some(cursor);
                }
            }
            info!(
                "Connecting to firehose at {} (cursor: {:?})",
                firehose_config.relay_url, firehose_config.cursor
            );
            let consumer = FirehoseConsumer::new(firehose_config);
            let indexer = FirehoseIndexer::new(indexer_client, consumer, indexer_config).await?;
            info!("Starting firehose indexer");
            tokio::spawn(async move { indexer.run().await })
        }
        SourceMode::Tap => {
            let tap_config = TapConfig::from_env()?;
            let num_workers = tap_config.num_workers;
            let indexer = TapIndexer::new(
                indexer_client,
                tap_config,
                InserterConfig::default(),
                indexer_config,
                num_workers,
            );
            info!("Starting tap indexer with {} workers", num_workers);
            tokio::spawn(async move { indexer.run().await })
        }
    };

    // Spawn background tasks
    let resolver = UnauthenticatedSession::new_public();
    tokio::spawn(run_draft_title_task(
        task_client,
        resolver,
        DraftTitleTaskConfig::default(),
    ));

    // Run server, monitoring indexer health
    tokio::select! {
        result = weaver_index::server::run(state, server_config, did_doc) => {
            result?;
        }
        result = indexer_handle => {
            match result {
                Ok(Ok(())) => info!("Indexer completed"),
                Ok(Err(e)) => error!("Indexer failed: {}", e),
                Err(e) => error!("Indexer task panicked: {}", e),
            }
        }
    }

    Ok(())
}

/// Run only the indexer (no HTTP server)
async fn run_indexer_only() -> miette::Result<()> {
    let ch_config = ClickHouseConfig::from_env()?;
    let indexer_config = IndexerConfig::from_env();
    let source_mode = SourceMode::from_env();

    info!(
        "Connecting to ClickHouse at {} (database: {})",
        ch_config.url, ch_config.database
    );
    let client = Client::new(&ch_config)?;

    match source_mode {
        SourceMode::Firehose => run_firehose_indexer(client, indexer_config).await,
        SourceMode::Tap => {
            let tap_config = TapConfig::from_env()?;
            run_tap_indexer(client, tap_config, indexer_config).await
        }
    }
}

async fn run_firehose_indexer(client: Client, indexer_config: IndexerConfig) -> miette::Result<()> {
    let mut firehose_config = FirehoseConfig::from_env()?;

    // Load cursor from ClickHouse if not overridden by env var
    if firehose_config.cursor.is_none() {
        if let Some(cursor) = load_cursor(&client).await? {
            firehose_config.cursor = Some(cursor);
        }
    }

    info!(
        "Connecting to firehose at {} (cursor: {:?})",
        firehose_config.relay_url, firehose_config.cursor
    );
    let consumer = FirehoseConsumer::new(firehose_config);

    let indexer = FirehoseIndexer::new(client, consumer, indexer_config).await?;

    info!("Starting firehose indexer");
    indexer.run().await?;

    Ok(())
}

async fn run_tap_indexer(
    client: Client,
    tap_config: TapConfig,
    indexer_config: IndexerConfig,
) -> miette::Result<()> {
    let num_workers = tap_config.num_workers;
    let indexer = TapIndexer::new(
        client,
        tap_config,
        InserterConfig::default(),
        indexer_config,
        num_workers,
    );

    info!("Starting tap indexer with {} workers", num_workers);
    indexer.run().await?;

    Ok(())
}

async fn run_server_only() -> miette::Result<()> {
    let ch_config = ClickHouseConfig::from_env()?;
    let shard_config = ShardConfig::from_env();
    let server_config = ServerConfig::from_env();

    info!(
        "Connecting to ClickHouse at {} (database: {})",
        ch_config.url, ch_config.database
    );
    info!("SQLite shards at {}", shard_config.base_path.display());

    // Load or generate service identity keypair
    let key_path = std::env::var("SERVICE_KEY_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("./data/service.key"));
    let identity = ServiceIdentity::load_or_generate(&key_path)?;
    info!(
        public_key = %identity.public_key_multibase(),
        "Service identity loaded"
    );

    // Generate DID document with service endpoint
    let service_endpoint = std::env::var("SERVICE_ENDPOINT").unwrap_or_else(|_| {
        format!(
            "https://{}",
            server_config
                .service_did
                .as_str()
                .strip_prefix("did:web:")
                .unwrap_or("localhost")
        )
    });
    let did_doc = identity.did_document_with_service(&server_config.service_did, &service_endpoint);

    let client = Client::new(&ch_config)?;

    let state = AppState::new(client, shard_config, server_config.service_did.clone());
    weaver_index::server::run(state, server_config, did_doc).await?;

    Ok(())
}
