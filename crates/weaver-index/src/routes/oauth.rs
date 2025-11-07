use axum::{
    Form, Json,
    extract::{Query, State},
};
use jacquard::{
    IntoStatic,
    oauth::{
        atproto::atproto_client_metadata,
        types::{AuthorizeOptions, CallbackParams},
    },
};
use miette::{IntoDiagnostic, Result, miette};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{api_error::ApiError, state::AppState};

/// Passthrough callback for native endpoint
pub async fn callback_native(
    Query(params): Query<CallbackParams<'_>>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(json!({
        "code": params.code,
        "iss": params.iss,
        "state": params.state,
    })))
}

/// OAuth callback handler
pub async fn callback(
    State(state): State<AppState>,
    Query(params): Query<CallbackParams<'_>>,
) -> Result<Json<Value>, ApiError> {
    let oauth_client = state.oauth_client();
    let active_sessions = state.active_sessions();

    let session = oauth_client
        .callback(params)
        .await
        .map_err(|e| ApiError::InternalError(miette!("oauth callback failed: {e}")))?;

    let (did, session_id) = session.session_info().await;
    let did = did.to_string();
    let session_id = session_id.clone().into_static();

    active_sessions.insert(session_id.to_string(), session);

    Ok(Json(json!({
        "status": "authenticated",
        "did": did,
        "session_id": session_id,
    })))
}

/// Get OAuth client metadata
pub async fn get_client_metadata(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let metadata = atproto_client_metadata(
        state.oauth_client().registry.client_data.config.clone(),
        &state.oauth_client().registry.client_data.keyset,
    )
    .map_err(|e| ApiError::InternalError(miette!("couldn't get oauth metadata: {e}")))?;

    Ok(Json(serde_json::to_value(metadata).map_err(|e| {
        ApiError::InternalError(miette!("json serialization error: {e}"))
    })?))
}

/// Get JWKS (public keys)
pub async fn get_jwks(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let jwks = state.oauth_client().jwks();

    Ok(Json(serde_json::to_value(jwks).map_err(|e| {
        ApiError::InternalError(miette!("json serialization error: {e}"))
    })?))
}

/// Login stub
pub async fn login(State(_state): State<AppState>) -> Result<Json<Value>, ApiError> {
    Ok(Json(json!({ "status": "ok" })))
}

#[derive(Deserialize)]
pub struct AuthorizeParams {
    pub handle: String,
}

/// Start OAuth authorization flow
pub async fn authorize(
    State(state): State<AppState>,
    Form(params): Form<AuthorizeParams>,
) -> Result<Json<Value>, ApiError> {
    let url = state
        .oauth_client()
        .start_auth(params.handle, AuthorizeOptions::default())
        .await
        .map_err(|e| ApiError::InternalError(miette!("oauth authorize error: {e}")))?;

    Ok(Json(json!({ "url": url })))
}

/// Logout stub
pub async fn logout(State(_state): State<AppState>) -> Result<Json<Value>, ApiError> {
    Ok(Json(json!({ "status": "ok" })))
}
