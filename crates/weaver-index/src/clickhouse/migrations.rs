use crate::error::{ClickHouseError, IndexError};
use tracing::info;

use super::Client;

/// Embedded migrations - compiled into the binary
const MIGRATIONS: &[(&str, &str)] = &[
    (
        "000_migrations.sql",
        include_str!("../../migrations/clickhouse/000_migrations.sql"),
    ),
    (
        "001_raw_records.sql",
        include_str!("../../migrations/clickhouse/001_raw_records.sql"),
    ),
    (
        "002_identity_events.sql",
        include_str!("../../migrations/clickhouse/002_identity_events.sql"),
    ),
    (
        "003_account_events.sql",
        include_str!("../../migrations/clickhouse/003_account_events.sql"),
    ),
    (
        "004_events_dlq.sql",
        include_str!("../../migrations/clickhouse/004_events_dlq.sql"),
    ),
    (
        "005_firehose_cursor.sql",
        include_str!("../../migrations/clickhouse/005_firehose_cursor.sql"),
    ),
];

/// Migration runner for ClickHouse
pub struct Migrator<'a> {
    client: &'a Client,
}

impl<'a> Migrator<'a> {
    pub fn new(client: &'a Client) -> Self {
        Self { client }
    }

    /// Run all pending migrations
    pub async fn run(&self) -> Result<MigrationResult, IndexError> {
        // First, ensure the migrations table exists (bootstrap)
        self.ensure_migrations_table().await?;

        // Get list of already applied migrations
        let applied = self.get_applied_migrations().await?;

        let mut applied_count = 0;
        let mut skipped_count = 0;

        for (name, sql) in MIGRATIONS {
            // Skip the bootstrap migration after first run
            if *name == "000_migrations.sql" && applied.contains(&"000_migrations.sql".to_string())
            {
                skipped_count += 1;
                continue;
            }

            if applied.contains(&name.to_string()) {
                info!(migration = %name, "already applied, skipping");
                skipped_count += 1;
                continue;
            }

            info!(migration = %name, "applying migration");
            self.client.execute(sql).await?;
            self.record_migration(name).await?;
            applied_count += 1;
        }

        Ok(MigrationResult {
            applied: applied_count,
            skipped: skipped_count,
        })
    }

    /// Check which migrations would be applied without running them
    pub async fn pending(&self) -> Result<Vec<String>, IndexError> {
        // Try to get applied migrations, but if table doesn't exist, all are pending
        let applied = match self.get_applied_migrations().await {
            Ok(list) => list,
            Err(_) => vec![],
        };

        let pending: Vec<String> = MIGRATIONS
            .iter()
            .filter(|(name, _)| !applied.contains(&name.to_string()))
            .map(|(name, _)| name.to_string())
            .collect();

        Ok(pending)
    }

    async fn ensure_migrations_table(&self) -> Result<(), IndexError> {
        // Run the bootstrap migration directly
        let (_, sql) = MIGRATIONS
            .iter()
            .find(|(name, _)| *name == "000_migrations.sql")
            .expect("bootstrap migration must exist");

        self.client.execute(sql).await
    }

    async fn get_applied_migrations(&self) -> Result<Vec<String>, IndexError> {
        let rows: Vec<MigrationRow> = self
            .client
            .inner()
            .query("SELECT name FROM _migrations ORDER BY name")
            .fetch_all()
            .await
            .map_err(|e| ClickHouseError::Query {
                message: "failed to fetch applied migrations".into(),
                source: e,
            })?;

        Ok(rows.into_iter().map(|r| r.name).collect())
    }

    async fn record_migration(&self, name: &str) -> Result<(), IndexError> {
        let query = format!("INSERT INTO _migrations (name) VALUES ('{}')", name);
        self.client.execute(&query).await
    }
}

#[derive(Debug, Clone, clickhouse::Row, serde::Deserialize)]
struct MigrationRow {
    name: String,
}

/// Result of running migrations
#[derive(Debug, Clone)]
pub struct MigrationResult {
    /// Number of migrations applied
    pub applied: usize,
    /// Number of migrations skipped (already applied)
    pub skipped: usize,
}

impl std::fmt::Display for MigrationResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} migrations applied, {} skipped",
            self.applied, self.skipped
        )
    }
}
