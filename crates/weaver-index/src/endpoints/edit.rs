//! Edit endpoint handlers

use std::collections::HashMap;

use axum::{Json, extract::State};
use jacquard::IntoStatic;
use jacquard::cowstr::ToCowStr;
use jacquard::types::datetime::Datetime;
use jacquard::types::string::{AtUri, Cid};
use jacquard_axum::ExtractXrpc;
use jacquard_axum::service_auth::ExtractOptionalServiceAuth;

use weaver_api::com_atproto::repo::strong_ref::StrongRef;
use weaver_api::sh_weaver::edit::EditHistoryEntry;
use weaver_api::sh_weaver::edit::get_contributors::{
    GetContributorsOutput, GetContributorsRequest,
};
use weaver_api::sh_weaver::edit::get_edit_history::{GetEditHistoryOutput, GetEditHistoryRequest};
use weaver_api::sh_weaver::edit::list_drafts::{DraftView, ListDraftsOutput, ListDraftsRequest};

use crate::clickhouse::{EditNodeRow, ProfileRow};
use crate::endpoints::actor::{Viewer, resolve_actor};
use crate::endpoints::collab::profile_to_view_basic;
use crate::endpoints::resolve_uri;
use crate::endpoints::repo::XrpcErrorResponse;
use crate::server::AppState;

/// Handle sh.weaver.edit.getEditHistory
///
/// Returns edit history (roots and diffs) for a resource.
pub async fn get_edit_history(
    State(state): State<AppState>,
    ExtractOptionalServiceAuth(viewer): ExtractOptionalServiceAuth,
    ExtractXrpc(args): ExtractXrpc<GetEditHistoryRequest>,
) -> Result<Json<GetEditHistoryOutput<'static>>, XrpcErrorResponse> {
    let _viewer: Viewer = viewer;

    let limit = args.limit.unwrap_or(50).min(100).max(1);

    // Resolve URI and get canonical form
    let resolved = resolve_uri(&state, &args.resource).await?;

    // Parse cursor as millisecond timestamp
    let cursor = args
        .cursor
        .as_deref()
        .map(|c| c.parse::<i64>())
        .transpose()
        .map_err(|_| XrpcErrorResponse::invalid_request("Invalid cursor format"))?;

    let after_rkey = args.after_rkey.as_deref();

    // Fetch edit nodes
    let nodes = state
        .clickhouse
        .get_edit_history(
            &resolved.did,
            &resolved.collection,
            &resolved.rkey,
            cursor,
            after_rkey,
            limit + 1,
        )
        .await
        .map_err(|e| {
            tracing::error!("Failed to get edit history: {}", e);
            XrpcErrorResponse::internal_error("Database query failed")
        })?;

    // Check if there are more results
    let has_more = nodes.len() > limit as usize;
    let nodes: Vec<_> = nodes.into_iter().take(limit as usize).collect();

    // Collect unique author DIDs
    let author_dids: Vec<&str> = nodes.iter().map(|n| n.did.as_str()).collect();
    let unique_dids: Vec<&str> = author_dids
        .iter()
        .copied()
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();

    // Batch fetch profiles
    let profiles = state
        .clickhouse
        .get_profiles_batch(&unique_dids)
        .await
        .map_err(|e| {
            tracing::error!("Failed to batch fetch profiles: {}", e);
            XrpcErrorResponse::internal_error("Database query failed")
        })?;

    let profile_map: HashMap<&str, &ProfileRow> =
        profiles.iter().map(|p| (p.did.as_str(), p)).collect();

    // Separate roots and diffs, building EditHistoryEntry for each
    let mut roots = Vec::new();
    let mut diffs = Vec::new();

    for node in &nodes {
        let entry = node_to_history_entry(node, &profile_map)?;

        if node.node_type == "root" {
            roots.push(entry);
        } else {
            diffs.push(entry);
        }
    }

    // Build cursor from last node's created_at
    let next_cursor = if has_more {
        nodes
            .last()
            .map(|n| n.created_at.timestamp_millis().to_cowstr().into_static())
    } else {
        None
    };

    Ok(Json(
        GetEditHistoryOutput {
            roots,
            diffs,
            cursor: next_cursor,
            extra_data: None,
        }
        .into_static(),
    ))
}

