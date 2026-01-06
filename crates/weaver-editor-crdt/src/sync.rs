//! PDS synchronization for CRDT documents.
//!
//! Generic sync logic for AT Protocol edit records (root/diff/draft).
//! Works with any client implementing the required traits.

use std::collections::{BTreeMap, HashMap};

use jacquard::bytes::Bytes;
use jacquard::cowstr::ToCowStr;
use jacquard::prelude::*;
use jacquard::smol_str::format_smolstr;
use jacquard::types::blob::MimeType;
use jacquard::types::ident::AtIdentifier;
use jacquard::types::recordkey::RecordKey;
use jacquard::types::string::{AtUri, Cid, Did, Nsid};
use jacquard::types::tid::Ticker;
use jacquard::types::uri::Uri;
use jacquard::url::Url;
use jacquard::{CowStr, IntoStatic, to_data};
use loro::{ExportMode, LoroDoc};
use weaver_api::com_atproto::repo::create_record::CreateRecord;
use weaver_api::com_atproto::repo::strong_ref::StrongRef;
use weaver_api::sh_weaver::edit::diff::Diff;
use weaver_api::sh_weaver::edit::draft::Draft;
use weaver_api::sh_weaver::edit::root::Root;
use weaver_api::sh_weaver::edit::{DocRef, DocRefValue, DraftRef, EntryRef};
use weaver_common::agent::WeaverExt;
use weaver_common::constellation::{GetBacklinksQuery, RecordId};

use crate::CrdtError;
use crate::document::CrdtDocument;

const ROOT_NSID: &str = "sh.weaver.edit.root";
const DIFF_NSID: &str = "sh.weaver.edit.diff";
const DRAFT_NSID: &str = "sh.weaver.edit.draft";
const CONSTELLATION_URL: &str = "https://constellation.microcosm.blue";

/// Result of a sync operation.
#[derive(Clone, Debug)]
pub enum SyncResult {
    /// Created a new root record (first sync).
    CreatedRoot {
        uri: AtUri<'static>,
        cid: Cid<'static>,
    },
    /// Created a new diff record.
    CreatedDiff {
        uri: AtUri<'static>,
        cid: Cid<'static>,
    },
    /// No changes to sync.
    NoChanges,
}

/// Result of creating an edit root.
pub struct CreateRootResult {
    /// The root record URI.
    pub root_uri: AtUri<'static>,
    /// The root record CID.
    pub root_cid: Cid<'static>,
    /// Draft stub StrongRef if this was a new draft.
    pub draft_ref: Option<StrongRef<'static>>,
}

/// Build a DocRef for either a published entry or an unpublished draft.
fn build_doc_ref(
    did: &Did<'_>,
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
        _ => {
            // Transform localStorage key to synthetic AT-URI
            let rkey = extract_draft_rkey(draft_key);
            let canonical_uri = format_smolstr!("at://{}/{}/{}", did, DRAFT_NSID, rkey);

            DocRef {
                value: DocRefValue::DraftRef(Box::new(DraftRef {
                    draft_key: canonical_uri.into(),
                    extra_data: None,
                })),
                extra_data: None,
            }
        }
    }
}

/// Extract the rkey (TID) from a draft key.
fn extract_draft_rkey(draft_key: &str) -> String {
    if let Some(tid) = draft_key.strip_prefix("new:") {
        tid.to_string()
    } else if draft_key.starts_with("at://") {
        draft_key.split('/').last().unwrap_or(draft_key).to_string()
    } else {
        draft_key.to_string()
    }
}

/// Get current DID from session.
async fn get_current_did<C>(client: &C) -> Result<Did<'static>, CrdtError>
where
    C: AgentSession,
{
    client
        .session_info()
        .await
        .map(|(did, _)| did)
        .ok_or(CrdtError::NotAuthenticated)
}

