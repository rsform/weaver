//! sh.weaver.actor.* endpoint handlers

use std::collections::{HashMap, HashSet};

use axum::{Json, extract::State};
use jacquard::IntoStatic;
use jacquard::cowstr::ToCowStr;
use jacquard::identity::resolver::IdentityResolver;
use jacquard::types::ident::AtIdentifier;
use jacquard::types::string::{AtUri, Cid, Did, Handle};
use jacquard_axum::ExtractXrpc;
use jacquard_axum::service_auth::{ExtractOptionalServiceAuth, VerifiedServiceAuth};
use smol_str::SmolStr;
use weaver_api::sh_weaver::actor::{
    ProfileDataView, ProfileDataViewInner, ProfileView,
    get_actor_entries::{GetActorEntriesOutput, GetActorEntriesRequest},
    get_actor_notebooks::{GetActorNotebooksOutput, GetActorNotebooksRequest},
    get_profile::{GetProfileOutput, GetProfileRequest},
};
use weaver_api::sh_weaver::notebook::{AuthorListView, EntryView, NotebookView};

use crate::clickhouse::ProfileRow;
use crate::endpoints::repo::XrpcErrorResponse;
use crate::server::AppState;

/// Authenticated viewer context (if present)
pub type Viewer = Option<VerifiedServiceAuth<'static>>;

/// Handle sh.weaver.actor.getProfile
///
/// Returns a profile view with counts for the requested actor.
pub async fn get_profile(
    State(state): State<AppState>,
    ExtractOptionalServiceAuth(viewer): ExtractOptionalServiceAuth,
    ExtractXrpc(args): ExtractXrpc<GetProfileRequest>,
) -> Result<Json<GetProfileOutput<'static>>, XrpcErrorResponse> {
    // viewer contains Some(auth) if the request has valid service auth
    // can be used later for viewer-specific state (e.g., "you follow this person")
    let _viewer = viewer;
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

/// Parse cursor string to i64 timestamp millis
fn parse_cursor(cursor: Option<&str>) -> Result<Option<i64>, XrpcErrorResponse> {
    cursor
        .map(|c| {
            c.parse::<i64>()
                .map_err(|_| XrpcErrorResponse::invalid_request("Invalid cursor format"))
        })
        .transpose()
}

/// Handle sh.weaver.actor.getActorNotebooks
///
/// Returns notebooks owned by the given actor.
pub async fn get_actor_notebooks(
    State(state): State<AppState>,
    ExtractOptionalServiceAuth(viewer): ExtractOptionalServiceAuth,
    ExtractXrpc(args): ExtractXrpc<GetActorNotebooksRequest>,
) -> Result<Json<GetActorNotebooksOutput<'static>>, XrpcErrorResponse> {
    let _viewer: Viewer = viewer;

    // Resolve actor to DID
    let did = resolve_actor(&state, &args.actor).await?;
    let did_str = did.as_str();

    // Fetch notebooks for this actor
    let limit = args.limit.unwrap_or(50).clamp(1, 100) as u32;
    let cursor = parse_cursor(args.cursor.as_deref())?;

    let notebook_rows = state
        .clickhouse
        .list_actor_notebooks(did_str, limit + 1, cursor)
        .await
        .map_err(|e| {
            tracing::error!("Failed to list actor notebooks: {}", e);
            XrpcErrorResponse::internal_error("Database query failed")
        })?;

    // Check if there are more
    let has_more = notebook_rows.len() > limit as usize;
    let notebook_rows: Vec<_> = notebook_rows.into_iter().take(limit as usize).collect();

    // Collect author DIDs for hydration
    let mut all_author_dids: HashSet<&str> = HashSet::new();
    for nb in &notebook_rows {
        for did in &nb.author_dids {
            all_author_dids.insert(did.as_str());
        }
    }

    // Batch fetch profiles
    let author_dids_vec: Vec<&str> = all_author_dids.into_iter().collect();
    let profiles = state
        .clickhouse
        .get_profiles_batch(&author_dids_vec)
        .await
        .map_err(|e| {
            tracing::error!("Failed to batch fetch profiles: {}", e);
            XrpcErrorResponse::internal_error("Database query failed")
        })?;

    let profile_map: HashMap<&str, &ProfileRow> =
        profiles.iter().map(|p| (p.did.as_str(), p)).collect();

    // Build NotebookViews
    let mut notebooks: Vec<NotebookView<'static>> = Vec::with_capacity(notebook_rows.len());
    for nb_row in &notebook_rows {
        let notebook_uri = AtUri::new(&nb_row.uri).map_err(|e| {
            tracing::error!("Invalid notebook URI in db: {}", e);
            XrpcErrorResponse::internal_error("Invalid URI stored")
        })?;

        let notebook_cid = Cid::new(nb_row.cid.as_bytes()).map_err(|e| {
            tracing::error!("Invalid notebook CID in db: {}", e);
            XrpcErrorResponse::internal_error("Invalid CID stored")
        })?;

        let authors = hydrate_authors(&nb_row.author_dids, &profile_map)?;
        let record = parse_record_json(&nb_row.record)?;

        let notebook = NotebookView::new()
            .uri(notebook_uri.into_static())
            .cid(notebook_cid.into_static())
            .authors(authors)
            .record(record)
            .indexed_at(nb_row.indexed_at.fixed_offset())
            .maybe_title(non_empty_str(&nb_row.title))
            .maybe_path(non_empty_str(&nb_row.path))
            .build();

        notebooks.push(notebook);
    }

    // Build cursor for pagination (created_at millis)
    let next_cursor = if has_more {
        notebook_rows
            .last()
            .map(|nb| nb.created_at.timestamp_millis().to_cowstr().into_static())
    } else {
        None
    };

    Ok(Json(
        GetActorNotebooksOutput {
            notebooks,
            cursor: next_cursor,
            extra_data: None
        }
        .into_static(),
    ))
}

