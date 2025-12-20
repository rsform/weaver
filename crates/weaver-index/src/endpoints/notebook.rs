//! sh.weaver.notebook.* endpoint handlers

use std::collections::{HashMap, HashSet};

use axum::{Json, extract::State};
use jacquard::IntoStatic;
use jacquard::cowstr::ToCowStr;
use jacquard::types::string::{AtUri, Cid, Did, Handle, Uri};
use jacquard::types::value::Data;
use jacquard_axum::ExtractXrpc;
use jacquard_axum::service_auth::ExtractOptionalServiceAuth;
use smol_str::SmolStr;
use weaver_api::com_atproto::repo::strong_ref::StrongRef;
use weaver_api::sh_weaver::actor::{ProfileDataView, ProfileDataViewInner, ProfileView};
use weaver_api::sh_weaver::notebook::{
    AuthorListView, BookEntryRef, BookEntryView, EntryView, FeedEntryView, NotebookView,
    get_book_entry::{GetBookEntryOutput, GetBookEntryRequest},
    get_entry::{GetEntryOutput, GetEntryRequest},
    get_entry_feed::{GetEntryFeedOutput, GetEntryFeedRequest},
    get_entry_notebooks::{GetEntryNotebooksOutput, GetEntryNotebooksRequest, NotebookRef},
    get_notebook::{GetNotebookOutput, GetNotebookRequest},
    get_notebook_feed::{GetNotebookFeedOutput, GetNotebookFeedRequest},
    resolve_entry::{ResolveEntryOutput, ResolveEntryRequest},
    resolve_notebook::{ResolveNotebookOutput, ResolveNotebookRequest},
};

use crate::clickhouse::{EntryRow, ProfileRow};
use crate::endpoints::actor::{Viewer, resolve_actor};
use crate::endpoints::repo::XrpcErrorResponse;
use crate::server::AppState;

