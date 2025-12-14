//! Collaboration endpoint handlers

use std::collections::HashMap;

use axum::{Json, extract::State};
use jacquard::IntoStatic;
use jacquard::cowstr::ToCowStr;
use jacquard::types::datetime::Datetime;
use jacquard::types::string::{AtUri, Cid, Did, Handle};
use jacquard_axum::ExtractXrpc;
use jacquard_axum::service_auth::ExtractOptionalServiceAuth;

use weaver_api::com_atproto::repo::strong_ref::StrongRef;
use weaver_api::sh_weaver::actor::ProfileViewBasic;
use weaver_api::sh_weaver::collab::get_collaboration_state::{
    GetCollaborationStateOutput, GetCollaborationStateRequest,
};
use weaver_api::sh_weaver::collab::get_resource_participants::{
    GetResourceParticipantsOutput, GetResourceParticipantsRequest,
};
use weaver_api::sh_weaver::collab::{CollaborationStateView, ParticipantStateView};

use crate::clickhouse::{CollaboratorRow, ProfileRow};
use crate::endpoints::actor::Viewer;
use crate::endpoints::non_empty_str;
use crate::endpoints::repo::XrpcErrorResponse;
use crate::server::AppState;

/// Handle sh.weaver.collab.getResourceParticipants
///
/// Returns owner and collaborators who can edit the resource.
pub async fn get_resource_participants(
    State(state): State<AppState>,
    ExtractOptionalServiceAuth(viewer): ExtractOptionalServiceAuth,
    ExtractXrpc(args): ExtractXrpc<GetResourceParticipantsRequest>,
) -> Result<Json<GetResourceParticipantsOutput<'static>>, XrpcErrorResponse> {
    let _viewer: Viewer = viewer;
    let viewer_did: Option<&str> = _viewer.as_ref().map(|v| v.did().as_str());

    let resource_uri = args.resource.as_str();

    // Get all permissions for the resource
    let permissions = state
        .clickhouse
        .get_resource_permissions(resource_uri)
        .await
        .map_err(|e| {
            tracing::error!("Failed to get resource permissions: {}", e);
            XrpcErrorResponse::internal_error("Database query failed")
        })?;

    if permissions.is_empty() {
        return Err(XrpcErrorResponse::not_found("Resource not found"));
    }

    // Collect all DIDs for profile hydration
    let all_dids: Vec<&str> = permissions.iter().map(|p| p.grantee_did.as_str()).collect();

    // Batch fetch profiles
    let profiles = state
        .clickhouse
        .get_profiles_batch(&all_dids)
        .await
        .map_err(|e| {
            tracing::error!("Failed to batch fetch profiles: {}", e);
            XrpcErrorResponse::internal_error("Database query failed")
        })?;

    let profile_map: HashMap<&str, &ProfileRow> =
        profiles.iter().map(|p| (p.did.as_str(), p)).collect();

    // Find owner and build participants
    let mut owner: Option<ProfileViewBasic<'static>> = None;
    let mut participants: Vec<ProfileViewBasic<'static>> = Vec::new();

    for perm in &permissions {
        let profile_view = if let Some(profile) = profile_map.get(perm.grantee_did.as_str()) {
            profile_to_view_basic(profile)?
        } else {
            // No profile found - skip (shouldn't happen if permissions table is consistent)
            continue;
        };

        if perm.scope == "owner" {
            owner = Some(profile_view);
        } else {
            participants.push(profile_view);
        }
    }

    let owner = owner.ok_or_else(|| {
        tracing::error!("Resource has no owner in permissions");
        XrpcErrorResponse::internal_error("Resource has no owner")
    })?;

    // Check if viewer can edit
    let viewer_can_edit = viewer_did.map(|v| all_dids.contains(&v));

    Ok(Json(
        GetResourceParticipantsOutput {
            owner,
            participants,
            viewer_can_edit,
            extra_data: None,
        }
        .into_static(),
    ))
}

/// Convert ProfileRow to ProfileViewBasic directly
pub fn profile_to_view_basic(
    profile: &ProfileRow,
) -> Result<ProfileViewBasic<'static>, XrpcErrorResponse> {
    let did = Did::new_owned(profile.did.clone())
        .map_err(|_| XrpcErrorResponse::internal_error("Invalid DID in profile"))?;

    let handle = Handle::new_owned(profile.handle.clone())
        .map_err(|_| XrpcErrorResponse::internal_error("Invalid handle in profile"))?;

    Ok(ProfileViewBasic::new()
        .did(did)
        .handle(handle)
        .maybe_display_name(non_empty_str(&profile.display_name))
        .build())
}

