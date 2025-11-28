//! PDS synchronization for editor edit state.
//!
//! This module handles syncing the editor's Loro CRDT document to AT Protocol
//! edit records (`sh.weaver.edit.root` and `sh.weaver.edit.diff`).
//!
//! ## Edit State Structure
//!
//! - `sh.weaver.edit.root`: The starting point for an edit session, containing
//!   a full Loro snapshot and a reference to the entry being edited.
//! - `sh.weaver.edit.diff`: Incremental updates since the root (or previous diff),
//!   containing only the Loro delta bytes.
//!
//! ## Sync Flow
//!
//! 1. **First sync**: Create a root record with a full snapshot
//! 2. **Subsequent syncs**: Create diff records with deltas since last sync
//! 3. **Loading**: Find root via constellation backlinks, fetch all diffs, apply in order

use std::collections::BTreeMap;

use jacquard::cowstr::ToCowStr;
use jacquard::prelude::*;
use jacquard::types::blob::MimeType;
use jacquard::types::collection::Collection;
use jacquard::types::ident::AtIdentifier;
use jacquard::types::recordkey::RecordKey;
use jacquard::types::string::{AtUri, Cid, Did, Nsid};
use jacquard::types::tid::Ticker;
use jacquard::types::uri::Uri;
use jacquard::url::Url;
use jacquard::{CowStr, IntoStatic, to_data};
use weaver_api::com_atproto::repo::create_record::CreateRecord;
use weaver_api::com_atproto::repo::strong_ref::StrongRef;
use weaver_api::com_atproto::sync::get_blob::GetBlob;
use weaver_api::sh_weaver::edit::diff::Diff;
use weaver_api::sh_weaver::edit::root::Root;
use weaver_api::sh_weaver::edit::{DocRef, DocRefValue, DraftRef, EntryRef};
use weaver_common::constellation::{GetBacklinksQuery, RecordId};
use weaver_common::{WeaverError, WeaverExt};

use crate::fetch::Fetcher;

use super::document::EditorDocument;

const ROOT_NSID: &str = "sh.weaver.edit.root";
const DIFF_NSID: &str = "sh.weaver.edit.diff";
const CONSTELLATION_URL: &str = "https://constellation.microcosm.blue";

/// Build a DocRef for either a published entry or an unpublished draft.
///
/// If entry_uri and entry_cid are provided, creates an EntryRef.
/// Otherwise, creates a DraftRef with the given draft key.
fn build_doc_ref(
    draft_key: &str,
    entry_uri: Option<&AtUri<'_>>,
    entry_cid: Option<&Cid<'_>>,
) -> DocRef<'static> {
    match (entry_uri, entry_cid) {
        (Some(uri), Some(cid)) => DocRef {
            value: DocRefValue::EntryRef(Box::new(EntryRef {
                entry: StrongRef::new()
                    .uri(uri.clone().into_static())
                    .cid(cid.clone().into_static())
                    .build(),
                extra_data: None,
            })),
            extra_data: None,
        },
        _ => DocRef {
            value: DocRefValue::DraftRef(Box::new(DraftRef {
                draft_key: CowStr::from(draft_key.to_string()),
                extra_data: None,
            })),
            extra_data: None,
        },
    }
}

/// Result of a sync operation.
#[derive(Clone, Debug)]
pub enum SyncResult {
    /// Created a new root record (first sync)
    CreatedRoot {
        uri: AtUri<'static>,
        cid: Cid<'static>,
    },
    /// Created a new diff record
    CreatedDiff {
        uri: AtUri<'static>,
        cid: Cid<'static>,
    },
    /// No changes to sync
    NoChanges,
}

