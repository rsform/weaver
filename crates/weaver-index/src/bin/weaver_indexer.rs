use clap::{Parser, Subcommand};
use tracing::info;
use weaver_index::clickhouse::{Client, Migrator};
use weaver_index::config::ClickHouseConfig;

#[derive(Parser)]
#[command(name = "weaver-indexer")]
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
    },

    /// Check database connectivity
    Health,

    /// Start the indexer service (not yet implemented)
    Run,
}

#[tokio::main]
async fn main() -> miette::Result<()> {
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("weaver_index=info".parse().unwrap())
                .add_directive("weaver_indexer=info".parse().unwrap()),
        )
        .init();

    let args = Args::parse();

    match args.command {
        Command::Migrate { dry_run } => run_migrate(dry_run).await,
        Command::Health => run_health().await,
        Command::Run => run_indexer().await,
    }
}

async fn run_migrate(dry_run: bool) -> miette::Result<()> {
    let config = ClickHouseConfig::from_env()?;
    info!(
        "Connecting to ClickHouse at {} (database: {})",
        config.url, config.database
    );

    let client = Client::new(&config)?;
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
    info!("Indexer not yet implemented");
    Ok(())
}
