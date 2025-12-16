//! com.atproto.repo.* endpoint handlers
//!
//! These serve as a record cache, reading from the raw_records table
//! populated by firehose/tap ingestion. On cache miss, fetches from
//! upstream via Slingshot and caches the result.

use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};
use jacquard::IntoStatic;
use jacquard::api::com_atproto::repo::{
    get_record::{GetRecordOutput, GetRecordRequest},
    list_records::{ListRecordsOutput, ListRecordsRequest, Record},
};
use jacquard::client::AgentSessionExt;
use jacquard::identity::resolver::IdentityResolver;
use jacquard::types::ident::AtIdentifier;
use jacquard::types::string::{AtUri, Cid};
use jacquard::types::value::Data;
use jacquard_axum::ExtractXrpc;
use serde_json::json;

use crate::server::AppState;

/// Error response for XRPC endpoints
pub struct XrpcErrorResponse {
    pub status: StatusCode,
    pub error: String,
    pub message: Option<String>,
}

impl XrpcErrorResponse {
    pub fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            error: "RecordNotFound".to_string(),
            message: Some(message.into()),
        }
    }

    pub fn invalid_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            error: "InvalidRequest".to_string(),
            message: Some(message.into()),
        }
    }

    pub fn internal_error(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            error: "InternalServerError".to_string(),
            message: Some(message.into()),
        }
    }

    pub fn auth_required(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            error: "AuthRequired".to_string(),
            message: Some(message.into()),
        }
    }

    pub fn forbidden(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::FORBIDDEN,
            error: "Forbidden".to_string(),
            message: Some(message.into()),
        }
    }
}

impl IntoResponse for XrpcErrorResponse {
    fn into_response(self) -> axum::response::Response {
        let body = json!({
            "error": self.error,
            "message": self.message,
        });
        (self.status, Json(body)).into_response()
    }
}

/// Handle com.atproto.repo.getRecord
///
/// Fetches a single record from the raw_records cache. On cache miss,
/// fetches from upstream via Slingshot and caches the result.
pub async fn get_record(
    State(state): State<AppState>,
    ExtractXrpc(args): ExtractXrpc<GetRecordRequest>,
) -> Result<Json<GetRecordOutput<'static>>, XrpcErrorResponse> {
    // Resolve identifier to DID
    let did = match &args.repo {
        AtIdentifier::Did(did) => did.clone(),
        AtIdentifier::Handle(handle) => {
            state.resolver.resolve_handle(handle).await.map_err(|e| {
                tracing::warn!("Handle resolution failed for {}: {}", handle, e);
                XrpcErrorResponse::invalid_request(format!("Could not resolve handle: {}", handle))
            })?
        }
    };

    let collection = args.collection.as_str();
    let rkey: &str = args.rkey.as_ref();

    // Query ClickHouse for the record
    let cached = state
        .clickhouse
        .get_record(did.as_str(), collection, rkey)
        .await
        .map_err(|e| {
            tracing::error!("ClickHouse query failed: {}", e);
            XrpcErrorResponse::internal_error("Database query failed")
        })?;

    if let Some(row) = cached {
        // Check if record was deleted
        if row.operation == "delete" {
            return Err(XrpcErrorResponse::not_found("Record not found"));
        }

        // Cache hit - return from ClickHouse
        let value: Data<'_> = serde_json::from_str(&row.record).map_err(|e| {
            tracing::error!("Failed to parse record JSON: {}", e);
            XrpcErrorResponse::internal_error("Failed to parse stored record")
        })?;

        let uri_str = format!("at://{}/{}/{}", did, collection, rkey);
        let uri = AtUri::new_owned(uri_str.clone()).map_err(|e| {
            tracing::error!("Failed to construct AT URI: {}", e);
            XrpcErrorResponse::internal_error("Failed to construct URI")
        })?;

        let cid = Cid::new(row.cid.as_bytes()).map_err(|e| {
            tracing::error!("Invalid CID in database: {}", e);
            XrpcErrorResponse::internal_error("Invalid CID stored")
        })?;

        // Stale-while-revalidate: check freshness in background
        let cached_cid = row.cid.clone();
        let clickhouse = state.clickhouse.clone();
        let resolver = state.resolver.clone();
        let did_str = did.as_str().to_string();
        let collection_str = collection.to_string();
        let rkey_str = rkey.to_string();

        tokio::spawn(async move {
            let uri = match AtUri::new_owned(uri_str) {
                Ok(u) => u,
                Err(_) => return,
            };

            let upstream = match resolver.fetch_record_slingshot(&uri).await {
                Ok(r) => r,
                Err(e) => {
                    tracing::debug!("Background revalidation fetch failed: {}", e);
                    return;
                }
            };

            // Check if CID changed
            let upstream_cid = upstream
                .cid
                .as_ref()
                .map(|c| c.as_str())
                .unwrap_or_default();

            if upstream_cid != cached_cid && !upstream_cid.is_empty() {
                let record_json = serde_json::to_string(&upstream.value).unwrap_or_default();
                if !record_json.is_empty() {
                    if let Err(e) = clickhouse
                        .insert_record(&did_str, &collection_str, &rkey_str, upstream_cid, &record_json)
                        .await
                    {
                        tracing::warn!("Failed to update stale cache entry: {}", e);
                    } else {
                        tracing::debug!("Updated stale cache entry for {}", uri);
                    }
                }
            }
        });

        return Ok(Json(
            GetRecordOutput {
                cid: Some(cid),
                uri,
                value,
                extra_data: None,
            }
            .into_static(),
        ));
    }

    // Cache miss - fetch from Slingshot
    tracing::debug!(
        "Cache miss for {}/{}/{}, fetching from Slingshot",
        did,
        collection,
        rkey
    );

    let uri_str = format!("at://{}/{}/{}", did, collection, rkey);
    let uri = AtUri::new_owned(uri_str.clone()).map_err(|e| {
        tracing::error!("Failed to construct AT URI: {}", e);
        XrpcErrorResponse::internal_error("Failed to construct URI")
    })?;

    let upstream = state
        .resolver
        .fetch_record_slingshot(&uri)
        .await
        .map_err(|e| {
            tracing::warn!("Slingshot fetch failed for {}: {}", uri, e);
            XrpcErrorResponse::not_found("Record not found")
        })?;

    // Cache the fetched record (fire-and-forget, don't block response)
    let cid_str = upstream
        .cid
        .as_ref()
        .map(|c| c.as_str().to_string())
        .unwrap_or_default();
    let record_json = serde_json::to_string(&upstream.value).unwrap_or_default();

    if !cid_str.is_empty() && !record_json.is_empty() {
        let clickhouse = state.clickhouse.clone();
        let did_str = did.as_str().to_string();
        let collection_str = collection.to_string();
        let rkey_str = rkey.to_string();

        tokio::spawn(async move {
            if let Err(e) = clickhouse
                .insert_record(&did_str, &collection_str, &rkey_str, &cid_str, &record_json)
                .await
            {
                tracing::warn!("Failed to cache fetched record: {}", e);
            }
        });
    }

    Ok(Json(upstream))
}

