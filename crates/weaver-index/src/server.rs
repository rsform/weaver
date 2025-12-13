use std::net::SocketAddr;
use std::sync::Arc;

use axum::{Json, Router, extract::State, http::StatusCode, response::IntoResponse, routing::get};
use jacquard::api::com_atproto::repo::{
    get_record::GetRecordRequest, list_records::ListRecordsRequest,
};
use weaver_api::sh_weaver::actor::get_profile::GetProfileRequest;
use weaver_api::sh_weaver::notebook::{
    get_entry::GetEntryRequest,
    resolve_entry::ResolveEntryRequest,
    resolve_notebook::ResolveNotebookRequest,
};
use jacquard::client::UnauthenticatedSession;
use jacquard::identity::JacquardResolver;
use jacquard_axum::IntoRouter;
use serde::Serialize;
use tower_http::trace::TraceLayer;
use tracing::info;

use crate::clickhouse::Client;
use crate::config::ShardConfig;
use crate::endpoints::{actor, notebook, repo};
use crate::error::{IndexError, ServerError};
use crate::sqlite::ShardRouter;

pub use weaver_common::telemetry::{self, TelemetryConfig};

/// Identity resolver type (unauthenticated, just for handle/DID resolution)
pub type Resolver = UnauthenticatedSession<JacquardResolver>;

/// Shared application state
#[derive(Clone)]
pub struct AppState {
    pub clickhouse: Arc<Client>,
    pub shards: Arc<ShardRouter>,
    pub resolver: Resolver,
}

impl AppState {
    pub fn new(clickhouse: Client, shard_config: ShardConfig) -> Self {
        Self {
            clickhouse: Arc::new(clickhouse),
            shards: Arc::new(ShardRouter::new(shard_config.base_path)),
            resolver: UnauthenticatedSession::new_slingshot(),
        }
    }
}

/// Build the axum router with all XRPC endpoints
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/xrpc/_health", get(health))
        .route("/metrics", get(metrics))
        // com.atproto.repo.* endpoints (record cache)
        .merge(GetRecordRequest::into_router(repo::get_record))
        .merge(ListRecordsRequest::into_router(repo::list_records))
        // sh.weaver.actor.* endpoints
        .merge(GetProfileRequest::into_router(actor::get_profile))
        // sh.weaver.notebook.* endpoints
        .merge(ResolveNotebookRequest::into_router(notebook::resolve_notebook))
        .merge(GetEntryRequest::into_router(notebook::get_entry))
        .merge(ResolveEntryRequest::into_router(notebook::resolve_entry))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

/// Prometheus metrics endpoint
async fn metrics() -> String {
    telemetry::render()
}

/// Health check response
#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
    clickhouse: bool,
    shard_count: usize,
}

/// Health check endpoint
///
/// Returns 200 OK with stats if healthy, 503 if ClickHouse unreachable.
async fn health(State(state): State<AppState>) -> impl IntoResponse {
    let clickhouse_ok = state.clickhouse.execute("SELECT 1").await.is_ok();
    let shard_count = state.shards.shard_count();

    let response = HealthResponse {
        status: if clickhouse_ok { "ok" } else { "degraded" },
        clickhouse: clickhouse_ok,
        shard_count,
    };

    let status = if clickhouse_ok {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };

    (status, Json(response))
}

/// Server configuration
#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: "0.0.0.0".to_string(),
            port: 3000,
        }
    }
}

impl ServerConfig {
    pub fn from_env() -> Self {
        let host = std::env::var("SERVER_HOST").unwrap_or_else(|_| "0.0.0.0".to_string());
        let port = std::env::var("SERVER_PORT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(3000);

        Self { host, port }
    }

    pub fn addr(&self) -> SocketAddr {
        format!("{}:{}", self.host, self.port)
            .parse()
            .expect("valid socket address")
    }
}

/// Run the HTTP server
pub async fn run(state: AppState, config: ServerConfig) -> Result<(), IndexError> {
    let addr = config.addr();
    let app = router(state);

    info!("Starting HTTP server on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|e| ServerError::Bind { addr, source: e })?;

    axum::serve(listener, app)
        .await
        .map_err(|e| ServerError::Serve { source: e })?;

    Ok(())
}