/// Find the edit root for an entry using constellation backlinks.
///
/// Queries constellation for `sh.weaver.edit.root` records that reference
/// the given entry URI via the `.doc.value.entry.uri` path.
#[allow(dead_code)]
pub async fn find_edit_root_for_entry(
    fetcher: &Fetcher,
    entry_uri: &AtUri<'_>,
) -> Result<Option<RecordId<'static>>, WeaverError> {
    let constellation_url = Url::parse(CONSTELLATION_URL)
        .map_err(|e| WeaverError::InvalidNotebook(format!("Invalid constellation URL: {}", e)))?;

    let query = GetBacklinksQuery {
        subject: Uri::At(entry_uri.clone().into_static()),
        source: format!("{}:.doc.value.entry.uri", ROOT_NSID).into(),
        cursor: None,
        did: vec![],
        limit: 1,
    };

    let response = fetcher
        .client
        .xrpc(constellation_url)
        .send(&query)
        .await
        .map_err(|e| WeaverError::InvalidNotebook(format!("Constellation query failed: {}", e)))?;

    let output = response.into_output().map_err(|e| {
        WeaverError::InvalidNotebook(format!("Failed to parse constellation response: {}", e))
    })?;

    Ok(output.records.into_iter().next().map(|r| r.into_static()))
}

/// Find all diffs for a root record using constellation backlinks.
#[allow(dead_code)]
pub async fn find_diffs_for_root(
    fetcher: &Fetcher,
    root_uri: &AtUri<'_>,
) -> Result<Vec<RecordId<'static>>, WeaverError> {
    let constellation_url = Url::parse(CONSTELLATION_URL)
        .map_err(|e| WeaverError::InvalidNotebook(format!("Invalid constellation URL: {}", e)))?;

    let mut all_diffs = Vec::new();
    let mut cursor: Option<String> = None;

    loop {
        let query = GetBacklinksQuery {
            subject: Uri::At(root_uri.clone().into_static()),
            source: format!("{}:.root.uri", DIFF_NSID).into(),
            cursor: cursor.map(Into::into),
            did: vec![],
            limit: 100,
        };

        let response = fetcher
            .client
            .xrpc(constellation_url.clone())
            .send(&query)
            .await
            .map_err(|e| {
                WeaverError::InvalidNotebook(format!("Constellation query failed: {}", e))
            })?;

        let output = response.into_output().map_err(|e| {
            WeaverError::InvalidNotebook(format!("Failed to parse constellation response: {}", e))
        })?;

        all_diffs.extend(output.records.into_iter().map(|r| r.into_static()));

        match output.cursor {
            Some(c) => cursor = Some(c.to_string()),
            None => break,
        }
    }

    Ok(all_diffs)
}

/// Create the edit root record for an entry.
///
/// Uploads the current Loro snapshot as a blob and creates an `sh.weaver.edit.root`
/// record referencing the entry (or draft key if unpublished).
///
/// # Arguments
/// * `fetcher` - The authenticated fetcher
/// * `doc` - The editor document
/// * `draft_key` - The draft key (used for unpublished entries)
/// * `entry_uri` - Optional AT-URI of the published entry
/// * `entry_cid` - Optional CID of the published entry
pub async fn create_edit_root(
    fetcher: &Fetcher,
    doc: &EditorDocument,
    draft_key: &str,
    entry_uri: Option<&AtUri<'_>>,
    entry_cid: Option<&Cid<'_>>,
) -> Result<(AtUri<'static>, Cid<'static>), WeaverError> {
    let client = fetcher.get_client();
    let did = fetcher
        .current_did()
        .await
        .ok_or_else(|| WeaverError::InvalidNotebook("Not authenticated".into()))?;

    // Export full snapshot
    let snapshot = doc.export_snapshot();

    // Upload snapshot blob
    let mime_type = MimeType::new_static("application/octet-stream");
    let blob_ref = client
        .upload_blob(snapshot, mime_type)
        .await
        .map_err(|e| WeaverError::InvalidNotebook(format!("Failed to upload snapshot: {}", e)))?;

    // Build DocRef - use EntryRef if published, DraftRef if not
    let doc_ref = build_doc_ref(draft_key, entry_uri, entry_cid);

    // Build root record
    let root = Root::new().doc(doc_ref).snapshot(blob_ref).build();

    let root_data = to_data(&root)
        .map_err(|e| WeaverError::InvalidNotebook(format!("Failed to serialize root: {}", e)))?;

    // Generate TID for the root rkey
    let root_tid = Ticker::new().next(None);
    let rkey = RecordKey::any(root_tid.as_str())
        .map_err(|e| WeaverError::InvalidNotebook(e.to_string()))?;

    let collection = Nsid::new(ROOT_NSID).map_err(|e| WeaverError::AtprotoString(e))?;

    let request = CreateRecord::new()
        .repo(AtIdentifier::Did(did))
        .collection(collection)
        .rkey(rkey)
        .record(root_data)
        .build();

    let response = fetcher
        .send(request)
        .await
        .map_err(jacquard::client::AgentError::from)?;

    let output = response
        .into_output()
        .map_err(|e| WeaverError::InvalidNotebook(e.to_string()))?;

    Ok((output.uri.into_static(), output.cid.into_static()))
}