/// Create the draft stub record on PDS.
async fn create_draft_stub<C>(
    client: &C,
    did: &Did<'_>,
    rkey: &str,
) -> Result<(AtUri<'static>, Cid<'static>), CrdtError>
where
    C: XrpcClient + AgentSession,
{
    let draft = Draft::new()
        .created_at(jacquard::types::datetime::Datetime::now())
        .build();

    let draft_data =
        to_data(&draft).map_err(|e| CrdtError::Serialization(format!("draft: {}", e)))?;

    let record_key =
        RecordKey::any(rkey).map_err(|e| CrdtError::InvalidUri(format!("rkey: {}", e)))?;

    let collection =
        Nsid::new(DRAFT_NSID).map_err(|e| CrdtError::InvalidUri(format!("nsid: {}", e)))?;

    let request = CreateRecord::new()
        .repo(AtIdentifier::Did(did.clone().into_static()))
        .collection(collection)
        .rkey(record_key)
        .record(draft_data)
        .build();

    let response = client
        .send(request)
        .await
        .map_err(|e| CrdtError::Xrpc(e.to_string()))?;

    let output = response
        .into_output()
        .map_err(|e| CrdtError::Xrpc(e.to_string()))?;

    Ok((output.uri.into_static(), output.cid.into_static()))
}

