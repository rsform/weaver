use atrium_oauth::{AuthorizeOptions, CallbackParams};
use axum::{
    Form, Json,
    extract::{Query, State},
};
use hyper::StatusCode;
use miette::{IntoDiagnostic, Result, miette};
use serde_json::{Value, json};

use crate::{api_error::ApiError, state::AppState};

/// Passthrough callback for native endpoint
pub async fn callback_native(
    Query(params): Query<CallbackParams>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(json! ({
        "code": params.code,
        "iss": params.iss,
        "state": params.state,
    })))
}

pub async fn callback(
    State(state): State<AppState>,
    Query(params): Query<CallbackParams>,
) -> Result<Json<Value>, ApiError> {
    let oauth_client = state.oauth_client();
    let active_sessions = state.active_sessions();
    let (session, state) = oauth_client
        .callback(params)
        .await
        .expect("oauth callback failed");
    if let Some(ref state) = state {
        active_sessions.insert(state.clone(), session);
    }
    Ok(Json(json!({
        "status": "authenticated",
        "state": state
    })))
}

pub async fn get_client_metadata(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let client_metadata = &state.oauth_client().client_metadata;

    Ok(Json(serde_json::to_value(client_metadata).map_err(
        |e| ApiError::InternalError(miette!("json serialization error: {e}")),
    )?))
}

pub async fn get_jwks(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let jwks = state.oauth_client().jwks();

    Ok(Json(serde_json::to_value(jwks).map_err(|e| {
        ApiError::InternalError(miette!("json serialization error: {e}"))
    })?))
}

pub async fn login(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    Ok(Json(json!({ "status": "ok" })))
}

pub struct AuthorizeParams(String, AuthorizeOptions);

pub async fn authorize(
    State(state): State<AppState>,
    Form(AuthorizeParams(endpoint, options)): Form<AuthorizeParams>,
) -> Result<Json<Value>, ApiError> {
    let url = state
        .oauth_client()
        .authorize(endpoint, options)
        .await
        .map_err(|e| ApiError::InternalError(miette!("oauth authorize error: {e}")))?;
    Ok(Json(json!({ "url": url })))
}

pub async fn logout(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    Ok(Json(json!({ "status": "ok" })))
}
