use std::net::SocketAddr;
use std::sync::Arc;

use axum::{
    Json, Router,
    extract::State,
    http::{StatusCode, header},
    response::{Html, IntoResponse},
    routing::get,
};
use jacquard::api::com_atproto::repo::{
    get_record::GetRecordRequest, list_records::ListRecordsRequest,
};
use jacquard::client::UnauthenticatedSession;
use jacquard::identity::JacquardResolver;
use jacquard::types::did_doc::DidDocument;
use jacquard::types::string::Did;
use jacquard_axum::IntoRouter;
use jacquard_axum::did_web::did_web_router;
use jacquard_axum::service_auth::ServiceAuth;
use serde::Serialize;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tracing::info;
use weaver_api::app_bsky::actor::get_profile::GetProfileRequest as BskyGetProfileRequest;
use weaver_api::app_bsky::feed::get_posts::GetPostsRequest as BskyGetPostsRequest;
use weaver_api::com_atproto::identity::resolve_handle::ResolveHandleRequest;
use weaver_api::sh_weaver::actor::{
    get_actor_entries::GetActorEntriesRequest, get_actor_notebooks::GetActorNotebooksRequest,
    get_profile::GetProfileRequest,
};
use weaver_api::sh_weaver::collab::get_collaboration_state::GetCollaborationStateRequest;
use weaver_api::sh_weaver::collab::get_resource_participants::GetResourceParticipantsRequest;
use weaver_api::sh_weaver::collab::get_resource_sessions::GetResourceSessionsRequest;
use weaver_api::sh_weaver::edit::get_contributors::GetContributorsRequest;
use weaver_api::sh_weaver::edit::get_edit_history::GetEditHistoryRequest;
use weaver_api::sh_weaver::edit::list_drafts::ListDraftsRequest;
use weaver_api::sh_weaver::notebook::{
    get_book_entry::GetBookEntryRequest, get_entry::GetEntryRequest,
    get_entry_feed::GetEntryFeedRequest, get_entry_notebooks::GetEntryNotebooksRequest,
    get_notebook::GetNotebookRequest, get_notebook_feed::GetNotebookFeedRequest,
    resolve_entry::ResolveEntryRequest,
    resolve_global_notebook::ResolveGlobalNotebookRequest, resolve_notebook::ResolveNotebookRequest,
};

use crate::clickhouse::Client;
use crate::config::ShardConfig;
use crate::endpoints::{actor, bsky, collab, edit, identity, notebook, repo};
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
    /// Our service DID (expected audience for service auth JWTs)
    pub service_did: Did<'static>,
}

impl AppState {
    pub fn new(clickhouse: Client, shard_config: ShardConfig, service_did: Did<'static>) -> Self {
        Self {
            clickhouse: Arc::new(clickhouse),
            shards: Arc::new(ShardRouter::new(shard_config.base_path)),
            resolver: UnauthenticatedSession::new_public(),
            service_did,
        }
    }
}

impl ServiceAuth for AppState {
    type Resolver = UnauthenticatedSession<JacquardResolver>;

    fn service_did(&self) -> &Did<'_> {
        &self.service_did
    }

    fn resolver(&self) -> &Self::Resolver {
        &self.resolver
    }

    fn require_lxm(&self) -> bool {
        true
    }
}