/// Create the edit root record for a document.
pub async fn create_edit_root<C, D>(
    client: &C,
    doc: &D,
    draft_key: &str,
    entry_uri: Option<&AtUri<'_>>,
    entry_cid: Option<&Cid<'_>>,
) -> Result<CreateRootResult, CrdtError>
where
    C: XrpcClient + IdentityResolver + AgentSession,
    D: CrdtDocument,
{
    let did = get_current_did(client).await?;

    // For drafts, create the stub record first
    let draft_ref: Option<StrongRef<'static>> = if entry_uri.is_none() {
        let rkey = extract_draft_rkey(draft_key);
        match create_draft_stub(client, &did, &rkey).await {
            Ok((uri, cid)) => Some(StrongRef::new().uri(uri).cid(cid).build()),
            Err(e) => {
                let err_str = e.to_string();
                if err_str.contains("RecordAlreadyExists") || err_str.contains("already exists") {
                    // Draft exists, try to fetch it
                    let draft_uri_str = format!("at://{}/{}/{}", did, DRAFT_NSID, rkey);
                    if let Ok(draft_uri) = AtUri::new(&draft_uri_str) {
                        if let Ok(response) = client.get_record::<Draft>(&draft_uri).await {
                            if let Ok(output) = response.into_output() {
                                output.cid.map(|cid| {
                                    StrongRef::new()
                                        .uri(draft_uri.into_static())
                                        .cid(cid.into_static())
                                        .build()
                                })
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                } else {
                    tracing::warn!("Failed to create draft stub: {}", e);
                    None
                }
            }
        }
    } else {
        None
    };

    // Export full snapshot
    let snapshot = doc.export_snapshot();

    // Upload snapshot blob
    let mime_type = MimeType::new_static("application/octet-stream");
    let blob_ref = client
        .upload_blob(snapshot, mime_type)
        .await
        .map_err(|e| CrdtError::Xrpc(format!("upload blob: {}", e)))?;

    // Build DocRef
    let doc_ref = build_doc_ref(&did, draft_key, entry_uri, entry_cid);

    // Build root record
    let root = Root::new().doc(doc_ref).snapshot(blob_ref).build();

    let root_data = to_data(&root).map_err(|e| CrdtError::Serialization(format!("root: {}", e)))?;

    // Generate TID for the root rkey
    let root_tid = Ticker::new().next(None);
    let rkey = RecordKey::any(root_tid.as_str())
        .map_err(|e| CrdtError::InvalidUri(format!("rkey: {}", e)))?;

    let collection =
        Nsid::new(ROOT_NSID).map_err(|e| CrdtError::InvalidUri(format!("nsid: {}", e)))?;

    let request = CreateRecord::new()
        .repo(AtIdentifier::Did(did))
        .collection(collection)
        .rkey(rkey)
        .record(root_data)
        .build();

    let response = client
        .send(request)
        .await
        .map_err(|e| CrdtError::Xrpc(e.to_string()))?;

    let output = response
        .into_output()
        .map_err(|e| CrdtError::Xrpc(e.to_string()))?;

    Ok(CreateRootResult {
        root_uri: output.uri.into_static(),
        root_cid: output.cid.into_static(),
        draft_ref,
    })
}

/// Create a diff record with updates since the last sync.
pub async fn create_diff<C, D>(
    client: &C,
    doc: &D,
    root_uri: &AtUri<'_>,
    root_cid: &Cid<'_>,
    prev_diff: Option<(&AtUri<'_>, &Cid<'_>)>,
    draft_key: &str,
    entry_uri: Option<&AtUri<'_>>,
    entry_cid: Option<&Cid<'_>>,
) -> Result<Option<(AtUri<'static>, Cid<'static>)>, CrdtError>
where
    C: XrpcClient + IdentityResolver + AgentSession,
    D: CrdtDocument,
{
    // Export updates since last sync
    let updates = match doc.export_updates_since_sync() {
        Some(u) => u,
        None => return Ok(None),
    };

    let did = get_current_did(client).await?;

    // Threshold for inline vs blob storage (8KB max for inline per lexicon)
    const INLINE_THRESHOLD: usize = 8192;

    let (blob_ref, inline_diff): (Option<jacquard::types::blob::BlobRef<'static>>, _) =
        if updates.len() <= INLINE_THRESHOLD {
            (None, Some(jacquard::bytes::Bytes::from(updates)))
        } else {
            let mime_type = MimeType::new_static("application/octet-stream");
            let blob = client
                .upload_blob(updates, mime_type)
                .await
                .map_err(|e| CrdtError::Xrpc(format!("upload diff: {}", e)))?;
            (Some(blob.into()), None)
        };

    // Build DocRef
    let doc_ref = build_doc_ref(&did, draft_key, entry_uri, entry_cid);

    // Build root reference
    let root_ref = StrongRef::new()
        .uri(root_uri.clone().into_static())
        .cid(root_cid.clone().into_static())
        .build();

    // Build prev reference
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
        .maybe_snapshot(blob_ref)
        .maybe_inline_diff(inline_diff)
        .maybe_prev(prev_ref)
        .build();

    let diff_data = to_data(&diff).map_err(|e| CrdtError::Serialization(format!("diff: {}", e)))?;

    // Generate TID for the diff rkey
    let diff_tid = Ticker::new().next(None);
    let rkey = RecordKey::any(diff_tid.as_str())
        .map_err(|e| CrdtError::InvalidUri(format!("rkey: {}", e)))?;

    let collection =
        Nsid::new(DIFF_NSID).map_err(|e| CrdtError::InvalidUri(format!("nsid: {}", e)))?;

    let request = CreateRecord::new()
        .repo(AtIdentifier::Did(did))
        .collection(collection)
        .rkey(rkey)
        .record(diff_data)
        .build();

    let response = client
        .send(request)
        .await
        .map_err(|e| CrdtError::Xrpc(e.to_string()))?;

    let output = response
        .into_output()
        .map_err(|e| CrdtError::Xrpc(e.to_string()))?;

    Ok(Some((output.uri.into_static(), output.cid.into_static())))
}

/// Sync the document to the PDS.
pub async fn sync_to_pds<C, D>(
    client: &C,
    doc: &mut D,
    draft_key: &str,
    entry_uri: Option<&AtUri<'_>>,
    entry_cid: Option<&Cid<'_>>,
) -> Result<SyncResult, CrdtError>
where
    C: XrpcClient + IdentityResolver + AgentSession,
    D: CrdtDocument,
{
    if !doc.has_unsynced_changes() {
        return Ok(SyncResult::NoChanges);
    }

    if doc.edit_root().is_none() {
        // First sync - create root
        let result = create_edit_root(client, doc, draft_key, entry_uri, entry_cid).await?;

        let root_ref = StrongRef::new()
            .uri(result.root_uri.clone())
            .cid(result.root_cid.clone())
            .build();

        doc.set_edit_root(Some(root_ref));
        doc.set_last_diff(None);
        doc.mark_synced();

        Ok(SyncResult::CreatedRoot {
            uri: result.root_uri,
            cid: result.root_cid,
        })
    } else {
        // Subsequent sync - create diff
        let root = doc.edit_root().unwrap();
        let prev = doc.last_diff();

        let prev_refs = prev.as_ref().map(|p| (&p.uri, &p.cid));

        let result = create_diff(
            client, doc, &root.uri, &root.cid, prev_refs, draft_key, entry_uri, entry_cid,
        )
        .await?;

        match result {
            Some((uri, cid)) => {
                let diff_ref = StrongRef::new().uri(uri.clone()).cid(cid.clone()).build();
                doc.set_last_diff(Some(diff_ref));
                doc.mark_synced();

                Ok(SyncResult::CreatedDiff { uri, cid })
            }
            None => Ok(SyncResult::NoChanges),
        }
    }
}

/// Find all edit roots for an entry using weaver-index.
#[cfg(feature = "use-index")]
pub async fn find_all_edit_roots<C>(
    client: &C,
    entry_uri: &AtUri<'_>,
    _collaborator_dids: Vec<Did<'static>>,
) -> Result<Vec<RecordId<'static>>, CrdtError>
where
    C: WeaverExt,
{
    use jacquard::types::ident::AtIdentifier;
    use jacquard::types::nsid::Nsid;
    use weaver_api::sh_weaver::edit::get_edit_history::GetEditHistory;

    let response = client
        .send(GetEditHistory::new().resource(entry_uri.clone()).build())
        .await
        .map_err(|e| CrdtError::Xrpc(format!("get edit history: {}", e)))?;

    let output = response
        .into_output()
        .map_err(|e| CrdtError::Xrpc(format!("parse edit history: {}", e)))?;

    let roots: Vec<RecordId<'static>> = output
        .roots
        .into_iter()
        .filter_map(|entry| {
            let uri = AtUri::new(entry.uri.as_ref()).ok()?;
            let did = match uri.authority() {
                AtIdentifier::Did(d) => d.clone().into_static(),
                _ => return None,
            };
            let rkey = uri.rkey()?.clone().into_static();
            Some(RecordId {
                did,
                collection: Nsid::raw(ROOT_NSID).into_static(),
                rkey,
            })
        })
        .collect();

    tracing::debug!("find_all_edit_roots (index): found {} roots", roots.len());

    Ok(roots)
}

/// Find all edit roots for an entry using Constellation backlinks.
#[cfg(not(feature = "use-index"))]
pub async fn find_all_edit_roots<C>(
    client: &C,
    entry_uri: &AtUri<'_>,
    collaborator_dids: Vec<Did<'static>>,
) -> Result<Vec<RecordId<'static>>, CrdtError>
where
    C: XrpcClient,
{
    let constellation_url =
        Url::parse(CONSTELLATION_URL).map_err(|e| CrdtError::InvalidUri(e.to_string()))?;

    let query = GetBacklinksQuery {
        subject: Uri::At(entry_uri.clone().into_static()),
        source: format_smolstr!("{}:doc.value.entry.uri", ROOT_NSID).into(),
        cursor: None,
        did: collaborator_dids,
        limit: 100,
    };

    let response = client
        .xrpc(constellation_url)
        .send(&query)
        .await
        .map_err(|e| CrdtError::Xrpc(e.to_string()))?;

    let output = response
        .into_output()
        .map_err(|e| CrdtError::Xrpc(e.to_string()))?;

    Ok(output
        .records
        .into_iter()
        .map(|r| r.into_static())
        .collect())
}

/// Find all diffs for a root record using Constellation backlinks.
pub async fn find_diffs_for_root<C>(
    client: &C,
    root_uri: &AtUri<'_>,
) -> Result<Vec<RecordId<'static>>, CrdtError>
where
    C: XrpcClient,
{
    let constellation_url =
        Url::parse(CONSTELLATION_URL).map_err(|e| CrdtError::InvalidUri(e.to_string()))?;

    let mut all_diffs = Vec::new();
    let mut cursor: Option<String> = None;

    loop {
        let query = GetBacklinksQuery {
            subject: Uri::At(root_uri.clone().into_static()),
            source: format_smolstr!("{}:root.uri", DIFF_NSID).into(),
            cursor: cursor.map(Into::into),
            did: vec![],
            limit: 100,
        };

        let response = client
            .xrpc(constellation_url.clone())
            .send(&query)
            .await
            .map_err(|e| CrdtError::Xrpc(e.to_string()))?;

        let output = response
            .into_output()
            .map_err(|e| CrdtError::Xrpc(e.to_string()))?;

        all_diffs.extend(output.records.into_iter().map(|r| r.into_static()));

        match output.cursor {
            Some(c) => cursor = Some(c.to_string()),
            None => break,
        }
    }

    Ok(all_diffs)
}

// ============================================================================
// Loading functions
// ============================================================================

/// Result of loading edit state from PDS.
#[derive(Clone, Debug)]
pub struct PdsEditState {
    /// The root record reference.
    pub root_ref: StrongRef<'static>,
    /// The latest diff reference (if any diffs exist).
    pub last_diff_ref: Option<StrongRef<'static>>,
    /// The Loro snapshot bytes from the root.
    pub root_snapshot: Bytes,
    /// All diff update bytes in order (oldest first, by TID).
    pub diff_updates: Vec<Bytes>,
    /// Last seen diff URI per collaborator root (for incremental sync).
    pub last_seen_diffs: HashMap<AtUri<'static>, AtUri<'static>>,
    /// The DocRef from the root record.
    pub doc_ref: DocRef<'static>,
}

/// Find edit root for a draft using Constellation backlinks.
pub async fn find_edit_root_for_draft<C>(
    client: &C,
    draft_uri: &AtUri<'_>,
) -> Result<Option<RecordId<'static>>, CrdtError>
where
    C: XrpcClient,
{
    let constellation_url =
        Url::parse(CONSTELLATION_URL).map_err(|e| CrdtError::InvalidUri(e.to_string()))?;

    let query = GetBacklinksQuery {
        subject: Uri::At(draft_uri.clone().into_static()),
        source: format_smolstr!("{}:doc.value.draft_key", ROOT_NSID).into(),
        cursor: None,
        did: vec![],
        limit: 1,
    };

    let response = client
        .xrpc(constellation_url)
        .send(&query)
        .await
        .map_err(|e| CrdtError::Xrpc(format!("constellation query: {}", e)))?;

    let output = response
        .into_output()
        .map_err(|e| CrdtError::Xrpc(format!("parse constellation: {}", e)))?;

    Ok(output.records.into_iter().next().map(|r| r.into_static()))
}

/// Build a canonical draft URI from draft key and DID.
pub fn build_draft_uri(did: &Did<'_>, draft_key: &str) -> AtUri<'static> {
    let rkey = extract_draft_rkey(draft_key);
    let uri_str = format_smolstr!("at://{}/{}/{}", did, DRAFT_NSID, rkey);
    AtUri::new(&uri_str).unwrap().into_static()
}

/// Load edit state from a root record ID.
async fn load_edit_state_from_root_id<C>(
    client: &C,
    root_id: RecordId<'static>,
    after_rkey: Option<&str>,
) -> Result<Option<PdsEditState>, CrdtError>
where
    C: WeaverExt,
{
    let root_uri = AtUri::new(&format_smolstr!(
        "at://{}/{}/{}",
        root_id.did,
        ROOT_NSID,
        root_id.rkey.as_ref()
    ))
    .map_err(|e| CrdtError::InvalidUri(format!("root URI: {}", e)))?
    .into_static();

    let root_response = client
        .get_record::<Root>(&root_uri)
        .await
        .map_err(|e| CrdtError::Xrpc(format!("fetch root: {}", e)))?;

    let root_output = root_response
        .into_output()
        .map_err(|e| CrdtError::Xrpc(format!("parse root: {}", e)))?;

    let root_cid = root_output
        .cid
        .ok_or_else(|| CrdtError::Xrpc("root missing CID".into()))?;

    let root_ref = StrongRef::new()
        .uri(root_uri.clone())
        .cid(root_cid.into_static())
        .build();

    let doc_ref = root_output.value.doc.into_static();

    let root_snapshot = client
        .fetch_blob(&root_id.did, root_output.value.snapshot.blob().cid())
        .await
        .map_err(|e| CrdtError::Xrpc(format!("fetch snapshot blob: {}", e)))?;

    let diff_ids = find_diffs_for_root(client, &root_uri).await?;

    if diff_ids.is_empty() {
        return Ok(Some(PdsEditState {
            root_ref,
            last_diff_ref: None,
            root_snapshot,
            diff_updates: vec![],
            last_seen_diffs: HashMap::new(),
            doc_ref,
        }));
    }

    let mut diffs_by_rkey: BTreeMap<
        CowStr<'static>,
        (Diff<'static>, Cid<'static>, AtUri<'static>),
    > = BTreeMap::new();

    for diff_id in &diff_ids {
        let rkey_str: &str = diff_id.rkey.as_ref();

        if let Some(after) = after_rkey {
            if rkey_str <= after {
                continue;
            }
        }

        let diff_uri = AtUri::new(&format_smolstr!(
            "at://{}/{}/{}",
            diff_id.did,
            DIFF_NSID,
            rkey_str
        ))
        .map_err(|e| CrdtError::InvalidUri(format!("diff URI: {}", e)))?
        .into_static();

        let diff_response = client
            .get_record::<Diff>(&diff_uri)
            .await
            .map_err(|e| CrdtError::Xrpc(format!("fetch diff: {}", e)))?;

        let diff_output = diff_response
            .into_output()
            .map_err(|e| CrdtError::Xrpc(format!("parse diff: {}", e)))?;

        let diff_cid = diff_output
            .cid
            .ok_or_else(|| CrdtError::Xrpc("diff missing CID".into()))?;

        diffs_by_rkey.insert(
            rkey_str.to_cowstr().into_static(),
            (
                diff_output.value.into_static(),
                diff_cid.into_static(),
                diff_uri,
            ),
        );
    }

    let mut diff_updates = Vec::new();
    let mut last_diff_ref = None;

    for (_rkey, (diff, cid, uri)) in &diffs_by_rkey {
        let diff_bytes = if let Some(ref inline) = diff.inline_diff {
            inline.clone()
        } else if let Some(ref snapshot) = diff.snapshot {
            client
                .fetch_blob(&root_id.did, snapshot.blob().cid())
                .await
                .map_err(|e| CrdtError::Xrpc(format!("fetch diff blob: {}", e)))?
        } else {
            tracing::warn!("Diff has neither inline_diff nor snapshot, skipping");
            continue;
        };

        diff_updates.push(diff_bytes);
        last_diff_ref = Some(StrongRef::new().uri(uri.clone()).cid(cid.clone()).build());
    }

    Ok(Some(PdsEditState {
        root_ref,
        last_diff_ref,
        root_snapshot,
        diff_updates,
        last_seen_diffs: HashMap::new(),
        doc_ref,
    }))
}

/// Load edit state from PDS for an entry (single root).
pub async fn load_edit_state_from_entry<C>(
    client: &C,
    entry_uri: &AtUri<'_>,
    collaborator_dids: Vec<Did<'static>>,
) -> Result<Option<PdsEditState>, CrdtError>
where
    C: WeaverExt,
{
    let root_id = match find_all_edit_roots(client, entry_uri, collaborator_dids)
        .await?
        .into_iter()
        .next()
    {
        Some(id) => id,
        None => return Ok(None),
    };

    load_edit_state_from_root_id(client, root_id, None).await
}

/// Load edit state from PDS for a draft.
pub async fn load_edit_state_from_draft<C>(
    client: &C,
    draft_uri: &AtUri<'_>,
) -> Result<Option<PdsEditState>, CrdtError>
where
    C: WeaverExt,
{
    let root_id = match find_edit_root_for_draft(client, draft_uri).await? {
        Some(id) => id,
        None => return Ok(None),
    };

    load_edit_state_from_root_id(client, root_id, None).await
}

/// Load and merge edit states from ALL collaborator repos.
pub async fn load_all_edit_states<C>(
    client: &C,
    entry_uri: &AtUri<'_>,
    collaborator_dids: Vec<Did<'static>>,
    current_did: Option<&Did<'_>>,
    last_seen_diffs: &HashMap<AtUri<'static>, AtUri<'static>>,
) -> Result<Option<PdsEditState>, CrdtError>
where
    C: WeaverExt,
{
    let all_roots = find_all_edit_roots(client, entry_uri, collaborator_dids).await?;

    if all_roots.is_empty() {
        return Ok(None);
    }

    let merged_doc = LoroDoc::new();
    let mut our_root_ref: Option<StrongRef<'static>> = None;
    let mut our_last_diff_ref: Option<StrongRef<'static>> = None;
    let mut merged_doc_ref: Option<DocRef<'static>> = None;
    let mut updated_last_seen = last_seen_diffs.clone();

    for root_id in all_roots {
        let root_did = root_id.did.clone();

        let root_uri = AtUri::new(&format_smolstr!(
            "at://{}/{}/{}",
            root_id.did,
            ROOT_NSID,
            root_id.rkey.as_ref()
        ))
        .ok()
        .map(|u| u.into_static());

        let after_rkey = root_uri.as_ref().and_then(|uri| {
            last_seen_diffs
                .get(uri)
                .and_then(|diff_uri| diff_uri.rkey().map(|rk| rk.0.to_string()))
        });

        if let Some(pds_state) =
            load_edit_state_from_root_id(client, root_id, after_rkey.as_deref()).await?
        {
            if let Err(e) = merged_doc.import(&pds_state.root_snapshot) {
                tracing::warn!("Failed to import root snapshot from {}: {:?}", root_did, e);
                continue;
            }

            for diff in &pds_state.diff_updates {
                if let Err(e) = merged_doc.import(diff) {
                    tracing::warn!("Failed to import diff from {}: {:?}", root_did, e);
                }
            }

            if let (Some(uri), Some(last_diff)) = (&root_uri, &pds_state.last_diff_ref) {
                updated_last_seen.insert(uri.clone(), last_diff.uri.clone().into_static());
            }

            if merged_doc_ref.is_none() {
                merged_doc_ref = Some(pds_state.doc_ref.clone());
            }

            let is_our_root = current_did.is_some_and(|did| root_did == *did);

            if is_our_root {
                our_root_ref = Some(pds_state.root_ref);
                our_last_diff_ref = pds_state.last_diff_ref;
            } else if our_root_ref.is_none() {
                our_root_ref = Some(pds_state.root_ref);
                our_last_diff_ref = pds_state.last_diff_ref;
            }
        }
    }

    let merged_snapshot = merged_doc
        .export(ExportMode::Snapshot)
        .map_err(|e| CrdtError::Loro(format!("export merged: {}", e)))?;

    Ok(our_root_ref.map(|root_ref| PdsEditState {
        root_ref,
        last_diff_ref: our_last_diff_ref,
        root_snapshot: merged_snapshot.into(),
        diff_updates: vec![],
        last_seen_diffs: updated_last_seen,
        doc_ref: merged_doc_ref.expect("Should have doc_ref if we have root"),
    }))
}

/// Remote draft info from PDS.
#[derive(Clone, Debug)]
pub struct RemoteDraft {
    /// The draft record URI.
    pub uri: AtUri<'static>,
    /// The rkey (TID) of the draft.
    pub rkey: String,
    /// When the draft was created.
    pub created_at: String,
}

/// List all drafts for a user using weaver-index.
#[cfg(feature = "use-index")]
pub async fn list_drafts<C>(client: &C, did: &Did<'_>) -> Result<Vec<RemoteDraft>, CrdtError>
where
    C: WeaverExt,
{
    use jacquard::types::ident::AtIdentifier;
    use weaver_api::sh_weaver::edit::list_drafts::ListDrafts;

    let actor = AtIdentifier::Did(did.clone().into_static());
    let response = client
        .send(ListDrafts::new().actor(actor).build())
        .await
        .map_err(|e| CrdtError::Xrpc(format!("list drafts: {}", e)))?;

    let output = response
        .into_output()
        .map_err(|e| CrdtError::Xrpc(format!("parse list drafts: {}", e)))?;

    tracing::debug!("list_drafts (index): found {} drafts", output.drafts.len());

    let drafts = output
        .drafts
        .into_iter()
        .filter_map(|draft| {
            let uri = AtUri::new(draft.uri.as_ref()).ok()?.into_static();
            let rkey = uri.rkey()?.0.as_str().to_string();
            let created_at = draft.created_at.to_string();
            Some(RemoteDraft {
                uri,
                rkey,
                created_at,
            })
        })
        .collect();

    Ok(drafts)
}

/// List all drafts for a user (direct PDS query, no index).
#[cfg(not(feature = "use-index"))]
pub async fn list_drafts<C>(client: &C, did: &Did<'_>) -> Result<Vec<RemoteDraft>, CrdtError>
where
    C: WeaverExt,
{
    use weaver_api::com_atproto::repo::list_records::ListRecords;

    let pds_url = client
        .pds_for_did(did)
        .await
        .map_err(|e| CrdtError::Xrpc(format!("resolve DID: {}", e)))?;

    let collection =
        Nsid::new(DRAFT_NSID).map_err(|e| CrdtError::InvalidUri(format!("nsid: {}", e)))?;

    let request = ListRecords::new()
        .repo(did.clone())
        .collection(collection)
        .limit(100)
        .build();

    let response = client
        .xrpc(pds_url)
        .send(&request)
        .await
        .map_err(|e| CrdtError::Xrpc(format!("list records: {}", e)))?;

    let output = response
        .into_output()
        .map_err(|e| CrdtError::Xrpc(format!("parse list records: {}", e)))?;

    let mut drafts = Vec::new();
    for record in output.records {
        let rkey = record
            .uri
            .rkey()
            .map(|r| r.0.as_str().to_string())
            .unwrap_or_default();

        let created_at = jacquard::from_data::<Draft>(&record.value)
            .map(|d| d.created_at.to_string())
            .unwrap_or_default();

        drafts.push(RemoteDraft {
            uri: record.uri.into_static(),
            rkey,
            created_at,
        });
    }

    Ok(drafts)
}