/// Create a diff record with updates since the last sync.
///
/// # Arguments
/// * `fetcher` - The authenticated fetcher
/// * `doc` - The editor document
/// * `root_uri` - URI of the edit root
/// * `root_cid` - CID of the edit root
/// * `prev_diff` - Optional reference to the previous diff
/// * `draft_key` - The draft key (used for doc reference)
/// * `entry_uri` - Optional AT-URI of the published entry
/// * `entry_cid` - Optional CID of the published entry
pub async fn create_diff(
    fetcher: &Fetcher,
    doc: &EditorDocument,
    root_uri: &AtUri<'_>,
    root_cid: &Cid<'_>,
    prev_diff: Option<(&AtUri<'_>, &Cid<'_>)>,
    draft_key: &str,
    entry_uri: Option<&AtUri<'_>>,
    entry_cid: Option<&Cid<'_>>,
) -> Result<Option<(AtUri<'static>, Cid<'static>)>, WeaverError> {
    // Export updates since last sync
    let updates = match doc.export_updates_since_sync() {
        Some(u) => u,
        None => return Ok(None), // No changes
    };

    let client = fetcher.get_client();
    let did = fetcher
        .current_did()
        .await
        .ok_or_else(|| WeaverError::InvalidNotebook("Not authenticated".into()))?;

    // Upload updates blob
    let mime_type = MimeType::new_static("application/octet-stream");
    let blob_ref = client
        .upload_blob(updates, mime_type)
        .await
        .map_err(|e| WeaverError::InvalidNotebook(format!("Failed to upload diff: {}", e)))?;

    // Build DocRef - use EntryRef if published, DraftRef if not
    let doc_ref = build_doc_ref(draft_key, entry_uri, entry_cid);

    // Build root reference
    let root_ref = StrongRef::new()
        .uri(root_uri.clone().into_static())
        .cid(root_cid.clone().into_static())
        .build();

    // Build prev reference if we have a previous diff
    let prev_ref = prev_diff.map(|(uri, cid)| {
        StrongRef::new()
            .uri(uri.clone().into_static())
            .cid(cid.clone().into_static())
            .build()
    });

    // Build diff record
    let diff = Diff::new()
        .doc(doc_ref)
        .root(root_ref)
        .snapshot(blob_ref)
        .maybe_prev(prev_ref)
        .build();

    let diff_data = to_data(&diff)
        .map_err(|e| WeaverError::InvalidNotebook(format!("Failed to serialize diff: {}", e)))?;

    // Generate TID for the diff rkey
    let diff_tid = Ticker::new().next(None);
    let rkey = RecordKey::any(diff_tid.as_str())
        .map_err(|e| WeaverError::InvalidNotebook(e.to_string()))?;

    let collection = Nsid::new(DIFF_NSID).map_err(|e| WeaverError::AtprotoString(e))?;

    let request = CreateRecord::new()
        .repo(AtIdentifier::Did(did))
        .collection(collection)
        .rkey(rkey)
        .record(diff_data)
        .build();

    let response = fetcher
        .send(request)
        .await
        .map_err(jacquard::client::AgentError::from)?;

    let output = response
        .into_output()
        .map_err(|e| WeaverError::InvalidNotebook(e.to_string()))?;

    Ok(Some((output.uri.into_static(), output.cid.into_static())))
}

