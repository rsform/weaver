//! Telemetry infrastructure for weaver services.
//!
//! Provides:
//! - Prometheus metrics with `/metrics` endpoint
//! - Tracing with pretty console output + optional Loki push
//!
//! # Usage
//!
//! ```ignore
//! use weaver_common::telemetry::{self, TelemetryConfig};
//!
//! #[tokio::main]
//! async fn main() {
//!     // Initialize telemetry (metrics + tracing)
//!     let config = TelemetryConfig::from_env("weaver-index");
//!     telemetry::init(config).await;
//!
//!     // Mount the metrics endpoint in your axum router
//!     let app = Router::new()
//!         .route("/metrics", get(|| async { telemetry::render() }));
//!
//!     // Use metrics
//!     metrics::counter!("requests_total").increment(1);
//!
//!     // Use tracing (goes to both console and loki if configured)
//!     tracing::info!("server started");
//! }
//! ```

use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};
use std::sync::OnceLock;
use tracing::Level;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer};

static PROMETHEUS_HANDLE: OnceLock<PrometheusHandle> = OnceLock::new();

/// Telemetry configuration
#[derive(Debug, Clone)]
pub struct TelemetryConfig {
    /// Service name for labeling (e.g., "weaver-index", "weaver-app")
    pub service_name: String,
    /// Loki push URL (e.g., "http://localhost:3100"). None disables Loki.
    pub loki_url: Option<String>,
    /// Console log level (default: INFO, DEBUG in debug builds)
    pub console_level: Level,
}

impl TelemetryConfig {
    /// Load config from environment variables.
    ///
    /// - `LOKI_URL`: Loki push endpoint (optional)
    /// - `RUST_LOG`: Standard env filter (optional, overrides console_level)
    pub fn from_env(service_name: impl Into<String>) -> Self {
        let console_level = if cfg!(debug_assertions) {
            Level::DEBUG
        } else {
            Level::INFO
        };

        Self {
            service_name: service_name.into(),
            loki_url: std::env::var("LOKI_URL").ok(),
            console_level,
        }
    }
}

/// Initialize telemetry (metrics + tracing).
///
/// Call once at application startup. If `LOKI_URL` is set, spawns a background
/// task to push logs to Loki.
pub async fn init(config: TelemetryConfig) {
    // Initialize prometheus metrics
    init_metrics();

    // Initialize tracing
    init_tracing(config).await;
}

/// Initialize just the prometheus metrics recorder.
pub fn init_metrics() -> &'static PrometheusHandle {
    PROMETHEUS_HANDLE.get_or_init(|| {
        PrometheusBuilder::new()
            .install_recorder()
            .expect("failed to install prometheus recorder")
    })
}

/// Initialize tracing with console + optional Loki layers.
async fn init_tracing(config: TelemetryConfig) {
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        EnvFilter::new(format!(
            "{}",
            config.console_level.as_str().to_lowercase()
        ))
    });

    // Pretty console layer for human-readable stdout
    let console_layer = tracing_subscriber::fmt::layer()
        .with_target(true)
        .with_thread_ids(false)
        .with_file(false)
        .with_line_number(false)
        .compact()
        .with_filter(env_filter);

    // Optional Loki layer for structured logs
    if let Some(loki_url) = config.loki_url {
        match tracing_loki::url::Url::parse(&loki_url) {
            Ok(url) => {
                let (loki_layer, loki_task) = tracing_loki::builder()
                    .label("service", config.service_name.clone())
                    .expect("invalid label")
                    .build_url(url)
                    .expect("failed to build loki layer");

                tracing_subscriber::registry()
                    .with(console_layer)
                    .with(loki_layer)
                    .init();

                // Spawn the background task that pushes to Loki
                tokio::spawn(loki_task);

                tracing::info!(
                    service = %config.service_name,
                    loki_url = %loki_url,
                    "telemetry initialized with loki"
                );
            }
            Err(e) => {
                // Invalid URL - fall back to console only
                tracing_subscriber::registry().with(console_layer).init();

                tracing::warn!(
                    error = %e,
                    loki_url = %loki_url,
                    "invalid LOKI_URL, falling back to console only"
                );
            }
        }
    } else {
        // No Loki URL - console only
        tracing_subscriber::registry().with(console_layer).init();

        tracing::debug!(
            service = %config.service_name,
            "telemetry initialized (console only, set LOKI_URL to enable loki)"
        );
    }
}

/// Get the prometheus handle.
pub fn handle() -> &'static PrometheusHandle {
    PROMETHEUS_HANDLE.get_or_init(|| {
        PrometheusBuilder::new()
            .install_recorder()
            .expect("failed to install prometheus recorder")
    })
}

/// Render metrics in prometheus text format.
pub fn render() -> String {
    handle().render()
}

// Re-export the metrics crate for convenience
pub use metrics::{counter, gauge, histogram};