/// Convert EditNodeRow to EditHistoryEntry
fn node_to_history_entry(
    node: &EditNodeRow,
    profile_map: &HashMap<&str, &ProfileRow>,
) -> Result<EditHistoryEntry<'static>, XrpcErrorResponse> {
    let author = profile_map
        .get(node.did.as_str())
        .map(|p| profile_to_view_basic(p))
        .transpose()?
        .ok_or_else(|| XrpcErrorResponse::internal_error("Author profile not found"))?;

    // Build URI
    let uri = AtUri::new(&format!(
        "at://{}/{}/{}",
        node.did, node.collection, node.rkey
    ))
    .map_err(|_| XrpcErrorResponse::internal_error("Invalid AT URI"))?
    .into_static();

    let cid = Cid::new(node.cid.as_bytes())
        .map_err(|_| XrpcErrorResponse::internal_error("Invalid CID"))?
        .into_static();

    // Build optional StrongRefs for diffs
    let root_ref = if !node.root_cid.is_empty() {
        let root_uri = AtUri::new(&format!(
            "at://{}/sh.weaver.edit.root/{}",
            node.root_did, node.root_rkey
        ))
        .map_err(|_| XrpcErrorResponse::internal_error("Invalid root URI"))?
        .into_static();

        let root_cid = Cid::new(node.root_cid.as_bytes())
            .map_err(|_| XrpcErrorResponse::internal_error("Invalid root CID"))?
            .into_static();

        Some(StrongRef::new().uri(root_uri).cid(root_cid).build())
    } else {
        None
    };

    let prev_ref = if !node.prev_cid.is_empty() {
        let prev_uri = AtUri::new(&format!(
            "at://{}/sh.weaver.edit.diff/{}",
            node.prev_did, node.prev_rkey
        ))
        .map_err(|_| XrpcErrorResponse::internal_error("Invalid prev URI"))?
        .into_static();

        let prev_cid = Cid::new(node.prev_cid.as_bytes())
            .map_err(|_| XrpcErrorResponse::internal_error("Invalid prev CID"))?
            .into_static();

        Some(StrongRef::new().uri(prev_uri).cid(prev_cid).build())
    } else {
        None
    };

    let created_at = Datetime::new(node.created_at.fixed_offset());

    Ok(EditHistoryEntry::new()
        .uri(uri)
        .cid(cid)
        .author(author)
        .created_at(created_at)
        .r#type(node.node_type.clone())
        .maybe_has_inline_diff(Some(node.has_inline_diff == 1))
        .maybe_prev_ref(prev_ref)
        .maybe_root_ref(root_ref)
        .build())
}

/// Handle sh.weaver.edit.getContributors
///
/// Returns evidence-based contributors for a resource (entry or notebook).
pub async fn get_contributors(
    State(state): State<AppState>,
    ExtractOptionalServiceAuth(viewer): ExtractOptionalServiceAuth,
    ExtractXrpc(args): ExtractXrpc<GetContributorsRequest>,
) -> Result<Json<GetContributorsOutput<'static>>, XrpcErrorResponse> {
    let _viewer: Viewer = viewer;

    let include_cascaded = args.include_cascaded.unwrap_or(true);

    // Resolve URI and get canonical form
    let resolved = resolve_uri(&state, &args.resource).await?;

    // Get contributors based on resource type
    let contributor_dids = match resolved.collection.as_str() {
        "sh.weaver.notebook.entry" => {
            if include_cascaded {
                state
                    .clickhouse
                    .get_entry_contributors(&resolved.did, &resolved.rkey)
                    .await
            } else {
                state
                    .clickhouse
                    .get_entry_contributors_direct(&resolved.did, &resolved.rkey)
                    .await
            }
        }
        "sh.weaver.notebook.book" => {
            state
                .clickhouse
                .get_notebook_contributors(&resolved.did, &resolved.rkey)
                .await
        }
        _ => {
            return Err(XrpcErrorResponse::invalid_request(
                "Resource must be an entry or notebook",
            ));
        }
    }
    .map_err(|e| {
        tracing::error!("Failed to get contributors: {}", e);
        XrpcErrorResponse::internal_error("Database query failed")
    })?;

    if contributor_dids.is_empty() {
        return Ok(Json(
            GetContributorsOutput {
                contributors: Vec::new(),
                extra_data: None,
            }
            .into_static(),
        ));
    }

    // Batch fetch profiles
    let did_refs: Vec<&str> = contributor_dids.iter().map(|s| s.as_str()).collect();
    let profiles = state
        .clickhouse
        .get_profiles_batch(&did_refs)
        .await
        .map_err(|e| {
            tracing::error!("Failed to batch fetch profiles: {}", e);
            XrpcErrorResponse::internal_error("Database query failed")
        })?;

    let profile_map: HashMap<&str, &ProfileRow> =
        profiles.iter().map(|p| (p.did.as_str(), p)).collect();

    // Build contributor list
    let mut contributors = Vec::with_capacity(contributor_dids.len());
    for did in &contributor_dids {
        if let Some(profile) = profile_map.get(did.as_str()) {
            contributors.push(profile_to_view_basic(profile)?);
        }
    }

    Ok(Json(
        GetContributorsOutput {
            contributors,
            extra_data: None,
        }
        .into_static(),
    ))
}

