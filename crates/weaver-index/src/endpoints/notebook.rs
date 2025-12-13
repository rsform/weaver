//! sh.weaver.notebook.* endpoint handlers

use std::collections::{HashMap, HashSet};

use axum::{Json, extract::State};
use jacquard::IntoStatic;
use jacquard::cowstr::ToCowStr;
use jacquard::types::string::{AtUri, Cid, Did, Handle, Uri};
use jacquard::types::value::Data;
use jacquard_axum::ExtractXrpc;
use smol_str::SmolStr;
use weaver_api::sh_weaver::actor::{ProfileDataView, ProfileDataViewInner, ProfileView};
use weaver_api::sh_weaver::notebook::{
    AuthorListView, BookEntryView, EntryView, NotebookView,
    get_entry::{GetEntryOutput, GetEntryRequest},
    resolve_entry::{ResolveEntryOutput, ResolveEntryRequest},
    resolve_notebook::{ResolveNotebookOutput, ResolveNotebookRequest},
};

use crate::clickhouse::ProfileRow;
use crate::endpoints::actor::resolve_actor;
use crate::endpoints::repo::XrpcErrorResponse;
use crate::server::AppState;

/// Handle sh.weaver.notebook.resolveNotebook
///
/// Resolves a notebook by actor + path/title, returns notebook with entries.
pub async fn resolve_notebook(
    State(state): State<AppState>,
    ExtractXrpc(args): ExtractXrpc<ResolveNotebookRequest>,
) -> Result<Json<ResolveNotebookOutput<'static>>, XrpcErrorResponse> {
    // Resolve actor to DID
    let did = resolve_actor(&state, &args.actor).await?;
    let did_str = did.as_str();
    let name = args.name.as_ref();

    // Fetch notebook and entries in parallel - both just need the DID
    let limit = args.entry_limit.unwrap_or(50).clamp(1, 100) as u32;
    let cursor = args.entry_cursor.as_deref();

    let (notebook_result, entries_result) = tokio::try_join!(
        async {
            state
                .clickhouse
                .resolve_notebook(did_str, name)
                .await
                .map_err(|e| {
                    tracing::error!("Failed to resolve notebook: {}", e);
                    XrpcErrorResponse::internal_error("Database query failed")
                })
        },
        async {
            state
                .clickhouse
                .list_notebook_entries(did_str, limit + 1, cursor)
                .await
                .map_err(|e| {
                    tracing::error!("Failed to list entries: {}", e);
                    XrpcErrorResponse::internal_error("Database query failed")
                })
        }
    )?;

    let notebook_row =
        notebook_result.ok_or_else(|| XrpcErrorResponse::not_found("Notebook not found"))?;
    let entry_rows = entries_result;

    // Fetch notebook contributors (evidence-based)
    let notebook_contributors = state
        .clickhouse
        .get_notebook_contributors(did_str, &notebook_row.rkey)
        .await
        .map_err(|e| {
            tracing::error!("Failed to get notebook contributors: {}", e);
            XrpcErrorResponse::internal_error("Database query failed")
        })?;

    // Check if there are more entries
    let has_more = entry_rows.len() > limit as usize;
    let entry_rows: Vec<_> = entry_rows.into_iter().take(limit as usize).collect();

    // Collect all unique author DIDs for batch hydration
    // Start with evidence-based notebook contributors
    let mut all_author_dids: HashSet<&str> =
        notebook_contributors.iter().map(|s| s.as_str()).collect();
    // Also include author_dids from the record (explicit declarations)
    for did in &notebook_row.author_dids {
        all_author_dids.insert(did.as_str());
    }
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

    // Build lookup map
    let profile_map: HashMap<&str, &ProfileRow> =
        profiles.iter().map(|p| (p.did.as_str(), p)).collect();

    // Build NotebookView
    let notebook_uri = AtUri::new(&notebook_row.uri).map_err(|e| {
        tracing::error!("Invalid notebook URI in db: {}", e);
        XrpcErrorResponse::internal_error("Invalid URI stored")
    })?;

    let notebook_cid = Cid::new(notebook_row.cid.as_bytes()).map_err(|e| {
        tracing::error!("Invalid notebook CID in db: {}", e);
        XrpcErrorResponse::internal_error("Invalid CID stored")
    })?;

    // Hydrate notebook authors (evidence-based contributors)
    let authors = hydrate_authors(&notebook_contributors, &profile_map)?;

    // Parse record JSON
    let record = parse_record_json(&notebook_row.record)?;

    let notebook = NotebookView::new()
        .uri(notebook_uri.into_static())
        .cid(notebook_cid.into_static())
        .authors(authors)
        .record(record)
        .indexed_at(notebook_row.indexed_at.fixed_offset())
        .maybe_title(non_empty_cowstr(&notebook_row.title))
        .maybe_path(non_empty_cowstr(&notebook_row.path))
        .build();

    // Build entry views
    let mut entries: Vec<BookEntryView<'static>> = Vec::with_capacity(entry_rows.len());
    for (idx, entry_row) in entry_rows.iter().enumerate() {
        let entry_uri = AtUri::new(&entry_row.uri).map_err(|e| {
            tracing::error!("Invalid entry URI in db: {}", e);
            XrpcErrorResponse::internal_error("Invalid URI stored")
        })?;

        let entry_cid = Cid::new(entry_row.cid.as_bytes()).map_err(|e| {
            tracing::error!("Invalid entry CID in db: {}", e);
            XrpcErrorResponse::internal_error("Invalid CID stored")
        })?;

        // Hydrate entry authors
        let entry_authors = hydrate_authors(&entry_row.author_dids, &profile_map)?;

        // Parse record JSON
        let entry_record = parse_record_json(&entry_row.record)?;

        let entry_view = EntryView::new()
            .uri(entry_uri.into_static())
            .cid(entry_cid.into_static())
            .authors(entry_authors)
            .record(entry_record)
            .indexed_at(entry_row.indexed_at.fixed_offset())
            .maybe_title(non_empty_cowstr(&entry_row.title))
            .maybe_path(non_empty_cowstr(&entry_row.path))
            .build();

        let book_entry = BookEntryView::new()
            .entry(entry_view)
            .index(idx as i64)
            .build();

        entries.push(book_entry);
    }

    // Build cursor for pagination
    let next_cursor = if has_more {
        entry_rows.last().map(|e| e.rkey.to_string().into())
    } else {
        None
    };

    Ok(Json(
        ResolveNotebookOutput {
            notebook,
            entries,
            entry_cursor: next_cursor,
            extra_data: None,
        }
        .into_static(),
    ))
}

