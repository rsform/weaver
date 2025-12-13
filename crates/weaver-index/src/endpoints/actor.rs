//! sh.weaver.actor.* endpoint handlers

use axum::{Json, extract::State};
use jacquard::IntoStatic;
use jacquard::cowstr::ToCowStr;
use jacquard::identity::resolver::IdentityResolver;
use jacquard::types::ident::AtIdentifier;
use jacquard::types::string::{Did, Handle};
use jacquard_axum::ExtractXrpc;
use weaver_api::sh_weaver::actor::{
    ProfileDataView, ProfileDataViewInner, ProfileView,
    get_profile::{GetProfileOutput, GetProfileRequest},
};

use crate::endpoints::repo::XrpcErrorResponse;
use crate::server::AppState;

/// Handle sh.weaver.actor.getProfile
///
/// Returns a profile view with counts for the requested actor.
pub async fn get_profile(
    State(state): State<AppState>,
    ExtractXrpc(args): ExtractXrpc<GetProfileRequest>,
) -> Result<Json<GetProfileOutput<'static>>, XrpcErrorResponse> {
    // Resolve identifier to DID
    let did = resolve_actor(&state, &args.actor).await?;
    let did_str = did.as_str();

    // Fetch profile with counts from ClickHouse
    let profile_data = state
        .clickhouse
        .get_profile_with_counts(did_str)
        .await
        .map_err(|e| {
            tracing::error!("Failed to get profile: {}", e);
            XrpcErrorResponse::internal_error("Database query failed")
        })?;

    let Some(data) = profile_data else {
        return Err(XrpcErrorResponse::not_found("Profile not found"));
    };

    // Build the response
    let profile = &data.profile;

    // Determine handle - use from profile row, or resolve if empty
    let handle_str = if profile.handle.is_empty() {
        // Try to resolve DID -> handle
        match state.clickhouse.resolve_did_to_handle(did_str).await {
            Ok(Some(mapping)) => mapping.handle.to_string(),
            _ => {
                // Last resort: use a placeholder or try external resolver
                // For now, use the DID as handle (not ideal but functional)
                did_str.to_string()
            }
        }
    } else {
        profile.handle.to_string()
    };

    let handle = Handle::new(&handle_str).map_err(|e| {
        tracing::error!("Invalid handle in database: {}", e);
        XrpcErrorResponse::internal_error("Invalid handle stored")
    })?;

    // Build ProfileView (weaver native profile)
    let inner_profile = ProfileView::new()
        .did(did.clone())
        .handle(handle)
        .maybe_display_name(non_empty_str(&profile.display_name))
        .maybe_description(non_empty_str(&profile.description))
        // TODO: avatar/banner need URL construction from CID
        .build();

    let inner = ProfileDataViewInner::ProfileView(Box::new(inner_profile));

    // Build ProfileDataView with counts
    let counts = data.counts.as_ref();

    let output = ProfileDataView::new()
        .inner(inner)
        .maybe_follower_count(counts.map(|c| c.follower_count))
        .maybe_following_count(counts.map(|c| c.following_count))
        .maybe_notebook_count(counts.map(|c| c.notebook_count))
        .maybe_entry_count(counts.map(|c| c.entry_count))
        .build();

    Ok(Json(
        GetProfileOutput {
            value: output,
            extra_data: None,
        }
        .into_static(),
    ))
}

/// Resolve an AtIdentifier to a DID.
///
/// For handles: tries handle_mappings first, falls back to external resolver.
/// For DIDs: returns as-is.
pub async fn resolve_actor<'a>(
    state: &AppState,
    actor: &AtIdentifier<'a>,
) -> Result<Did<'static>, XrpcErrorResponse> {
    match actor {
        AtIdentifier::Did(did) => Ok(did.clone().into_static()),
        AtIdentifier::Handle(handle) => {
            let handle_str = handle.as_str();

            // Try handle_mappings first
            match state.clickhouse.resolve_handle(handle_str).await {
                Ok(Some(mapping)) => {
                    let did = Did::new(&mapping.did).map_err(|e| {
                        tracing::error!("Invalid DID in handle_mappings: {}", e);
                        XrpcErrorResponse::internal_error("Invalid DID stored")
                    })?;
                    return Ok(did.into_static());
                }
                Ok(None) => {
                    tracing::debug!("Handle {} not in cache, trying resolver", handle_str);
                }
                Err(e) => {
                    tracing::warn!("Handle lookup failed, trying resolver: {}", e);
                }
            }

            // Fall back to external resolver
            let resolved = state.resolver.resolve_handle(handle).await.map_err(|e| {
                tracing::warn!("Handle resolution failed for {}: {}", handle, e);
                XrpcErrorResponse::invalid_request(format!("Could not resolve handle: {}", handle))
            })?;

            // Cache the result (fire-and-forget)
            let clickhouse = state.clickhouse.clone();
            let handle_owned = handle_str.to_string();
            let did_owned = resolved.as_str().to_string();
            tokio::spawn(async move {
                if let Err(e) = clickhouse
                    .cache_handle_resolution(&handle_owned, &did_owned)
                    .await
                {
                    tracing::warn!("Failed to cache handle resolution: {}", e);
                }
            });

            Ok(resolved.into_static())
        }
    }
}

/// Convert SmolStr to Option<CowStr> if non-empty
fn non_empty_str(s: &smol_str::SmolStr) -> Option<jacquard::CowStr<'static>> {
    if s.is_empty() {
        None
    } else {
        Some(s.to_cowstr().into_static())
    }
}