/// Sync the document to the PDS.
///
/// If no edit root exists, creates one with a full snapshot.
/// If a root exists, creates a diff with updates since last sync.
///
/// Updates the document's sync state on success.
///
/// # Arguments
/// * `fetcher` - The authenticated fetcher
/// * `doc` - The editor document (mutable to update sync state)
/// * `draft_key` - The draft key for this document
///
/// # Returns
/// The sync result indicating what was created.
pub async fn sync_to_pds(
    fetcher: &Fetcher,
    doc: &mut EditorDocument,
    draft_key: &str,
) -> Result<SyncResult, WeaverError> {
    // Check if we have changes to sync
    if !doc.has_unsync_changes() {
        return Ok(SyncResult::NoChanges);
    }

    // Get entry info if published
    let entry_ref = doc.entry_ref();

    if doc.edit_root().is_none() {
        // First sync - create root
        let (root_uri, root_cid) = create_edit_root(
            fetcher,
            doc,
            draft_key,
            entry_ref.as_ref().map(|r| &r.uri),
            entry_ref.as_ref().map(|r| &r.cid),
        )
        .await?;

        // Build StrongRef for the root
        let root_ref = StrongRef::new()
            .uri(root_uri.clone())
            .cid(root_cid.clone())
            .build();

        // Update document state
        doc.set_edit_root(Some(root_ref));
        doc.set_last_diff(None);
        doc.mark_synced();

        Ok(SyncResult::CreatedRoot {
            uri: root_uri,
            cid: root_cid,
        })
    } else {
        // Subsequent sync - create diff
        let root_ref = doc.edit_root().unwrap();
        let prev_diff = doc.last_diff();

        let result = create_diff(
            fetcher,
            doc,
            &root_ref.uri,
            &root_ref.cid,
            prev_diff.as_ref().map(|d| (&d.uri, &d.cid)),
            draft_key,
            entry_ref.as_ref().map(|r| &r.uri),
            entry_ref.as_ref().map(|r| &r.cid),
        )
        .await?;

        match result {
            Some((diff_uri, diff_cid)) => {
                // Build StrongRef for the diff
                let diff_ref = StrongRef::new()
                    .uri(diff_uri.clone())
                    .cid(diff_cid.clone())
                    .build();

                doc.set_last_diff(Some(diff_ref));
                doc.mark_synced();

                Ok(SyncResult::CreatedDiff {
                    uri: diff_uri,
                    cid: diff_cid,
                })
            }
            None => Ok(SyncResult::NoChanges),
        }
    }
}

/// Result of loading edit state from PDS.
#[derive(Clone, Debug)]
pub struct PdsEditState {
    /// The root record reference
    pub root_ref: StrongRef<'static>,
    /// The latest diff reference (if any diffs exist)
    pub last_diff_ref: Option<StrongRef<'static>>,
    /// The Loro snapshot bytes from the root
    pub root_snapshot: Vec<u8>,
    /// All diff update bytes in order (oldest first, by TID)
    pub diff_updates: Vec<Vec<u8>>,
}

/// Fetch a blob from the PDS.
async fn fetch_blob(
    fetcher: &Fetcher,
    did: &Did<'_>,
    cid: &Cid<'_>,
) -> Result<Vec<u8>, WeaverError> {
    let pds_url = fetcher
        .client
        .pds_for_did(did)
        .await
        .map_err(|e| WeaverError::InvalidNotebook(format!("Failed to resolve DID: {}", e)))?;

    let request = GetBlob::new().did(did.clone()).cid(cid.clone()).build();

    let response = fetcher
        .client
        .xrpc(pds_url)
        .send(&request)
        .await
        .map_err(|e| WeaverError::InvalidNotebook(format!("Failed to fetch blob: {}", e)))?;

    let output = response.into_output().map_err(|e| {
        WeaverError::InvalidNotebook(format!("Failed to parse blob response: {}", e))
    })?;

    Ok(output.body.to_vec())
}