/// Build the axum router with all XRPC endpoints
pub fn router(state: AppState, did_doc: DidDocument<'static>) -> Router {
    Router::new()
        .route("/", get(landing))
        .route(
            "/assets/IoskeleyMono-Regular.woff2",
            get(font_ioskeley_regular),
        )
        .route("/assets/IoskeleyMono-Bold.woff2", get(font_ioskeley_bold))
        .route(
            "/assets/IoskeleyMono-Italic.woff2",
            get(font_ioskeley_italic),
        )
        .route("/xrpc/_health", get(health))
        .route("/metrics", get(metrics))
        // com.atproto.identity.* endpoints
        .merge(ResolveHandleRequest::into_router(identity::resolve_handle))
        // com.atproto.repo.* endpoints (record cache)
        .merge(GetRecordRequest::into_router(repo::get_record))
        .merge(ListRecordsRequest::into_router(repo::list_records))
        // app.bsky.* passthrough endpoints
        .merge(BskyGetProfileRequest::into_router(bsky::get_profile))
        .merge(BskyGetPostsRequest::into_router(bsky::get_posts))
        // sh.weaver.actor.* endpoints
        .merge(GetProfileRequest::into_router(actor::get_profile))
        .merge(GetActorNotebooksRequest::into_router(
            actor::get_actor_notebooks,
        ))
        .merge(GetActorEntriesRequest::into_router(
            actor::get_actor_entries,
        ))
        // sh.weaver.notebook.* endpoints
        .merge(ResolveNotebookRequest::into_router(
            notebook::resolve_notebook,
        ))
        .merge(GetNotebookRequest::into_router(notebook::get_notebook))
        .merge(GetEntryRequest::into_router(notebook::get_entry))
        .merge(ResolveEntryRequest::into_router(notebook::resolve_entry))
        .merge(GetNotebookFeedRequest::into_router(
            notebook::get_notebook_feed,
        ))
        .merge(GetEntryFeedRequest::into_router(notebook::get_entry_feed))
        .merge(GetBookEntryRequest::into_router(notebook::get_book_entry))
        .merge(GetEntryNotebooksRequest::into_router(
            notebook::get_entry_notebooks,
        ))
        .merge(ResolveGlobalNotebookRequest::into_router(
            notebook::resolve_global_notebook,
        ))
        // sh.weaver.collab.* endpoints
        .merge(GetResourceParticipantsRequest::into_router(
            collab::get_resource_participants,
        ))
        .merge(GetCollaborationStateRequest::into_router(
            collab::get_collaboration_state,
        ))
        .merge(GetResourceSessionsRequest::into_router(
            collab::get_resource_sessions,
        ))
        // sh.weaver.edit.* endpoints
        .merge(GetEditHistoryRequest::into_router(edit::get_edit_history))
        .merge(GetContributorsRequest::into_router(edit::get_contributors))
        .merge(ListDraftsRequest::into_router(edit::list_drafts))
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive().max_age(std::time::Duration::from_secs(86400)))
        .with_state(state)
        .merge(did_web_router(did_doc))
}

/// Prometheus metrics endpoint
async fn metrics() -> String {
    telemetry::render()
}

// Embedded font files
const IOSKELEY_MONO_REGULAR: &[u8] =
    include_bytes!("../../weaver-app/assets/fonts/ioskeley-mono/IoskeleyMono-Regular.woff2");
const IOSKELEY_MONO_BOLD: &[u8] =
    include_bytes!("../../weaver-app/assets/fonts/ioskeley-mono/IoskeleyMono-Bold.woff2");
const IOSKELEY_MONO_ITALIC: &[u8] =
    include_bytes!("../../weaver-app/assets/fonts/ioskeley-mono/IoskeleyMono-Italic.woff2");

/// Serve the Ioskeley Mono Regular font
async fn font_ioskeley_regular() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "font/woff2")],
        IOSKELEY_MONO_REGULAR,
    )
}
/// Serve the Ioskeley Mono Regular font
async fn font_ioskeley_bold() -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "font/woff2")], IOSKELEY_MONO_BOLD)
}

/// Serve the Ioskeley Mono Regular font
async fn font_ioskeley_italic() -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "font/woff2")], IOSKELEY_MONO_ITALIC)
}

const LANDING_HTML: &str = include_str!("./landing.html");

/// Landing page
async fn landing() -> Html<&'static str> {
    Html(LANDING_HTML)
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
    /// Service DID for this indexer (used as expected audience for service auth)
    pub service_did: Did<'static>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: "0.0.0.0".to_string(),
            port: 3000,
            // Default to a placeholder - should be overridden in production
            service_did: Did::new_static("did:web:index.weaver.sh").unwrap(),
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
        let service_did = std::env::var("SERVICE_DID")
            .ok()
            .and_then(|s| Did::new_owned(s).ok())
            .unwrap_or_else(|| Did::new_static("did:web:index.weaver.sh").unwrap());

        Self {
            host,
            port,
            service_did,
        }
    }

    pub fn addr(&self) -> SocketAddr {
        format!("{}:{}", self.host, self.port)
            .parse()
            .expect("valid socket address")
    }
}

/// Run the HTTP server
pub async fn run(
    state: AppState,
    config: ServerConfig,
    did_doc: DidDocument<'static>,
) -> Result<(), IndexError> {
    let addr = config.addr();
    let app = router(state, did_doc);

    info!("Starting HTTP server on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|e| ServerError::Bind { addr, source: e })?;

    axum::serve(listener, app)
        .await
        .map_err(|e| ServerError::Serve { source: e })?;

    Ok(())
}
