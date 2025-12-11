use crate::error::{ConfigError, IndexError};
use dashmap::DashSet;
use url::Url;

/// ClickHouse connection configuration
#[derive(Debug, Clone)]
pub struct ClickHouseConfig {
    pub url: Url,
    pub database: String,
    pub user: String,
    pub password: String,
}

impl ClickHouseConfig {
    /// Load configuration from environment variables.
    ///
    /// Required env vars:
    /// - `CLICKHOUSE_URL`: Full URL including protocol (e.g., `https://xyz.clickhouse.cloud:8443`)
    /// - `CLICKHOUSE_DATABASE`: Database name
    /// - `CLICKHOUSE_USER`: Username
    /// - `CLICKHOUSE_PASSWORD`: Password
    pub fn from_env() -> Result<Self, IndexError> {
        let url_str = std::env::var("CLICKHOUSE_URL").map_err(|_| ConfigError::MissingEnv {
            var: "CLICKHOUSE_URL",
        })?;

        let url = Url::parse(&url_str).map_err(|e| ConfigError::UrlParse {
            url: url_str,
            message: e.to_string(),
        })?;

        let database =
            std::env::var("CLICKHOUSE_DATABASE").map_err(|_| ConfigError::MissingEnv {
                var: "CLICKHOUSE_DATABASE",
            })?;

        let user = std::env::var("CLICKHOUSE_USER").map_err(|_| ConfigError::MissingEnv {
            var: "CLICKHOUSE_USER",
        })?;

        let password =
            std::env::var("CLICKHOUSE_PASSWORD").map_err(|_| ConfigError::MissingEnv {
                var: "CLICKHOUSE_PASSWORD",
            })?;

        Ok(Self {
            url,
            database,
            user,
            password,
        })
    }
}

/// Firehose relay configuration
#[derive(Debug, Clone)]
pub struct FirehoseConfig {
    pub relay_url: Url,
    pub cursor: Option<i64>,
}

impl FirehoseConfig {
    /// Default relay URL (Bluesky network)
    pub const DEFAULT_RELAY: &'static str = "wss://bsky.network";

    /// Load configuration from environment variables.
    ///
    /// Optional env vars:
    /// - `FIREHOSE_RELAY_URL`: Relay WebSocket URL (default: wss://bsky.network)
    /// - `FIREHOSE_CURSOR`: Starting cursor position (default: none, starts from live)
    pub fn from_env() -> Result<Self, IndexError> {
        let relay_str =
            std::env::var("FIREHOSE_RELAY_URL").unwrap_or_else(|_| Self::DEFAULT_RELAY.to_string());

        let relay_url = Url::parse(&relay_str).map_err(|e| ConfigError::UrlParse {
            url: relay_str,
            message: e.to_string(),
        })?;

        let cursor = std::env::var("FIREHOSE_CURSOR")
            .ok()
            .and_then(|s| s.parse().ok());

        Ok(Self { relay_url, cursor })
    }
}

use smol_str::{SmolStr, ToSmolStr};

/// Pre-parsed collection filter for efficient matching
#[derive(Debug, Clone)]
pub struct CollectionFilter {
    /// Prefix patterns (from "foo.*" -> "foo.")
    prefixes: Vec<SmolStr>,
    /// Exact match patterns (HashSet for O(1) lookup)
    exact: DashSet<SmolStr>,
    /// True if filter is empty (accept all)
    accept_all: bool,
}

impl CollectionFilter {
    /// Parse filter patterns into prefixes and exact matches
    pub fn new(patterns: Vec<SmolStr>) -> Self {
        let mut prefixes = Vec::new();
        let exact = DashSet::new();

        for pattern in patterns {
            if let Some(prefix) = pattern.strip_suffix('*') {
                prefixes.push(SmolStr::new(prefix));
            } else {
                exact.insert(SmolStr::new(&pattern));
            }
        }

        let accept_all = prefixes.is_empty() && exact.is_empty();
        Self {
            prefixes,
            exact,
            accept_all,
        }
    }

    /// Check if a collection matches any pattern
    #[inline]
    pub fn matches(&self, collection: &str) -> bool {
        if self.accept_all {
            return true;
        }

        // O(1) exact match check first
        if self.exact.contains(collection) {
            return true;
        }

        // Prefix check - for small N, linear scan is fine
        // Accumulate without early return to help branch predictor
        let mut matched = false;
        for prefix in &self.prefixes {
            matched |= collection.starts_with(prefix.as_str());
        }
        matched
    }
}

/// Indexer runtime configuration
#[derive(Debug, Clone)]
pub struct IndexerConfig {
    /// Maximum records to batch before flushing to ClickHouse
    pub batch_size: usize,
    /// Maximum time (ms) before flushing even if batch isn't full
    pub flush_interval_ms: u64,
    /// Collection filter (pre-parsed patterns)
    pub collections: CollectionFilter,
}

impl Default for IndexerConfig {
    fn default() -> Self {
        Self {
            batch_size: 1000,
            flush_interval_ms: 1000,
            collections: CollectionFilter::new(vec![
                SmolStr::new_static("sh.weaver.*"),
                SmolStr::new_static("app.bsky.actor.profile"),
            ]),
        }
    }
}

impl IndexerConfig {
    /// Load configuration from environment variables.
    ///
    /// Optional env vars:
    /// - `INDEXER_BATCH_SIZE`: Max records per batch (default: 1000)
    /// - `INDEXER_FLUSH_INTERVAL_MS`: Max ms between flushes (default: 1000)
    /// - `INDEXER_COLLECTIONS`: Comma-separated collection patterns (default: sh.weaver.*,app.bsky.actor.profile)
    ///   Use * suffix for prefix matching, e.g., "sh.weaver.*" matches all sh.weaver.* collections
    pub fn from_env() -> Self {
        let batch_size = std::env::var("INDEXER_BATCH_SIZE")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(1000);

        let flush_interval_ms = std::env::var("INDEXER_FLUSH_INTERVAL_MS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(1000);

        let patterns: Vec<SmolStr> = std::env::var("INDEXER_COLLECTIONS")
            .map(|s| s.split(',').map(|p| p.trim().to_smolstr()).collect())
            .unwrap_or_else(|_| {
                vec![
                    SmolStr::new_static("sh.weaver.*"),
                    SmolStr::new_static("app.bsky.actor.profile"),
                ]
            });

        Self {
            batch_size,
            flush_interval_ms,
            collections: CollectionFilter::new(patterns),
        }
    }
}

/// Combined configuration for the indexer
#[derive(Debug, Clone)]
pub struct Config {
    pub clickhouse: ClickHouseConfig,
    pub firehose: FirehoseConfig,
    pub indexer: IndexerConfig,
}

impl Config {
    /// Load all configuration from environment variables.
    pub fn from_env() -> Result<Self, IndexError> {
        Ok(Self {
            clickhouse: ClickHouseConfig::from_env()?,
            firehose: FirehoseConfig::from_env()?,
            indexer: IndexerConfig::from_env(),
        })
    }
}