/// Handle sh.weaver.notebook.getEntry
///
/// Gets an entry by AT URI.
pub async fn get_entry(
    State(state): State<AppState>,
    ExtractXrpc(args): ExtractXrpc<GetEntryRequest>,
) -> Result<Json<GetEntryOutput<'static>>, XrpcErrorResponse> {
    // Parse the AT URI to extract authority and rkey
    let uri = &args.uri;
    let authority = uri.authority();
    let rkey = uri
        .rkey()
        .ok_or_else(|| XrpcErrorResponse::invalid_request("URI must include rkey"))?;
    let rkey_str = rkey.as_ref();

    // Resolve authority to DID (could be handle or DID)
    let did = resolve_actor(&state, authority).await?;
    let did_str = did.as_str();

    // Fetch entry and contributors in parallel
    let (entry_result, contributors_result) = tokio::try_join!(
        async {
            state
                .clickhouse
                .get_entry_exact(did_str, rkey_str)
                .await
                .map_err(|e| {
                    tracing::error!("Failed to get entry: {}", e);
                    XrpcErrorResponse::internal_error("Database query failed")
                })
        },
        async {
            state
                .clickhouse
                .get_entry_contributors(did_str, rkey_str)
                .await
                .map_err(|e| {
                    tracing::error!("Failed to get contributors: {}", e);
                    XrpcErrorResponse::internal_error("Database query failed")
                })
        }
    )?;

    let entry_row = entry_result.ok_or_else(|| XrpcErrorResponse::not_found("Entry not found"))?;

    // Merge contributors with author_dids from record (dedupe)
    let mut all_author_dids: HashSet<&str> =
        contributors_result.iter().map(|s| s.as_str()).collect();
    for did in &entry_row.author_dids {
        all_author_dids.insert(did.as_str());
    }

    // Fetch profiles for all authors
    let author_dids_vec: Vec<&str> = all_author_dids.into_iter().collect();
    let profiles = state
        .clickhouse
        .get_profiles_batch(&author_dids_vec)
        .await
        .map_err(|e| {
            tracing::error!("Failed to fetch profiles: {}", e);
            XrpcErrorResponse::internal_error("Database query failed")
        })?;

    let profile_map: HashMap<&str, &ProfileRow> =
        profiles.iter().map(|p| (p.did.as_str(), p)).collect();

    // Build EntryView - use contributors as the author list (evidence-based)
    let entry_view = build_entry_view_with_authors(&entry_row, &contributors_result, &profile_map)?;

    Ok(Json(
        GetEntryOutput {
            value: entry_view,
            extra_data: None,
        }
        .into_static(),
    ))
}