/// Handle sh.weaver.notebook.resolveNotebook
///
/// Resolves a notebook by actor + path/title, returns notebook with entries.
pub async fn resolve_notebook(
    State(state): State<AppState>,
    ExtractOptionalServiceAuth(viewer): ExtractOptionalServiceAuth,
    ExtractXrpc(args): ExtractXrpc<ResolveNotebookRequest>,
) -> Result<Json<ResolveNotebookOutput<'static>>, XrpcErrorResponse> {
    // viewer can be used later for viewer state (bookmarks, read status, etc.)
    let _viewer: Viewer = viewer;

    // Resolve actor to DID
    let did = resolve_actor(&state, &args.actor).await?;
    let did_str = did.as_str();
    let name = args.name.as_ref();

    let limit = args.entry_limit.unwrap_or(50).clamp(1, 100) as u32;
    let cursor: Option<u32> = args.entry_cursor.as_deref().and_then(|c| c.parse().ok());

    // Fetch notebook first to get its rkey
    let notebook_row = state
        .clickhouse
        .resolve_notebook(did_str, name)
        .await
        .map_err(|e| {
            tracing::error!("Failed to resolve notebook: {}", e);
            XrpcErrorResponse::internal_error("Database query failed")
        })?
        .ok_or_else(|| XrpcErrorResponse::not_found("Notebook not found"))?;

    // Now fetch entries using notebook's rkey
    let entry_rows = state
        .clickhouse
        .list_notebook_entries(did_str, &notebook_row.rkey, limit + 1, cursor)
        .await
        .map_err(|e| {
            tracing::error!("Failed to list entries: {}", e);
            XrpcErrorResponse::internal_error("Database query failed")
        })?;

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

    // Build entry views (first pass: create EntryViews)
    let mut entry_views: Vec<EntryView<'static>> = Vec::with_capacity(entry_rows.len());
    for entry_row in entry_rows.iter() {
        let entry_uri = AtUri::new(&entry_row.uri).map_err(|e| {
            tracing::error!("Invalid entry URI in db: {}", e);
            XrpcErrorResponse::internal_error("Invalid URI stored")
        })?;

        let entry_cid = Cid::new(entry_row.cid.as_bytes()).map_err(|e| {
            tracing::error!("Invalid entry CID in db: {}", e);
            XrpcErrorResponse::internal_error("Invalid CID stored")
        })?;

        let entry_contributors = state
            .clickhouse
            .get_entry_contributors(did_str, &entry_row.rkey)
            .await
            .map_err(|e| {
                tracing::error!("Failed to get entry contributors: {}", e);
                XrpcErrorResponse::internal_error("Database query failed")
            })?;

        let mut all_author_dids: HashSet<SmolStr> = entry_contributors.iter().cloned().collect();
        // Also include author_dids from the record (explicit declarations)
        for did in &entry_row.author_dids {
            all_author_dids.insert(did.clone());
        }

        let author_dids_vec: Vec<SmolStr> = all_author_dids.into_iter().collect();

        // Hydrate entry authors
        let entry_authors = hydrate_authors(&author_dids_vec, &profile_map)?;

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

        entry_views.push(entry_view);
    }

    // Build BookEntryViews with prev/next navigation
    let mut entries: Vec<BookEntryView<'static>> = Vec::with_capacity(entry_views.len());
    for (idx, entry_view) in entry_views.iter().enumerate() {
        let prev = (idx > 0)
            .then(|| BookEntryRef::new().entry(entry_views[idx - 1].clone()).build());
        let next = entry_views
            .get(idx + 1)
            .map(|e| BookEntryRef::new().entry(e.clone()).build());

        entries.push(
            BookEntryView::new()
                .entry(entry_view.clone())
                .index(idx as i64)
                .maybe_prev(prev)
                .maybe_next(next)
                .build(),
        );
    }

    // Build cursor for pagination (position-based)
    let next_cursor = if has_more {
        // Position = cursor offset + number of entries returned
        let last_position = cursor.unwrap_or(0) + entry_rows.len() as u32;
        Some(last_position.to_string().into())
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

/// Handle sh.weaver.notebook.getNotebook
///
/// Gets a notebook by AT URI, returns notebook view with entry refs.
pub async fn get_notebook(
    State(state): State<AppState>,
    ExtractOptionalServiceAuth(viewer): ExtractOptionalServiceAuth,
    ExtractXrpc(args): ExtractXrpc<GetNotebookRequest>,
) -> Result<Json<GetNotebookOutput<'static>>, XrpcErrorResponse> {
    let _viewer: Viewer = viewer;

    // Parse the AT URI to extract authority and rkey
    let uri = &args.notebook;
    let authority = uri.authority();
    let rkey = uri
        .rkey()
        .ok_or_else(|| XrpcErrorResponse::invalid_request("URI must include rkey"))?;
    let rkey_str = rkey.as_ref();

    // Resolve authority to DID (could be handle or DID)
    let did = resolve_actor(&state, authority).await?;
    let did_str = did.as_str();

    // Fetch notebook by DID + rkey
    let notebook_row = state
        .clickhouse
        .get_notebook(did_str, rkey_str)
        .await
        .map_err(|e| {
            tracing::error!("Failed to get notebook: {}", e);
            XrpcErrorResponse::internal_error("Database query failed")
        })?
        .ok_or_else(|| XrpcErrorResponse::not_found("Notebook not found"))?;

    // Fetch notebook contributors
    let notebook_contributors = state
        .clickhouse
        .get_notebook_contributors(did_str, rkey_str)
        .await
        .map_err(|e| {
            tracing::error!("Failed to get notebook contributors: {}", e);
            XrpcErrorResponse::internal_error("Database query failed")
        })?;

    // Collect all author DIDs for batch hydration
    let mut all_author_dids: HashSet<&str> =
        notebook_contributors.iter().map(|s| s.as_str()).collect();
    for did in &notebook_row.author_dids {
        all_author_dids.insert(did.as_str());
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

    // Build NotebookView
    let notebook_uri = AtUri::new(&notebook_row.uri).map_err(|e| {
        tracing::error!("Invalid notebook URI in db: {}", e);
        XrpcErrorResponse::internal_error("Invalid URI stored")
    })?;

    let notebook_cid = Cid::new(notebook_row.cid.as_bytes()).map_err(|e| {
        tracing::error!("Invalid notebook CID in db: {}", e);
        XrpcErrorResponse::internal_error("Invalid CID stored")
    })?;

    let authors = hydrate_authors(&notebook_contributors, &profile_map)?;
    let record = parse_record_json(&notebook_row.record)?;

    let notebook = NotebookView::new()
        .uri(notebook_uri.into_static())
        .cid(notebook_cid.into_static())
        .authors(authors)
        .record(record.clone())
        .indexed_at(notebook_row.indexed_at.fixed_offset())
        .maybe_title(non_empty_cowstr(&notebook_row.title))
        .maybe_path(non_empty_cowstr(&notebook_row.path))
        .build();

    // Deserialize Book from record to get entry_list
    let book: weaver_api::sh_weaver::notebook::book::Book =
        jacquard::from_data(&record).map_err(|e| {
            tracing::error!("Failed to deserialize Book record: {}", e);
            XrpcErrorResponse::internal_error("Invalid Book record")
        })?;

    let entries: Vec<StrongRef<'static>> = book
        .entry_list
        .into_iter()
        .map(|r| r.into_static())
        .collect();

    Ok(Json(
        GetNotebookOutput {
            notebook,
            entries,
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
    ExtractOptionalServiceAuth(viewer): ExtractOptionalServiceAuth,
    ExtractXrpc(args): ExtractXrpc<GetEntryRequest>,
) -> Result<Json<GetEntryOutput<'static>>, XrpcErrorResponse> {
    let _viewer: Viewer = viewer;

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
    ExtractOptionalServiceAuth(viewer): ExtractOptionalServiceAuth,
    ExtractXrpc(args): ExtractXrpc<ResolveEntryRequest>,
) -> Result<Json<ResolveEntryOutput<'static>>, XrpcErrorResponse> {
    let _viewer: Viewer = viewer;

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
        // TODO: fix this, as we do need the entries to know for sure which, in case of collisions
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

/// Parse cursor string to i64 timestamp millis
fn parse_cursor(cursor: Option<&str>) -> Result<Option<i64>, XrpcErrorResponse> {
    cursor
        .map(|c| {
            c.parse::<i64>()
                .map_err(|_| XrpcErrorResponse::invalid_request("Invalid cursor format"))
        })
        .transpose()
}

/// Handle sh.weaver.notebook.getNotebookFeed
///
/// Returns a global feed of notebooks.
pub async fn get_notebook_feed(
    State(state): State<AppState>,
    ExtractOptionalServiceAuth(viewer): ExtractOptionalServiceAuth,
    ExtractXrpc(args): ExtractXrpc<GetNotebookFeedRequest>,
) -> Result<Json<GetNotebookFeedOutput<'static>>, XrpcErrorResponse> {
    let _viewer: Viewer = viewer;

    let limit = args.limit.unwrap_or(50).clamp(1, 100) as u32;
    let cursor = parse_cursor(args.cursor.as_deref())?;
    let algorithm = args.algorithm.as_deref().unwrap_or("chronological");

    // Convert tags to &[&str] if present
    let tags_vec: Vec<&str> = args
        .tags
        .as_ref()
        .map(|t| t.iter().map(|s| s.as_ref()).collect())
        .unwrap_or_default();
    let tags = if tags_vec.is_empty() {
        None
    } else {
        Some(tags_vec.as_slice())
    };

    let notebook_rows = state
        .clickhouse
        .get_notebook_feed(algorithm, tags, limit + 1, cursor)
        .await
        .map_err(|e| {
            tracing::error!("Failed to get notebook feed: {}", e);
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
            .maybe_title(non_empty_cowstr(&nb_row.title))
            .maybe_path(non_empty_cowstr(&nb_row.path))
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
        GetNotebookFeedOutput {
            notebooks,
            cursor: next_cursor,
            extra_data: None,
        }
        .into_static(),
    ))
}

/// Handle sh.weaver.notebook.getEntryFeed
///
/// Returns a global feed of entries.
pub async fn get_entry_feed(
    State(state): State<AppState>,
    ExtractOptionalServiceAuth(viewer): ExtractOptionalServiceAuth,
    ExtractXrpc(args): ExtractXrpc<GetEntryFeedRequest>,
) -> Result<Json<GetEntryFeedOutput<'static>>, XrpcErrorResponse> {
    let _viewer: Viewer = viewer;

    let limit = args.limit.unwrap_or(50).clamp(1, 100) as u32;
    let cursor = parse_cursor(args.cursor.as_deref())?;
    let algorithm = args.algorithm.as_deref().unwrap_or("chronological");

    // Convert tags to &[&str] if present
    let tags_vec: Vec<&str> = args
        .tags
        .as_ref()
        .map(|t| t.iter().map(|s| s.as_ref()).collect())
        .unwrap_or_default();
    let tags = if tags_vec.is_empty() {
        None
    } else {
        Some(tags_vec.as_slice())
    };

    let entry_rows = state
        .clickhouse
        .get_entry_feed(algorithm, tags, limit + 1, cursor)
        .await
        .map_err(|e| {
            tracing::error!("Failed to get entry feed: {}", e);
            XrpcErrorResponse::internal_error("Database query failed")
        })?;

    // Check if there are more
    let has_more = entry_rows.len() > limit as usize;
    let entry_rows: Vec<_> = entry_rows.into_iter().take(limit as usize).collect();

    // Batch fetch contributors for all entries
    let entry_keys: Vec<(&str, &str)> = entry_rows
        .iter()
        .map(|e| (e.did.as_str(), e.rkey.as_str()))
        .collect();
    let contributors_map = state
        .clickhouse
        .get_entry_contributors_batch(&entry_keys)
        .await
        .map_err(|e| {
            tracing::error!("Failed to batch fetch contributors: {}", e);
            XrpcErrorResponse::internal_error("Database query failed")
        })?;

    // Collect all contributor DIDs for profile hydration
    let mut all_author_dids: HashSet<&str> = HashSet::new();
    for contributors in contributors_map.values() {
        for did in contributors {
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

    // Build FeedEntryViews
    let mut feed: Vec<FeedEntryView<'static>> = Vec::with_capacity(entry_rows.len());
    for entry_row in &entry_rows {
        // Get contributors for this entry
        let entry_key = (entry_row.did.clone(), entry_row.rkey.clone());
        let contributors = contributors_map
            .get(&entry_key)
            .map(|v| v.as_slice())
            .unwrap_or(&[]);

        let entry_view = build_entry_view_with_authors(entry_row, contributors, &profile_map)?;

        let feed_entry = FeedEntryView::new().entry(entry_view).build();

        feed.push(feed_entry);
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
        GetEntryFeedOutput {
            feed,
            cursor: next_cursor,
            extra_data: None,
        }
        .into_static(),
    ))
}

/// Handle sh.weaver.notebook.getBookEntry
///
/// Returns an entry at a specific index within a notebook, with prev/next navigation.
pub async fn get_book_entry(
    State(state): State<AppState>,
    ExtractOptionalServiceAuth(viewer): ExtractOptionalServiceAuth,
    ExtractXrpc(args): ExtractXrpc<GetBookEntryRequest>,
) -> Result<Json<GetBookEntryOutput<'static>>, XrpcErrorResponse> {
    let _viewer: Viewer = viewer;

    // Parse the notebook URI
    let notebook_uri = &args.notebook;
    let authority = notebook_uri.authority();
    let notebook_rkey = notebook_uri
        .rkey()
        .ok_or_else(|| XrpcErrorResponse::invalid_request("Notebook URI must include rkey"))?;

    // Resolve authority to DID
    let notebook_did = resolve_actor(&state, authority).await?;
    let notebook_did_str = notebook_did.as_str();
    let notebook_rkey_str = notebook_rkey.as_ref();

    let index = args.index.unwrap_or(0).max(0) as u32;

    // Fetch entry at index with prev/next
    let result = state
        .clickhouse
        .get_book_entry_at_index(notebook_did_str, notebook_rkey_str, index)
        .await
        .map_err(|e| {
            tracing::error!("Failed to get book entry: {}", e);
            XrpcErrorResponse::internal_error("Database query failed")
        })?;

    let (current_row, prev_row, next_row) =
        result.ok_or_else(|| XrpcErrorResponse::not_found("Entry not found at index"))?;

    // Collect all author DIDs for hydration
    let mut all_author_dids: HashSet<&str> = HashSet::new();
    for did in &current_row.author_dids {
        all_author_dids.insert(did.as_str());
    }
    if let Some(ref prev) = prev_row {
        for did in &prev.author_dids {
            all_author_dids.insert(did.as_str());
        }
    }
    if let Some(ref next) = next_row {
        for did in &next.author_dids {
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
            tracing::error!("Failed to fetch profiles: {}", e);
            XrpcErrorResponse::internal_error("Database query failed")
        })?;

    let profile_map: HashMap<&str, &ProfileRow> =
        profiles.iter().map(|p| (p.did.as_str(), p)).collect();

    // Build the current entry view
    let entry_view = build_entry_view(&current_row, &profile_map)?;

    // Build prev/next refs if present
    let prev_ref = if let Some(ref prev) = prev_row {
        let prev_view = build_entry_view(prev, &profile_map)?;
        Some(BookEntryRef::new().entry(prev_view).build())
    } else {
        None
    };

    let next_ref = if let Some(ref next) = next_row {
        let next_view = build_entry_view(next, &profile_map)?;
        Some(BookEntryRef::new().entry(next_view).build())
    } else {
        None
    };

    let book_entry = BookEntryView::new()
        .entry(entry_view)
        .index(index as i64)
        .maybe_prev(prev_ref)
        .maybe_next(next_ref)
        .build();

    Ok(Json(
        GetBookEntryOutput {
            value: book_entry,
            extra_data: None,
        }
        .into_static(),
    ))
}

/// Build an EntryView from an EntryRow
fn build_entry_view(
    entry_row: &EntryRow,
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

    let authors = hydrate_authors(&entry_row.author_dids, profile_map)?;
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

/// Handle sh.weaver.notebook.getEntryNotebooks
///
/// Returns notebooks that contain a given entry.
pub async fn get_entry_notebooks(
    State(state): State<AppState>,
    ExtractOptionalServiceAuth(viewer): ExtractOptionalServiceAuth,
    ExtractXrpc(args): ExtractXrpc<GetEntryNotebooksRequest>,
) -> Result<Json<GetEntryNotebooksOutput<'static>>, XrpcErrorResponse> {
    let _viewer: Viewer = viewer;

    // Parse the entry URI
    let entry_uri = &args.entry;
    let authority = entry_uri.authority();
    let entry_rkey = entry_uri
        .rkey()
        .ok_or_else(|| XrpcErrorResponse::invalid_request("Entry URI must include rkey"))?;

    // Resolve authority to DID
    let entry_did = resolve_actor(&state, authority).await?;
    let entry_did_str = entry_did.as_str();
    let entry_rkey_str = entry_rkey.as_ref();

    // Get notebooks containing this entry
    let notebook_refs = state
        .clickhouse
        .get_notebooks_for_entry(entry_did_str, entry_rkey_str)
        .await
        .map_err(|e| {
            tracing::error!("Failed to get notebooks for entry: {}", e);
            XrpcErrorResponse::internal_error("Database query failed")
        })?;

    if notebook_refs.is_empty() {
        return Ok(Json(
            GetEntryNotebooksOutput {
                notebooks: Vec::new(),
                extra_data: None,
            }
            .into_static(),
        ));
    }

    // Fetch notebook details and owner profiles
    let mut notebooks = Vec::with_capacity(notebook_refs.len());
    let mut owner_dids: HashSet<&str> = HashSet::new();

    // First pass: collect owner DIDs
    for (notebook_did, _notebook_rkey) in &notebook_refs {
        owner_dids.insert(notebook_did.as_str());
    }

    // Batch fetch profiles
    let owner_dids_vec: Vec<&str> = owner_dids.into_iter().collect();
    let profiles = state
        .clickhouse
        .get_profiles_batch(&owner_dids_vec)
        .await
        .map_err(|e| {
            tracing::error!("Failed to batch fetch profiles: {}", e);
            XrpcErrorResponse::internal_error("Database query failed")
        })?;

    let profile_map: HashMap<&str, &ProfileRow> =
        profiles.iter().map(|p| (p.did.as_str(), p)).collect();

    // Fetch each notebook's details
    for (notebook_did, notebook_rkey) in &notebook_refs {
        let notebook_row = state
            .clickhouse
            .get_notebook(notebook_did.as_str(), notebook_rkey.as_str())
            .await
            .map_err(|e| {
                tracing::error!("Failed to get notebook: {}", e);
                XrpcErrorResponse::internal_error("Database query failed")
            })?;

        if let Some(nb) = notebook_row {
            let uri = AtUri::new(&nb.uri)
                .map_err(|_| XrpcErrorResponse::internal_error("Invalid notebook URI"))?
                .into_static();

            let cid = Cid::new(nb.cid.as_bytes())
                .map_err(|_| XrpcErrorResponse::internal_error("Invalid notebook CID"))?
                .into_static();

            // Get owner profile
            let owner = profile_map
                .get(notebook_did.as_str())
                .map(|p| crate::endpoints::collab::profile_to_view_basic(p))
                .transpose()?;

            notebooks.push(
                NotebookRef::new()
                    .uri(uri)
                    .cid(cid)
                    .maybe_title(non_empty_cowstr(&nb.title))
                    .maybe_owner(owner)
                    .build(),
            );
        }
    }

    Ok(Json(
        GetEntryNotebooksOutput {
            notebooks,
            extra_data: None,
        }
        .into_static(),
    ))
}