/// Handle sh.weaver.actor.getActorEntries
///
/// Returns entries owned by the given actor.
pub async fn get_actor_entries(
    State(state): State<AppState>,
    ExtractOptionalServiceAuth(viewer): ExtractOptionalServiceAuth,
    ExtractXrpc(args): ExtractXrpc<GetActorEntriesRequest>,
) -> Result<Json<GetActorEntriesOutput<'static>>, XrpcErrorResponse> {
    let _viewer: Viewer = viewer;

    // Resolve actor to DID
    let did = resolve_actor(&state, &args.actor).await?;
    let did_str = did.as_str();

    // Fetch entries for this actor
    let limit = args.limit.unwrap_or(50).clamp(1, 100) as u32;
    let cursor = parse_cursor(args.cursor.as_deref())?;

    let entry_rows = state
        .clickhouse
        .list_actor_entries(did_str, limit + 1, cursor)
        .await
        .map_err(|e| {
            tracing::error!("Failed to list actor entries: {}", e);
            XrpcErrorResponse::internal_error("Database query failed")
        })?;

    // Check if there are more
    let has_more = entry_rows.len() > limit as usize;
    let entry_rows: Vec<_> = entry_rows.into_iter().take(limit as usize).collect();

    // Collect author DIDs for hydration
    let mut all_author_dids: HashSet<&str> = HashSet::new();
    for entry in &entry_rows {
        for did in &entry.author_dids {
            all_author_dids.insert(did.as_str());
        }
    }

    // Batch fetch profiles
    let author_dids_vec: Vec<&str> = all_author_dids.into_iter().collect();
    let profiles = state
        .clickhouse
        .get_profiles_batch(&author_dids_vec)
        .await
        .map_err(|e| {
            tracing::error!("Failed to batch fetch profiles: {}", e);
            XrpcErrorResponse::internal_error("Database query failed")
        })?;

    let profile_map: HashMap<&str, &ProfileRow> =
        profiles.iter().map(|p| (p.did.as_str(), p)).collect();

    // Build EntryViews
    let mut entries: Vec<EntryView<'static>> = Vec::with_capacity(entry_rows.len());
    for entry_row in &entry_rows {
        let entry_uri = AtUri::new(&entry_row.uri).map_err(|e| {
            tracing::error!("Invalid entry URI in db: {}", e);
            XrpcErrorResponse::internal_error("Invalid URI stored")
        })?;

        let entry_cid = Cid::new(entry_row.cid.as_bytes()).map_err(|e| {
            tracing::error!("Invalid entry CID in db: {}", e);
            XrpcErrorResponse::internal_error("Invalid CID stored")
        })?;

        let authors = hydrate_authors(&entry_row.author_dids, &profile_map)?;
        let record = parse_record_json(&entry_row.record)?;

        let entry = EntryView::new()
            .uri(entry_uri.into_static())
            .cid(entry_cid.into_static())
            .authors(authors)
            .record(record)
            .indexed_at(entry_row.indexed_at.fixed_offset())
            .maybe_title(non_empty_str(&entry_row.title))
            .maybe_path(non_empty_str(&entry_row.path))
            .build();

        entries.push(entry);
    }

    // Build cursor for pagination (created_at millis)
    let next_cursor = if has_more {
        entry_rows
            .last()
            .map(|e| e.created_at.timestamp_millis().to_cowstr().into_static())
    } else {
        None
    };

    Ok(Json(
        GetActorEntriesOutput {
            entries,
            cursor: next_cursor,
            extra_data: None,
        }
        .into_static(),
    ))
}

