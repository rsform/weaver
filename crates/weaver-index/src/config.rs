use crate::error::{ConfigError, IndexError};
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

/// Combined configuration for the indexer
#[derive(Debug, Clone)]
pub struct Config {
    pub clickhouse: ClickHouseConfig,
    pub firehose: FirehoseConfig,
}

impl Config {
    /// Load all configuration from environment variables.
    pub fn from_env() -> Result<Self, IndexError> {
        Ok(Self {
            clickhouse: ClickHouseConfig::from_env()?,
            firehose: FirehoseConfig::from_env()?,
        })
    }
}