/// Handle sh.weaver.edit.listDrafts
///
/// Returns draft records for an actor.
pub async fn list_drafts(
    State(state): State<AppState>,
    ExtractOptionalServiceAuth(viewer): ExtractOptionalServiceAuth,
    ExtractXrpc(args): ExtractXrpc<ListDraftsRequest>,
) -> Result<Json<ListDraftsOutput<'static>>, XrpcErrorResponse> {
    let _viewer: Viewer = viewer;

    let limit = args.limit.unwrap_or(50).min(100).max(1);

    // Parse cursor as millisecond timestamp
    let cursor = args
        .cursor
        .as_deref()
        .map(|c| c.parse::<i64>())
        .transpose()
        .map_err(|_| XrpcErrorResponse::invalid_request("Invalid cursor format"))?;

    // Resolve actor to DID
    let actor_did = resolve_actor(&state, &args.actor).await?;

    // Fetch drafts
    let draft_rows = state
        .clickhouse
        .list_drafts(&actor_did, cursor, limit + 1)
        .await
        .map_err(|e| {
            tracing::error!("Failed to list drafts: {}", e);
            XrpcErrorResponse::internal_error("Database query failed")
        })?;

    // Check if there are more results
    let has_more = draft_rows.len() > limit as usize;
    let draft_rows: Vec<_> = draft_rows.into_iter().take(limit as usize).collect();

    // Build draft views
    let mut drafts = Vec::with_capacity(draft_rows.len());
    for row in &draft_rows {
        let uri = AtUri::new(&format!(
            "at://{}/sh.weaver.edit.draft/{}",
            row.did, row.rkey
        ))
        .map_err(|_| XrpcErrorResponse::internal_error("Invalid AT URI"))?
        .into_static();

        let cid = Cid::new(row.cid.as_bytes())
            .map_err(|_| XrpcErrorResponse::internal_error("Invalid CID"))?
            .into_static();

        let created_at = Datetime::new(row.created_at.fixed_offset());

        // Build optional edit root reference
        let edit_root = if !row.root_cid.is_empty() {
            let root_uri = AtUri::new(&format!(
                "at://{}/sh.weaver.edit.root/{}",
                row.root_did, row.root_rkey
            ))
            .map_err(|_| XrpcErrorResponse::internal_error("Invalid root URI"))?
            .into_static();

            let root_cid = Cid::new(row.root_cid.as_bytes())
                .map_err(|_| XrpcErrorResponse::internal_error("Invalid root CID"))?
                .into_static();

            Some(StrongRef::new().uri(root_uri).cid(root_cid).build())
        } else {
            None
        };

        let last_edit_at = row.last_edit_at.map(|dt| Datetime::new(dt.fixed_offset()));

        drafts.push(
            DraftView::new()
                .uri(uri)
                .cid(cid)
                .created_at(created_at)
                .maybe_edit_root(edit_root)
                .maybe_last_edit_at(last_edit_at)
                .build(),
        );
    }

    // Build cursor from last draft's created_at
    let next_cursor = if has_more {
        draft_rows
            .last()
            .map(|d| d.created_at.timestamp_millis().to_cowstr().into_static())
    } else {
        None
    };

    Ok(Json(
        ListDraftsOutput {
            drafts,
            cursor: next_cursor,
            extra_data: None,
        }
        .into_static(),
    ))
}