/// Handle sh.weaver.notebook.resolveEntry
///
/// Resolves an entry by actor + notebook name + entry name.
pub async fn resolve_entry(
    State(state): State<AppState>,
    ExtractXrpc(args): ExtractXrpc<ResolveEntryRequest>,
) -> Result<Json<ResolveEntryOutput<'static>>, XrpcErrorResponse> {
    // Resolve actor to DID
    let did = resolve_actor(&state, &args.actor).await?;
    let did_str = did.as_str();

    // Resolve notebook and entry in parallel - both just need the DID
    let notebook_name = args.notebook.as_ref();
    let entry_name = args.entry.as_ref();

    let (notebook_result, entry_result) = tokio::try_join!(
        async {
            state
                .clickhouse
                .resolve_notebook(did_str, notebook_name)
                .await
                .map_err(|e| {
                    tracing::error!("Failed to resolve notebook: {}", e);
                    XrpcErrorResponse::internal_error("Database query failed")
                })
        },
        async {
            state
                .clickhouse
                .resolve_entry(did_str, entry_name)
                .await
                .map_err(|e| {
                    tracing::error!("Failed to resolve entry: {}", e);
                    XrpcErrorResponse::internal_error("Database query failed")
                })
        }
    )?;

    let _notebook_row =
        notebook_result.ok_or_else(|| XrpcErrorResponse::not_found("Notebook not found"))?;
    let entry_row = entry_result.ok_or_else(|| XrpcErrorResponse::not_found("Entry not found"))?;

    // Fetch contributors and notebooks in parallel (need entry rkey, so must wait for entry resolution)
    let (contributors, notebooks) = tokio::try_join!(
        async {
            state
                .clickhouse
                .get_entry_contributors(did_str, &entry_row.rkey)
                .await
                .map_err(|e| {
                    tracing::error!("Failed to get contributors: {}", e);
                    XrpcErrorResponse::internal_error("Database query failed")
                })
        },
        async {
            state
                .clickhouse
                .get_notebooks_for_entry(did_str, &entry_row.rkey)
                .await
                .map_err(|e| {
                    tracing::error!("Failed to get notebooks for entry: {}", e);
                    XrpcErrorResponse::internal_error("Database query failed")
                })
        }
    )?;

    // Merge contributors with author_dids from record (dedupe)
    let mut all_author_dids: HashSet<&str> = contributors.iter().map(|s| s.as_str()).collect();
    for did in &entry_row.author_dids {
        all_author_dids.insert(did.as_str());
    }

    // Fetch profiles for all authors
    let author_dids_vec: Vec<&str> = all_author_dids.into_iter().collect();
    let profiles = state
        .clickhouse
        .get_profiles_batch(&author_dids_vec)
        .await
        .map_err(|e| {
            tracing::error!("Failed to fetch profiles: {}", e);
            XrpcErrorResponse::internal_error("Database query failed")
        })?;

    let profile_map: HashMap<&str, &ProfileRow> =
        profiles.iter().map(|p| (p.did.as_str(), p)).collect();

    // Build EntryView - use contributors as the author list (evidence-based)
    let entry_view = build_entry_view_with_authors(&entry_row, &contributors, &profile_map)?;

    // Parse the record for the output
    let record = parse_record_json(&entry_row.record)?;

    // Actual count of notebooks containing this entry
    let notebook_count = notebooks.len() as i64;

    Ok(Json(
        ResolveEntryOutput {
            entry: entry_view,
            notebook_count,
            notebooks: None,
            record,
            extra_data: None,
        }
        .into_static(),
    ))
}