/// Load edit state from the PDS for an entry.
///
/// Finds the edit root via constellation backlinks, fetches all diffs,
/// and returns the snapshot + updates needed to reconstruct the document.
///
/// # Arguments
/// * `fetcher` - The authenticated fetcher
/// * `entry_uri` - The AT-URI of the entry to load edit state for
///
/// # Returns
/// The edit state if found, or None if no edit root exists for this entry.
pub async fn load_edit_state_from_pds(
    fetcher: &Fetcher,
    entry_uri: &AtUri<'_>,
) -> Result<Option<PdsEditState>, WeaverError> {
    // Find the edit root for this entry
    let root_id = match find_edit_root_for_entry(fetcher, entry_uri).await? {
        Some(id) => id,
        None => return Ok(None),
    };

    // Build root URI
    let root_uri = AtUri::new(&format!(
        "at://{}/{}/{}",
        root_id.did(),
        ROOT_NSID,
        root_id.rkey().as_ref()
    ))
    .map_err(|e| WeaverError::InvalidNotebook(format!("Invalid root URI: {}", e)))?
    .into_static();

    // Fetch the root record using get_record helper
    let root_response = fetcher
        .client
        .get_record::<Root>(&root_uri)
        .await
        .map_err(|e| WeaverError::InvalidNotebook(format!("Failed to fetch root: {}", e)))?;

    let root_output = root_response
        .into_output()
        .map_err(|e| WeaverError::InvalidNotebook(format!("Failed to parse root: {}", e)))?;

    let root_cid = root_output
        .cid
        .ok_or_else(|| WeaverError::InvalidNotebook("Root response missing CID".into()))?;

    let root_ref = StrongRef::new()
        .uri(root_uri.clone())
        .cid(root_cid.into_static())
        .build();

    // Fetch the root snapshot blob
    let root_snapshot = fetch_blob(
        fetcher,
        &root_id.did(),
        root_output.value.snapshot.blob().cid(),
    )
    .await?;

    // Find all diffs for this root
    let diff_ids = find_diffs_for_root(fetcher, &root_uri).await?;

    if diff_ids.is_empty() {
        return Ok(Some(PdsEditState {
            root_ref,
            last_diff_ref: None,
            root_snapshot,
            diff_updates: vec![],
        }));
    }

    // Fetch all diffs and store in BTreeMap keyed by rkey (TID) for sorted order
    // TIDs are lexicographically sortable timestamps
    let mut diffs_by_rkey: BTreeMap<
        CowStr<'static>,
        (Diff<'static>, Cid<'static>, AtUri<'static>),
    > = BTreeMap::new();

    for diff_id in &diff_ids {
        let rkey = diff_id.rkey();
        let rkey_str: &str = rkey.as_ref();
        let diff_uri = AtUri::new(&format!(
            "at://{}/{}/{}",
            diff_id.did(),
            DIFF_NSID,
            rkey_str
        ))
        .map_err(|e| WeaverError::InvalidNotebook(format!("Invalid diff URI: {}", e)))?
        .into_static();

        let diff_response = fetcher
            .client
            .get_record::<Diff>(&diff_uri)
            .await
            .map_err(|e| WeaverError::InvalidNotebook(format!("Failed to fetch diff: {}", e)))?;

        let diff_output = diff_response
            .into_output()
            .map_err(|e| WeaverError::InvalidNotebook(format!("Failed to parse diff: {}", e)))?;

        let diff_cid = diff_output
            .cid
            .ok_or_else(|| WeaverError::InvalidNotebook("Diff response missing CID".into()))?;

        diffs_by_rkey.insert(
            rkey_str.to_cowstr().into_static(),
            (
                diff_output.value.into_static(),
                diff_cid.into_static(),
                diff_uri,
            ),
        );
    }

    // Fetch all diff blobs in TID order (BTreeMap iterates in sorted order)
    let mut diff_updates = Vec::new();
    let mut last_diff_ref = None;

    for (_rkey, (diff, cid, uri)) in &diffs_by_rkey {
        let blob_bytes = fetch_blob(fetcher, &root_id.did(), diff.snapshot.blob().cid()).await?;
        diff_updates.push(blob_bytes);

        // Track the last diff (will be the one with highest TID after iteration)
        last_diff_ref = Some(StrongRef::new().uri(uri.clone()).cid(cid.clone()).build());
    }

    Ok(Some(PdsEditState {
        root_ref,
        last_diff_ref,
        root_snapshot,
        diff_updates,
    }))
}

/// Load an EditorDocument by merging local storage and PDS state.
///
/// This is the main entry point for loading a document with full sync support.
/// It:
/// 1. Loads from localStorage (if available)
/// 2. Loads from PDS (if available)
/// 3. Merges both using Loro's CRDT merge
///
/// The result is a document with all changes from both sources.
///
/// # Arguments
/// * `fetcher` - The authenticated fetcher
/// * `draft_key` - The localStorage key for this draft
/// * `entry_uri` - Optional AT-URI if editing an existing entry
///
/// # Returns
/// A merged EditorDocument, or None if no state exists anywhere.
pub async fn load_and_merge_document(
    fetcher: &Fetcher,
    draft_key: &str,
    entry_uri: Option<&AtUri<'_>>,
) -> Result<Option<EditorDocument>, WeaverError> {
    use super::storage::load_from_storage;

    // Load from localStorage
    let local_doc = load_from_storage(draft_key);

    // Load from PDS (only if we have an entry URI)
    let pds_state = if let Some(uri) = entry_uri {
        load_edit_state_from_pds(fetcher, uri).await?
    } else {
        None
    };

    match (local_doc, pds_state) {
        (None, None) => Ok(None),

        (Some(doc), None) => {
            // Only local state exists
            tracing::debug!("Loaded document from localStorage only");
            Ok(Some(doc))
        }

        (None, Some(pds)) => {
            // Only PDS state exists - reconstruct from snapshot + diffs
            tracing::debug!("Loaded document from PDS only");
            let mut doc = EditorDocument::from_snapshot(&pds.root_snapshot, None, 0);

            // Apply all diffs in order
            for updates in &pds.diff_updates {
                if let Err(e) = doc.import_updates(updates) {
                    tracing::warn!("Failed to apply diff update: {:?}", e);
                }
            }

            // Set sync state so we don't re-upload what we just downloaded
            doc.set_synced_from_pds(pds.root_ref, pds.last_diff_ref);

            Ok(Some(doc))
        }

        (Some(mut local_doc), Some(pds)) => {
            // Both exist - merge using CRDT
            tracing::debug!("Merging document from localStorage and PDS");

            // Import PDS root snapshot into local doc
            // Loro will automatically merge concurrent changes
            if let Err(e) = local_doc.import_updates(&pds.root_snapshot) {
                tracing::warn!("Failed to merge PDS root snapshot: {:?}", e);
            }

            // Import all diffs
            for updates in &pds.diff_updates {
                if let Err(e) = local_doc.import_updates(updates) {
                    tracing::warn!("Failed to merge PDS diff: {:?}", e);
                }
            }

            // Update sync state
            // We keep the PDS root/diff refs since that's where we'll push updates
            local_doc.set_edit_root(Some(pds.root_ref));
            local_doc.set_last_diff(pds.last_diff_ref);
            // Don't call set_synced_from_pds - local changes still need syncing

            Ok(Some(local_doc))
        }
    }
}
