//! app.bsky.* passthrough endpoints
//!
//! These forward requests to the Bluesky appview.

use axum::{Json, extract::State};
use jacquard::prelude::*;
use jacquard_axum::ExtractXrpc;
use weaver_api::app_bsky::actor::get_profile::{GetProfileOutput, GetProfileRequest};
use weaver_api::app_bsky::feed::get_posts::{GetPostsOutput, GetPostsRequest};

use crate::endpoints::repo::XrpcErrorResponse;
use crate::server::AppState;

/// Handle app.bsky.actor.getProfile (passthrough)
pub async fn get_profile(
    State(state): State<AppState>,
    ExtractXrpc(args): ExtractXrpc<GetProfileRequest>,
) -> Result<Json<GetProfileOutput<'static>>, XrpcErrorResponse> {
    let response = state.resolver.send(args).await.map_err(|e| {
        tracing::warn!("Appview getProfile failed: {}", e);
        XrpcErrorResponse::internal_error("Failed to fetch profile from appview")
    })?;

    let output = response.into_output().map_err(|e| {
        tracing::warn!("Failed to parse getProfile response: {}", e);
        XrpcErrorResponse::internal_error("Failed to parse appview response")
    })?;

    Ok(Json(output))
}

/// Handle app.bsky.feed.getPosts (passthrough)
pub async fn get_posts(
    State(state): State<AppState>,
    ExtractXrpc(args): ExtractXrpc<GetPostsRequest>,
) -> Result<Json<GetPostsOutput<'static>>, XrpcErrorResponse> {
    let response = state.resolver.send(args).await.map_err(|e| {
        tracing::warn!("Appview getPosts failed: {}", e);
        XrpcErrorResponse::internal_error("Failed to fetch posts from appview")
    })?;

    let output = response.into_output().map_err(|e| {
        tracing::warn!("Failed to parse getPosts response: {}", e);
        XrpcErrorResponse::internal_error("Failed to parse appview response")
    })?;

    Ok(Json(output))
}