/// Handle com.atproto.repo.listRecords
///
/// Lists records for a repo+collection from the raw_records cache.
pub async fn list_records(
    State(state): State<AppState>,
    ExtractXrpc(args): ExtractXrpc<ListRecordsRequest>,
) -> Result<Json<ListRecordsOutput<'static>>, XrpcErrorResponse> {
    // Resolve identifier to DID
    let did = match &args.repo {
        AtIdentifier::Did(did) => did.clone(),
        AtIdentifier::Handle(handle) => {
            state.resolver.resolve_handle(handle).await.map_err(|e| {
                tracing::warn!("Handle resolution failed for {}: {}", handle, e);
                XrpcErrorResponse::invalid_request(format!("Could not resolve handle: {}", handle))
            })?
        }
    };

    let collection = args.collection.as_str();
    let limit = args.limit.unwrap_or(50).clamp(1, 100) as u32;
    let cursor = args.cursor.as_deref();
    let reverse = args.reverse.unwrap_or(false);

    // Query ClickHouse for records
    let rows = state
        .clickhouse
        .list_records(did.as_str(), collection, limit, cursor, reverse)
        .await
        .map_err(|e| {
            tracing::error!("ClickHouse query failed: {}", e);
            XrpcErrorResponse::internal_error("Database query failed")
        })?;

    // Convert rows to Record output
    let mut records = Vec::with_capacity(rows.len());
    for row in &rows {
        let value: Data<'_> = serde_json::from_str(&row.record).map_err(|e| {
            tracing::error!("Failed to parse record JSON: {}", e);
            XrpcErrorResponse::internal_error("Failed to parse stored record")
        })?;

        let uri_str = format!("at://{}/{}/{}", did, collection, row.rkey);
        let uri = AtUri::new_owned(uri_str)
            .map_err(|_| XrpcErrorResponse::internal_error("Failed to construct URI"))?;

        let cid = Cid::new(row.cid.as_bytes())
            .map_err(|_| XrpcErrorResponse::internal_error("Invalid CID stored"))?;

        records.push(
            Record {
                uri,
                cid,
                value,
                extra_data: None,
            }
            .into_static(),
        );
    }

    // Cursor is the rkey of the last record, if we have more
    let next_cursor = if records.len() == limit as usize {
        rows.last().map(|r| r.rkey.clone().into())
    } else {
        None
    };

    Ok(Json(ListRecordsOutput {
        records,
        cursor: next_cursor,
        extra_data: None,
    }))
}