/// Hydrate author list from DIDs using profile map
fn hydrate_authors(
    author_dids: &[SmolStr],
    profile_map: &HashMap<&str, &ProfileRow>,
) -> Result<Vec<AuthorListView<'static>>, XrpcErrorResponse> {
    let mut authors = Vec::with_capacity(author_dids.len());

    for (idx, did_str) in author_dids.iter().enumerate() {
        let profile_data = if let Some(profile) = profile_map.get(did_str.as_str()) {
            profile_to_data_view(profile)?
        } else {
            // No profile found - create minimal view with just the DID
            let did = Did::new(did_str).map_err(|e| {
                tracing::error!("Invalid DID in author_dids: {}", e);
                XrpcErrorResponse::internal_error("Invalid DID stored")
            })?;

            let inner_profile = ProfileView::new()
                .did(did.into_static())
                .handle(
                    Handle::new(did_str)
                        .unwrap_or_else(|_| Handle::new("unknown.invalid").unwrap()),
                )
                .build();

            ProfileDataView::new()
                .inner(ProfileDataViewInner::ProfileView(Box::new(inner_profile)))
                .build()
        };

        let author_view = AuthorListView::new()
            .index(idx as i64)
            .record(profile_data.into_static())
            .build();

        authors.push(author_view);
    }

    Ok(authors)
}

/// Convert ProfileRow to ProfileDataView
fn profile_to_data_view(
    profile: &ProfileRow,
) -> Result<ProfileDataView<'static>, XrpcErrorResponse> {
    use jacquard::types::string::Uri;

    let did = Did::new(&profile.did).map_err(|e| {
        tracing::error!("Invalid DID in profile: {}", e);
        XrpcErrorResponse::internal_error("Invalid DID stored")
    })?;

    let handle = if profile.handle.is_empty() {
        Handle::new(&profile.did).unwrap_or_else(|_| Handle::new("unknown.invalid").unwrap())
    } else {
        Handle::new(&profile.handle).map_err(|e| {
            tracing::error!("Invalid handle in profile: {}", e);
            XrpcErrorResponse::internal_error("Invalid handle stored")
        })?
    };

    // Build avatar URL from CID if present
    let avatar = if !profile.avatar_cid.is_empty() {
        let url = format!(
            "https://cdn.bsky.app/img/avatar/plain/{}/{}@jpeg",
            profile.did, profile.avatar_cid
        );
        Uri::new_owned(url).ok()
    } else {
        None
    };

    // Build banner URL from CID if present
    let banner = if !profile.banner_cid.is_empty() {
        let url = format!(
            "https://cdn.bsky.app/img/banner/plain/{}/{}@jpeg",
            profile.did, profile.banner_cid
        );
        Uri::new_owned(url).ok()
    } else {
        None
    };

    let inner_profile = ProfileView::new()
        .did(did.into_static())
        .handle(handle.into_static())
        .maybe_display_name(non_empty_str(&profile.display_name))
        .maybe_description(non_empty_str(&profile.description))
        .maybe_avatar(avatar)
        .maybe_banner(banner)
        .build();

    let profile_data = ProfileDataView::new()
        .inner(ProfileDataViewInner::ProfileView(Box::new(inner_profile)))
        .build();

    Ok(profile_data)
}

/// Parse record JSON string into owned Data
fn parse_record_json(
    json: &str,
) -> Result<jacquard::types::value::Data<'static>, XrpcErrorResponse> {
    use jacquard::types::value::Data;

    let data: Data<'_> = serde_json::from_str(json).map_err(|e| {
        tracing::error!("Failed to parse record JSON: {}", e);
        XrpcErrorResponse::internal_error("Invalid record JSON stored")
    })?;
    Ok(data.into_static())
}