/// Build an EntryView from an EntryRow with explicit author list (evidence-based contributors)
fn build_entry_view_with_authors(
    entry_row: &crate::clickhouse::EntryRow,
    author_dids: &[SmolStr],
    profile_map: &HashMap<&str, &ProfileRow>,
) -> Result<EntryView<'static>, XrpcErrorResponse> {
    let entry_uri = AtUri::new(&entry_row.uri).map_err(|e| {
        tracing::error!("Invalid entry URI in db: {}", e);
        XrpcErrorResponse::internal_error("Invalid URI stored")
    })?;

    let entry_cid = Cid::new(entry_row.cid.as_bytes()).map_err(|e| {
        tracing::error!("Invalid entry CID in db: {}", e);
        XrpcErrorResponse::internal_error("Invalid CID stored")
    })?;

    let authors = hydrate_authors(author_dids, profile_map)?;
    let record = parse_record_json(&entry_row.record)?;

    let entry_view = EntryView::new()
        .uri(entry_uri.into_static())
        .cid(entry_cid.into_static())
        .authors(authors)
        .record(record)
        .indexed_at(entry_row.indexed_at.fixed_offset())
        .maybe_title(non_empty_cowstr(&entry_row.title))
        .maybe_path(non_empty_cowstr(&entry_row.path))
        .build();

    Ok(entry_view)
}

/// Convert SmolStr to Option<CowStr> if non-empty
fn non_empty_cowstr(s: &smol_str::SmolStr) -> Option<jacquard::CowStr<'static>> {
    if s.is_empty() {
        None
    } else {
        Some(s.to_cowstr().into_static())
    }
}

/// Parse record JSON string into owned Data
fn parse_record_json(json: &str) -> Result<Data<'static>, XrpcErrorResponse> {
    let data: Data<'_> = serde_json::from_str(json).map_err(|e| {
        tracing::error!("Failed to parse record JSON: {}", e);
        XrpcErrorResponse::internal_error("Invalid record JSON stored")
    })?;
    Ok(data.into_static())
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
    let did = Did::new(&profile.did).map_err(|e| {
        tracing::error!("Invalid DID in profile: {}", e);
        XrpcErrorResponse::internal_error("Invalid DID stored")
    })?;

    let handle = if profile.handle.is_empty() {
        // Use DID as fallback handle (not ideal but functional)
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
        .maybe_display_name(non_empty_cowstr(&profile.display_name))
        .maybe_description(non_empty_cowstr(&profile.description))
        .maybe_avatar(avatar)
        .maybe_banner(banner)
        .build();

    let profile_data = ProfileDataView::new()
        .inner(ProfileDataViewInner::ProfileView(Box::new(inner_profile)))
        .build();

    Ok(profile_data)
}