/// Handle sh.weaver.collab.getCollaborationState
///
/// Returns full collaboration state for a resource.
pub async fn get_collaboration_state(
    State(state): State<AppState>,
    ExtractOptionalServiceAuth(viewer): ExtractOptionalServiceAuth,
    ExtractXrpc(args): ExtractXrpc<GetCollaborationStateRequest>,
) -> Result<Json<GetCollaborationStateOutput<'static>>, XrpcErrorResponse> {
    let _viewer: Viewer = viewer;

    let resource_uri = args.resource.as_str();

    // Get permissions for the resource
    let permissions = state
        .clickhouse
        .get_resource_permissions(resource_uri)
        .await
        .map_err(|e| {
            tracing::error!("Failed to get resource permissions: {}", e);
            XrpcErrorResponse::internal_error("Database query failed")
        })?;

    if permissions.is_empty() {
        return Err(XrpcErrorResponse::not_found("Resource not found"));
    }

    // Get collaborators (invite+accept pairs) for additional data
    let collaborators = state
        .clickhouse
        .get_collaborators(resource_uri)
        .await
        .map_err(|e| {
            tracing::error!("Failed to get collaborators: {}", e);
            XrpcErrorResponse::internal_error("Database query failed")
        })?;

    // Check for divergence
    let has_divergence = state
        .clickhouse
        .has_divergence(resource_uri)
        .await
        .map_err(|e| {
            tracing::error!("Failed to check divergence: {}", e);
            XrpcErrorResponse::internal_error("Database query failed")
        })?;

    // Collect all DIDs for profile hydration
    let all_dids: Vec<&str> = permissions.iter().map(|p| p.grantee_did.as_str()).collect();

    // Batch fetch profiles
    let profiles = state
        .clickhouse
        .get_profiles_batch(&all_dids)
        .await
        .map_err(|e| {
            tracing::error!("Failed to batch fetch profiles: {}", e);
            XrpcErrorResponse::internal_error("Database query failed")
        })?;

    let profile_map: HashMap<&str, &ProfileRow> =
        profiles.iter().map(|p| (p.did.as_str(), p)).collect();

    // Build collaborator lookup for invite/accept URIs
    let collab_map: HashMap<&str, &CollaboratorRow> = collaborators
        .iter()
        .map(|c| (c.collaborator_did.as_str(), c))
        .collect();

    // Find owner and get resource CID
    let owner_perm = permissions
        .iter()
        .find(|p| p.scope == "owner")
        .ok_or_else(|| {
            tracing::error!("Resource has no owner in permissions");
            XrpcErrorResponse::internal_error("Resource has no owner")
        })?;

    // Build resource StrongRef - look up the CID from the appropriate table
    let resource_uri_parsed = AtUri::new(resource_uri)
        .map_err(|_| XrpcErrorResponse::internal_error("Invalid resource URI"))?
        .into_static();

    // Look up the resource CID from raw_records
    let resource_cid = state
        .clickhouse
        .get_record_cid(
            &owner_perm.resource_did,
            &owner_perm.resource_collection,
            &owner_perm.resource_rkey,
        )
        .await
        .map_err(|e| {
            tracing::error!("Failed to get resource CID: {}", e);
            XrpcErrorResponse::internal_error("Database query failed")
        })?
        .ok_or_else(|| XrpcErrorResponse::not_found("Resource not found in database"))?;

    let resource = StrongRef::new()
        .uri(resource_uri_parsed.clone())
        .cid(
            Cid::new(resource_cid.as_bytes())
                .map_err(|_| XrpcErrorResponse::internal_error("Invalid resource CID"))?
                .into_static(),
        )
        .build();

    // Build participants
    let mut participants: Vec<ParticipantStateView<'static>> = Vec::new();
    let mut first_collab_at: Option<chrono::DateTime<chrono::Utc>> = None;

    for perm in &permissions {
        let profile = profile_map
            .get(perm.grantee_did.as_str())
            .ok_or_else(|| XrpcErrorResponse::internal_error("Missing profile for participant"))?;
        let collab = collab_map.get(perm.grantee_did.as_str());

        // Track first collaborator time
        if perm.scope != "owner" {
            if let Some(c) = collab {
                match first_collab_at {
                    None => first_collab_at = Some(c.accepted_at),
                    Some(t) if c.accepted_at < t => first_collab_at = Some(c.accepted_at),
                    _ => {}
                }
            }
        }

        let participant = build_participant_state(profile, collab, &perm.scope)?;
        participants.push(participant);
    }

    // Determine status
    let status = if collaborators.is_empty() {
        "solo"
    } else if has_divergence {
        "diverged"
    } else {
        "synced"
    };

    let collab_state = CollaborationStateView::new()
        .resource(resource)
        .status(status)
        .participants(participants)
        .maybe_canonical_uri(Some(resource_uri_parsed))
        .maybe_has_divergence(Some(has_divergence))
        .maybe_first_collaborator_added_at(
            first_collab_at.map(|dt| Datetime::new(dt.fixed_offset())),
        )
        .build();

    Ok(Json(
        GetCollaborationStateOutput {
            value: collab_state,
            extra_data: None,
        }
        .into_static(),
    ))
}

/// Build ParticipantStateView from available data
fn build_participant_state(
    profile: &ProfileRow,
    collab: Option<&&CollaboratorRow>,
    scope: &str,
) -> Result<ParticipantStateView<'static>, XrpcErrorResponse> {
    let user = profile_to_view_basic(profile)?;

    let role = match scope {
        "owner" => "owner",
        "collaborator" => "collaborator",
        _ => "unknown",
    };

    let status = if collab.is_some() {
        "active"
    } else {
        "pending"
    };

    // Parse URIs if we have collab data
    let (invite_uri, accept_uri) = if let Some(c) = collab {
        let inv = AtUri::new(c.invite_uri.as_str())
            .map_err(|_| XrpcErrorResponse::internal_error("Invalid invite URI"))?
            .into_static();
        let acc = AtUri::new(c.accept_uri.as_str())
            .map_err(|_| XrpcErrorResponse::internal_error("Invalid accept URI"))?
            .into_static();
        (Some(inv), Some(acc))
    } else {
        (None, None)
    };

    Ok(ParticipantStateView::new()
        .role(role)
        .status(status)
        .user(user)
        .maybe_invite_uri(invite_uri)
        .maybe_accept_uri(accept_uri)
        .build())
}
