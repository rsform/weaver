use clap::{Parser, Subcommand};
use miette::IntoDiagnostic;
use tracing::{Level, info, warn};
use tracing_subscriber::EnvFilter;
use weaver_index::clickhouse::{Client, Migrator, Tables};
use weaver_index::config::{ClickHouseConfig, FirehoseConfig, IndexerConfig};
use weaver_index::firehose::FirehoseConsumer;
use weaver_index::{Indexer, load_cursor};

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

    /// Start the indexer service (not yet implemented)
    Run,
}

#[tokio::main]
async fn main() -> miette::Result<()> {
    dotenvy::dotenv().ok();

    let console_level = if cfg!(debug_assertions) {
        Level::DEBUG
    } else {
        Level::INFO
    };

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::builder()
                .from_env_lossy()
                .add_directive(console_level.into())
                .add_directive("hyper_util=info".parse().into_diagnostic()?),
        )
        .init();

    let args = Args::parse();

    match args.command {
        Command::Migrate { dry_run, reset } => run_migrate(dry_run, reset).await,
        Command::Health => run_health().await,
        Command::Run => run_indexer().await,
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
        if dry_run {
            info!("Would drop tables:");
            for table in Tables::ALL {
                info!("  - {}", table);
            }
        } else {
            info!("Dropping all tables...");
            for table in Tables::ALL {
                let query = format!("DROP TABLE IF EXISTS {}", table);
                match client.execute(&query).await {
                    Ok(_) => info!("  dropped {}", table),
                    Err(e) => warn!("  failed to drop {}: {}", table, e),
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

async fn run_indexer() -> miette::Result<()> {
    let ch_config = ClickHouseConfig::from_env()?;
    let mut firehose_config = FirehoseConfig::from_env()?;
    let indexer_config = IndexerConfig::from_env();

    info!(
        "Connecting to ClickHouse at {} (database: {})",
        ch_config.url, ch_config.database
    );
    let client = Client::new(&ch_config)?;

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

    let indexer = Indexer::new(client, consumer, indexer_config).await?;

    info!("Starting indexer");
    indexer.run().await?;

    Ok(())
}
