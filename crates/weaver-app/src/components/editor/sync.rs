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

use super::document::{EditorDocument, LoadedDocState};
use crate::fetch::Fetcher;
use jacquard::bytes::Bytes;
use jacquard::cowstr::ToCowStr;
use jacquard::prelude::*;
use jacquard::smol_str::format_smolstr;
use jacquard::types::blob::MimeType;
use jacquard::types::collection::Collection;
use jacquard::types::ident::AtIdentifier;
use jacquard::types::recordkey::RecordKey;
use jacquard::types::string::{AtUri, Cid, Did, Nsid};
use jacquard::types::tid::Ticker;
use jacquard::types::uri::Uri;
use jacquard::url::Url;
use jacquard::{CowStr, IntoStatic, to_data};
use loro::LoroDoc;
use loro::ToJson;
use weaver_api::com_atproto::repo::create_record::CreateRecord;
use weaver_api::com_atproto::repo::strong_ref::StrongRef;
use weaver_api::com_atproto::sync::get_blob::GetBlob;
use weaver_api::sh_weaver::edit::diff::Diff;
use weaver_api::sh_weaver::edit::draft::Draft;
use weaver_api::sh_weaver::edit::root::Root;
use weaver_api::sh_weaver::edit::{DocRef, DocRefValue, DraftRef, EntryRef};
use weaver_common::constellation::{GetBacklinksQuery, RecordId};
use weaver_common::{WeaverError, WeaverExt};

const ROOT_NSID: &str = "sh.weaver.edit.root";
const DIFF_NSID: &str = "sh.weaver.edit.diff";
const DRAFT_NSID: &str = "sh.weaver.edit.draft";
const CONSTELLATION_URL: &str = "https://constellation.microcosm.blue";

/// Extract record embeds from a LoroDoc and pre-fetch their rendered content.
///
/// Reads the embeds.records list from the document, extracts RecordEmbed entries,
/// and fetches/renders each one to populate a ResolvedContent map.
/// Also pre-warms the blob cache for images if `owner_ident` is provided.
async fn prefetch_embeds_from_doc(
    doc: &LoroDoc,
    fetcher: &Fetcher,
    owner_ident: Option<&str>,
) -> weaver_common::ResolvedContent {
    use weaver_api::sh_weaver::embed::images::Image;
    use weaver_api::sh_weaver::embed::records::RecordEmbed;

    let mut resolved = weaver_common::ResolvedContent::default();

    let embeds_map = doc.get_map("embeds");

    // Pre-warm blob cache for images
    #[cfg(feature = "fullstack-server")]
    if let Some(ident) = owner_ident {
        if let Ok(images_container) =
            embeds_map.get_or_create_container("images", loro::LoroList::new())
        {
            for i in 0..images_container.len() {
                let Some(value) = images_container.get(i) else {
                    continue;
                };
                let Some(loro_value) = value.as_value() else {
                    continue;
                };
                let json = loro_value.to_json_value();
                let Ok(image) = jacquard::from_json_value::<Image>(json) else {
                    continue;
                };

                let cid = image.image.blob().cid();
                let name = image.name.as_ref().map(|n| n.as_ref());
                if let Err(e) = crate::data::cache_blob(
                    ident.into(),
                    cid.as_ref().into(),
                    name.map(|n| n.into()),
                )
                .await
                {
                    tracing::warn!("Failed to pre-warm blob cache for {}: {}", cid, e);
                }
            }
        }
    }
    #[cfg(not(feature = "fullstack-server"))]
    let _ = owner_ident;

    // Strategy 1: Get embeds from Loro embeds map -> records list

    if let Ok(records_container) =
        embeds_map.get_or_create_container("records", loro::LoroList::new())
    {
        for i in 0..records_container.len() {
            let Some(value) = records_container.get(i) else {
                continue;
            };
            let Some(loro_value) = value.as_value() else {
                continue;
            };
            let json = loro_value.to_json_value();
            let Ok(record_embed) = jacquard::from_json_value::<RecordEmbed>(json) else {
                continue;
            };

            // name is the key used in markdown, fallback to record.uri
            let key_uri = if let Some(ref name) = record_embed.name {
                match AtUri::new(name.as_ref()) {
                    Ok(uri) => uri.into_static(),
                    Err(_) => continue,
                }
            } else {
                record_embed.record.uri.clone().into_static()
            };

            // Fetch and render
            match weaver_renderer::atproto::fetch_and_render(&record_embed.record.uri, fetcher)
                .await
            {
                Ok(html) => {
                    resolved.add_embed(key_uri, html, None);
                }
                Err(e) => {
                    tracing::warn!(
                        "Failed to pre-fetch embed {}: {}",
                        record_embed.record.uri,
                        e
                    );
                }
            }
        }
    }

    // Strategy 2: If no embeds found in Loro map, parse markdown text
    if resolved.embed_content.is_empty() {
        use weaver_common::{ExtractedRef, collect_refs_from_markdown};

        let text = doc.get_text("content");
        let markdown = text.to_string();

        if !markdown.is_empty() {
            let refs = collect_refs_from_markdown(&markdown);

            for extracted in refs {
                if let ExtractedRef::AtEmbed { uri, .. } = extracted {
                    let key_uri = match AtUri::new(&uri) {
                        Ok(u) => u.into_static(),
                        Err(_) => continue,
                    };

                    match weaver_renderer::atproto::fetch_and_render(&key_uri, fetcher).await {
                        Ok(html) => {
                            resolved.add_embed(key_uri, html, None);
                        }
                        Err(e) => {
                            tracing::warn!("Failed to pre-fetch embed {}: {}", uri, e);
                        }
                    }
                }
            }
        }
    }

    resolved
}

/// Build a DocRef for either a published entry or an unpublished draft.
///
/// If entry_uri and entry_cid are provided, creates an EntryRef.
/// Otherwise, creates a DraftRef with a synthetic AT-URI for Constellation indexing.
///
/// The synthetic URI format is: `at://{did}/sh.weaver.edit.draft/{rkey}`
/// This allows Constellation to index drafts as backlinks, enabling discovery.
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
            // Transform localStorage key to synthetic AT-URI for Constellation indexing
            // localStorage uses "new:{tid}" or AT-URI, PDS uses "at://{did}/sh.weaver.edit.draft/{rkey}"
            let rkey = if let Some(tid) = draft_key.strip_prefix("new:") {
                // New draft: extract TID as rkey
                tid.to_string()
            } else if draft_key.starts_with("at://") {
                // Editing existing entry: use the entry's rkey
                draft_key.split('/').last().unwrap_or(draft_key).to_string()
            } else if draft_key.starts_with("did:") && draft_key.contains(':') {
                // Old canonical format "did:xxx:rkey" - extract rkey
                draft_key
                    .rsplit(':')
                    .next()
                    .unwrap_or(draft_key)
                    .to_string()
            } else {
                // Fallback: use as-is
                draft_key.to_string()
            };

            // Build AT-URI pointing to actual draft record: at://{did}/sh.weaver.edit.draft/{rkey}
            let canonical_uri = format_smolstr!("at://{}/{}/{}", did, DRAFT_NSID, rkey);

            DocRef {
                value: DocRefValue::DraftRef(Box::new(DraftRef {
                    draft_key: CowStr::from(canonical_uri),
                    extra_data: None,
                })),
                extra_data: None,
            }
        }
    }
}

/// Convert a DocRef to an entry_ref StrongRef.
///
/// For EntryRef: returns the entry's StrongRef directly
/// For DraftRef: parses the draft_key as AT-URI, fetches the draft record to get CID, builds StrongRef
/// For NotebookRef: returns the notebook's StrongRef
async fn doc_ref_to_entry_ref(
    fetcher: &Fetcher,
    doc_ref: &DocRef<'_>,
) -> Option<StrongRef<'static>> {
    match &doc_ref.value {
        DocRefValue::EntryRef(entry_ref) => Some(entry_ref.entry.clone().into_static()),
        DocRefValue::DraftRef(draft_ref) => {
            // draft_key contains the canonical AT-URI: at://{did}/sh.weaver.edit.draft/{rkey}
            let draft_uri = AtUri::new(&draft_ref.draft_key).ok()?.into_static();

            // Fetch the draft record to get its CID
            match fetcher.client.get_record::<Draft>(&draft_uri).await {
                Ok(response) => {
                    let output = response.into_output().ok()?;
                    let cid = output.cid?.into_static();
                    Some(StrongRef::new().uri(draft_uri).cid(cid).build())
                }
                Err(e) => {
                    tracing::warn!("Failed to fetch draft record for entry_ref: {}", e);
                    None
                }
            }
        }
        DocRefValue::NotebookRef(notebook_ref) => Some(notebook_ref.notebook.clone().into_static()),
        DocRefValue::Unknown(_) => {
            tracing::warn!("Unknown DocRefValue variant, cannot convert to entry_ref");
            None
        }
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

/// Find ALL edit.root records across collaborators for an entry.
///
/// With use-index: Uses weaver-index getEditHistory endpoint.
/// Without use-index: Queries Constellation for backlinks.
#[cfg(feature = "use-index")]
pub async fn find_all_edit_roots_for_entry(
    fetcher: &Fetcher,
    entry_uri: &AtUri<'_>,
) -> Result<Vec<RecordId<'static>>, WeaverError> {
    let output = fetcher
        .get_edit_history(entry_uri)
        .await
        .map_err(|e| WeaverError::InvalidNotebook(format!("Failed to get edit history: {}", e)))?;

    // Convert EditHistoryEntry roots to RecordId format
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

    tracing::debug!(
        "find_all_edit_roots_for_entry (index): found {} roots",
        roots.len()
    );

    Ok(roots)
}

/// Find ALL edit.root records across collaborators for an entry.
///
/// 1. Gets list of collaborators via permissions
/// 2. Queries Constellation for edit.root in each collaborator's repo
/// 3. Returns all found roots for CRDT merge
#[cfg(not(feature = "use-index"))]
pub async fn find_all_edit_roots_for_entry(
    fetcher: &Fetcher,
    entry_uri: &AtUri<'_>,
) -> Result<Vec<RecordId<'static>>, WeaverError> {
    // Get collaborators from permissions
    let collaborators = fetcher
        .get_client()
        .find_collaborators_for_resource(entry_uri)
        .await
        .unwrap_or_default();

    // Include the entry owner
    let owner_did = match entry_uri.authority() {
        AtIdentifier::Did(d) => d.clone().into_static(),
        AtIdentifier::Handle(h) => fetcher
            .client
            .resolve_handle(h)
            .await
            .map_err(|e| WeaverError::InvalidNotebook(format!("Failed to resolve handle: {}", e)))?
            .into_static(),
    };

    let all_dids: Vec<Did<'static>> = std::iter::once(owner_did)
        .chain(collaborators.into_iter())
        .collect();

    let constellation_url = Url::parse(CONSTELLATION_URL)
        .map_err(|e| WeaverError::InvalidNotebook(format!("Invalid constellation URL: {}", e)))?;

    let mut all_roots = Vec::new();

    // Query for edit.root records from this DID that reference entry_uri
    let query = GetBacklinksQuery {
        subject: Uri::At(entry_uri.clone().into_static()),
        source: format_smolstr!("{}:doc.value.entry.uri", ROOT_NSID).into(),
        cursor: None,
        did: all_dids.clone(),
        limit: 10,
    };

    let response = fetcher
        .get_client()
        .xrpc(constellation_url.clone())
        .send(&query)
        .await;

    if let Ok(response) = response {
        if let Ok(output) = response.into_output() {
            all_roots.extend(output.records.into_iter().map(|r| r.into_static()));
        } else {
            tracing::warn!("Failed to parse response for edit root query");
        }
    } else {
        tracing::warn!("Failed to fetch edit root query");
    }

    tracing::debug!(
        "find_all_edit_roots_for_entry: found {} roots across {} collaborators",
        all_roots.len(),
        all_dids.len()
    );

    Ok(all_roots)
}

/// Find the edit root for a draft using constellation backlinks.
///
/// Queries constellation for `sh.weaver.edit.root` records that reference
/// the given draft URI via the `.doc.value.draft_key` path.
///
/// The draft_uri should be in canonical format: `at://{did}/sh.weaver.edit.draft/{rkey}`
pub async fn find_edit_root_for_draft(
    fetcher: &Fetcher,
    draft_uri: &AtUri<'_>,
) -> Result<Option<RecordId<'static>>, WeaverError> {
    let constellation_url = Url::parse(CONSTELLATION_URL)
        .map_err(|e| WeaverError::InvalidNotebook(format!("Invalid constellation URL: {}", e)))?;

    let query = GetBacklinksQuery {
        subject: Uri::At(draft_uri.clone().into_static()),
        source: format_smolstr!("{}:doc.value.draft_key", ROOT_NSID).into(),
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

/// Build a canonical draft URI from localStorage key and DID.
///
/// Transforms localStorage format ("new:{tid}" or AT-URI) to
/// draft record URI format: `at://{did}/sh.weaver.edit.draft/{rkey}`
pub fn build_draft_uri(did: &Did<'_>, draft_key: &str) -> AtUri<'static> {
    let rkey = if let Some(tid) = draft_key.strip_prefix("new:") {
        tid.to_string()
    } else if draft_key.starts_with("at://") {
        draft_key.split('/').last().unwrap_or(draft_key).to_string()
    } else {
        draft_key.to_string()
    };

    let uri_str = format_smolstr!("at://{}/{}/{}", did, DRAFT_NSID, rkey);
    // Safe to unwrap: we're constructing a valid AT-URI
    AtUri::new(&uri_str).unwrap().into_static()
}

/// Extract the rkey (TID) from a localStorage draft key.
fn extract_draft_rkey(draft_key: &str) -> String {
    if let Some(tid) = draft_key.strip_prefix("new:") {
        tid.to_string()
    } else if draft_key.starts_with("at://") {
        draft_key.split('/').last().unwrap_or(draft_key).to_string()
    } else {
        draft_key.to_string()
    }
}

/// Create the draft stub record on PDS.
///
/// This creates a minimal `sh.weaver.edit.draft` record that acts as an anchor
/// for edit.root/diff records and enables draft discovery via listRecords.
async fn create_draft_stub(
    fetcher: &Fetcher,
    did: &Did<'_>,
    rkey: &str,
) -> Result<(AtUri<'static>, Cid<'static>), WeaverError> {
    // Build minimal draft record with just createdAt
    let draft = Draft::new()
        .created_at(jacquard::types::datetime::Datetime::now())
        .build();

    let draft_data = to_data(&draft)
        .map_err(|e| WeaverError::InvalidNotebook(format!("Failed to serialize draft: {}", e)))?;

    let record_key =
        RecordKey::any(rkey).map_err(|e| WeaverError::InvalidNotebook(e.to_string()))?;

    let collection = Nsid::new(DRAFT_NSID).map_err(WeaverError::AtprotoString)?;

    let request = CreateRecord::new()
        .repo(AtIdentifier::Did(did.clone().into_static()))
        .collection(collection)
        .rkey(record_key)
        .record(draft_data)
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

/// Remote draft info from PDS.
#[derive(Clone, Debug)]
pub struct RemoteDraft {
    /// The draft record URI
    pub uri: AtUri<'static>,
    /// The rkey (TID) of the draft
    pub rkey: String,
    /// When the draft was created
    pub created_at: String,
}

/// List all drafts for the current user.
///
/// With use-index: Uses weaver-index listDrafts endpoint.
/// Without use-index: Uses direct PDS ListRecords query.
#[cfg(feature = "use-index")]
pub async fn list_drafts_from_pds(fetcher: &Fetcher) -> Result<Vec<RemoteDraft>, WeaverError> {
    let did = fetcher
        .current_did()
        .await
        .ok_or_else(|| WeaverError::InvalidNotebook("Not authenticated".into()))?;

    let actor = AtIdentifier::Did(did);
    let output = fetcher
        .list_drafts(&actor)
        .await
        .map_err(|e| WeaverError::InvalidNotebook(format!("Failed to list drafts: {}", e)))?;

    tracing::debug!(
        "list_drafts_from_pds (index): found {} drafts",
        output.drafts.len()
    );

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

/// List all drafts from PDS for the current user.
///
/// Returns a list of draft records from `sh.weaver.edit.draft` collection.
#[cfg(not(feature = "use-index"))]
pub async fn list_drafts_from_pds(fetcher: &Fetcher) -> Result<Vec<RemoteDraft>, WeaverError> {
    use weaver_api::com_atproto::repo::list_records::ListRecords;

    let did = fetcher
        .current_did()
        .await
        .ok_or_else(|| WeaverError::InvalidNotebook("Not authenticated".into()))?;

    let client = fetcher.get_client();
    let collection = Nsid::new(DRAFT_NSID).map_err(WeaverError::AtprotoString)?;

    let request = ListRecords::new()
        .repo(did)
        .collection(collection)
        .limit(100)
        .build();

    let response = client
        .send(request)
        .await
        .map_err(|e| WeaverError::InvalidNotebook(format!("Failed to list drafts: {}", e)))?;

    let output = response.into_output().map_err(|e| {
        WeaverError::InvalidNotebook(format!("Failed to parse list records response: {}", e))
    })?;

    tracing::debug!(
        "list_drafts_from_pds: found {} records",
        output.records.len()
    );

    let mut drafts = Vec::new();
    for record in output.records {
        let rkey = record
            .uri
            .rkey()
            .map(|r| r.0.as_str().to_string())
            .unwrap_or_default();

        tracing::debug!("  Draft record: uri={}, rkey={}", record.uri, rkey);

        // Parse the draft record to get createdAt
        let created_at =
            jacquard::from_data::<weaver_api::sh_weaver::edit::draft::Draft>(&record.value)
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

/// Find all diffs for a root record using constellation backlinks.
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
            source: format_smolstr!("{}:root.uri", DIFF_NSID).into(),
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

/// Result of creating an edit root, includes optional draft stub info.
pub struct CreateRootResult {
    /// The root record URI
    pub root_uri: AtUri<'static>,
    /// The root record CID
    pub root_cid: Cid<'static>,
    /// Draft stub StrongRef if this was a new draft (not editing published entry)
    pub draft_ref: Option<StrongRef<'static>>,
}

/// Create the edit root record for an entry.
///
/// Uploads the current Loro snapshot as a blob and creates an `sh.weaver.edit.root`
/// record referencing the entry (or draft key if unpublished).
///
/// For drafts, also creates the `sh.weaver.edit.draft` stub record first.
/// Returns the draft stub info so caller can set entry_ref.
pub async fn create_edit_root(
    fetcher: &Fetcher,
    doc: &EditorDocument,
    draft_key: &str,
    entry_uri: Option<&AtUri<'_>>,
    entry_cid: Option<&Cid<'_>>,
) -> Result<CreateRootResult, WeaverError> {
    let client = fetcher.get_client();
    let did = fetcher
        .current_did()
        .await
        .ok_or_else(|| WeaverError::InvalidNotebook("Not authenticated".into()))?;

    // For drafts, create the stub record first (makes it discoverable via listRecords)
    let draft_ref: Option<StrongRef<'static>> = if entry_uri.is_none() {
        let rkey = extract_draft_rkey(draft_key);
        // Try to create draft stub, or get existing one
        match create_draft_stub(fetcher, &did, &rkey).await {
            Ok((uri, cid)) => {
                tracing::debug!("Created draft stub: {}", uri);
                Some(StrongRef::new().uri(uri).cid(cid).build())
            }
            Err(e) => {
                // Check if it's a "record already exists" error
                let err_str = e.to_string();
                if err_str.contains("RecordAlreadyExists") || err_str.contains("already exists") {
                    // Draft exists - fetch it to get the CID
                    let draft_uri_str = format!("at://{}/{}/{}", did, DRAFT_NSID, rkey);
                    if let Ok(draft_uri) = AtUri::new(&draft_uri_str) {
                        if let Ok(response) =
                            fetcher.get_client().get_record::<Draft>(&draft_uri).await
                        {
                            if let Ok(output) = response.into_output() {
                                if let Some(cid) = output.cid {
                                    Some(
                                        StrongRef::new()
                                            .uri(draft_uri.into_static())
                                            .cid(cid.into_static())
                                            .build(),
                                    )
                                } else {
                                    tracing::warn!("Draft exists but has no CID");
                                    None
                                }
                            } else {
                                tracing::warn!("Draft exists but couldn't parse output");
                                None
                            }
                        } else {
                            tracing::warn!("Draft exists but couldn't fetch record");
                            None
                        }
                    } else {
                        None
                    }
                } else {
                    tracing::warn!("Failed to create draft stub (continuing anyway): {}", e);
                    None
                }
            }
        }
    } else {
        None // Published entry, not a draft
    };

    // Export full snapshot
    let snapshot = doc.export_snapshot();

    // Upload snapshot blob
    let mime_type = MimeType::new_static("application/octet-stream");
    let blob_ref = client
        .upload_blob(snapshot, mime_type)
        .await
        .map_err(|e| WeaverError::InvalidNotebook(format!("Failed to upload snapshot: {}", e)))?;

    // Build DocRef - use EntryRef if published, DraftRef if not
    let doc_ref = build_doc_ref(&did, draft_key, entry_uri, entry_cid);

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

    Ok(CreateRootResult {
        root_uri: output.uri.into_static(),
        root_cid: output.cid.into_static(),
        draft_ref,
    })
}

/// Create a diff record with updates since the last sync.
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

    // Threshold for inline vs blob storage (8KB max for inline per lexicon)
    const INLINE_THRESHOLD: usize = 8192;

    // Use inline for small diffs, blob for larger ones
    let (blob_ref, inline_diff): (Option<jacquard::types::blob::BlobRef<'static>>, _) =
        if updates.len() <= INLINE_THRESHOLD {
            (None, Some(jacquard::bytes::Bytes::from(updates)))
        } else {
            let mime_type = MimeType::new_static("application/octet-stream");
            let blob = client.upload_blob(updates, mime_type).await.map_err(|e| {
                WeaverError::InvalidNotebook(format!("Failed to upload diff: {}", e))
            })?;
            (Some(blob.into()), None)
        };

    // Build DocRef - use EntryRef if published, DraftRef if not
    let doc_ref = build_doc_ref(&did, draft_key, entry_uri, entry_cid);

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
        .maybe_snapshot(blob_ref)
        .maybe_inline_diff(inline_diff)
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
/// Updates the document's sync state on success.
pub async fn sync_to_pds(
    fetcher: &Fetcher,
    doc: &mut EditorDocument,
    draft_key: &str,
) -> Result<SyncResult, WeaverError> {
    let fn_start = crate::perf::now();

    // Check if we have changes to sync
    if !doc.has_unsynced_changes() {
        return Ok(SyncResult::NoChanges);
    }

    // Get entry info if published
    let entry_ref = doc.entry_ref();

    if doc.edit_root().is_none() {
        // First sync - create root
        let create_start = crate::perf::now();
        let result = create_edit_root(
            fetcher,
            doc,
            draft_key,
            entry_ref.as_ref().map(|r| &r.uri),
            entry_ref.as_ref().map(|r| &r.cid),
        )
        .await?;
        let create_ms = crate::perf::now() - create_start;

        // Build StrongRef for the root
        let root_ref = StrongRef::new()
            .uri(result.root_uri.clone())
            .cid(result.root_cid.clone())
            .build();

        // Update document state
        doc.set_edit_root(Some(root_ref));
        doc.set_last_diff(None);
        doc.mark_synced();

        // For drafts: set entry_ref to the draft record (enables draft discovery/recovery)
        if let Some(draft_ref) = result.draft_ref {
            if doc.entry_ref().is_none() {
                tracing::debug!("Setting entry_ref to draft: {}", draft_ref.uri);
                doc.set_entry_ref(Some(draft_ref));
            }
        }

        let total_ms = crate::perf::now() - fn_start;
        tracing::debug!(total_ms, create_ms, "sync_to_pds: created root");

        Ok(SyncResult::CreatedRoot {
            uri: result.root_uri,
            cid: result.root_cid,
        })
    } else {
        // Subsequent sync - create diff
        let root_ref = doc.edit_root().unwrap();
        let prev_diff = doc.last_diff();

        let create_start = crate::perf::now();
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
        let create_ms = crate::perf::now() - create_start;

        match result {
            Some((diff_uri, diff_cid)) => {
                // Build StrongRef for the diff
                let diff_ref = StrongRef::new()
                    .uri(diff_uri.clone())
                    .cid(diff_cid.clone())
                    .build();

                doc.set_last_diff(Some(diff_ref));
                doc.mark_synced();

                let total_ms = crate::perf::now() - fn_start;
                tracing::debug!(total_ms, create_ms, "sync_to_pds: created diff");

                Ok(SyncResult::CreatedDiff {
                    uri: diff_uri,
                    cid: diff_cid,
                })
            }
            None => {
                let total_ms = crate::perf::now() - fn_start;
                tracing::debug!(total_ms, create_ms, "sync_to_pds: no changes in diff");
                Ok(SyncResult::NoChanges)
            }
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
    pub root_snapshot: Bytes,
    /// All diff update bytes in order (oldest first, by TID)
    pub diff_updates: Vec<Bytes>,
    /// Last seen diff URI per collaborator root (for incremental sync).
    /// Maps root URI -> last diff URI we've imported from that root.
    pub last_seen_diffs: std::collections::HashMap<AtUri<'static>, AtUri<'static>>,
    /// The DocRef from the root record (tells us what's being edited)
    pub doc_ref: DocRef<'static>,
}

/// Fetch a blob from the PDS.
async fn fetch_blob(fetcher: &Fetcher, did: &Did<'_>, cid: &Cid<'_>) -> Result<Bytes, WeaverError> {
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

    Ok(output.body)
}

/// Load edit state from the PDS for an entry.
///
/// Finds the edit root via constellation backlinks, fetches all diffs,
/// and returns the snapshot + updates needed to reconstruct the document.
pub async fn load_edit_state_from_pds(
    fetcher: &Fetcher,
    entry_uri: &AtUri<'_>,
) -> Result<Option<PdsEditState>, WeaverError> {
    // Find the edit root for this entry (take first if multiple exist)
    let root_id = match find_all_edit_roots_for_entry(fetcher, entry_uri)
        .await?
        .into_iter()
        .next()
    {
        Some(id) => id,
        None => return Ok(None),
    };

    load_edit_state_from_root_id(fetcher, root_id, None).await
}

/// Load edit state from the PDS for a draft.
///
/// Finds the edit root via constellation backlinks using the draft URI,
/// fetches all diffs, and returns the snapshot + updates.
pub async fn load_edit_state_from_draft(
    fetcher: &Fetcher,
    draft_uri: &AtUri<'_>,
) -> Result<Option<PdsEditState>, WeaverError> {
    // Find the edit root for this draft
    let root_id = match find_edit_root_for_draft(fetcher, draft_uri).await? {
        Some(id) => id,
        None => return Ok(None),
    };

    load_edit_state_from_root_id(fetcher, root_id, None).await
}

/// Load edit state from ALL collaborator repos for an entry, returning merged state.
///
/// For each edit.root found across collaborators:
/// - Fetches the root snapshot
/// - Finds and fetches all diffs for that root (skipping already-seen diffs)
/// - Merges all Loro states into one unified document
///
/// `last_seen_diffs` maps root URI -> last diff URI we've imported from that root.
/// This enables incremental sync by only fetching new diffs.
///
/// Returns merged state suitable for CRDT collaboration, including updated last_seen_diffs.
pub async fn load_all_edit_states_from_pds(
    fetcher: &Fetcher,
    entry_uri: &AtUri<'_>,
    last_seen_diffs: &std::collections::HashMap<AtUri<'static>, AtUri<'static>>,
) -> Result<Option<PdsEditState>, WeaverError> {
    let all_roots = find_all_edit_roots_for_entry(fetcher, entry_uri).await?;

    if all_roots.is_empty() {
        return Ok(None);
    }

    // We'll merge all snapshots and diffs into one unified LoroDoc
    let merged_doc = LoroDoc::new();
    let mut our_root_ref: Option<StrongRef<'static>> = None;
    let mut our_last_diff_ref: Option<StrongRef<'static>> = None;
    let mut merged_doc_ref: Option<DocRef<'static>> = None;
    let mut updated_last_seen = last_seen_diffs.clone();

    // Get current user's DID to identify "our" root for sync state tracking
    let current_did = fetcher.current_did().await;

    for root_id in all_roots {
        // Save the DID before consuming root_id
        let root_did = root_id.did.clone();

        // Build root URI to look up last seen diff
        let root_uri = AtUri::new(&format_smolstr!(
            "at://{}/{}/{}",
            root_id.did,
            ROOT_NSID,
            root_id.rkey.as_ref()
        ))
        .ok()
        .map(|u| u.into_static());

        // Get the last seen diff rkey for this root (if any)
        let after_rkey = root_uri.as_ref().and_then(|uri| {
            last_seen_diffs
                .get(uri)
                .and_then(|diff_uri| diff_uri.rkey().map(|rk| rk.0.to_string()))
        });

        // Load state from this root (skipping already-seen diffs)
        if let Some(pds_state) =
            load_edit_state_from_root_id(fetcher, root_id, after_rkey.as_deref()).await?
        {
            // Import root snapshot into merged doc
            if let Err(e) = merged_doc.import(&pds_state.root_snapshot) {
                tracing::warn!("Failed to import root snapshot from {}: {:?}", root_did, e);
                continue;
            }

            // Import all diffs
            for diff in &pds_state.diff_updates {
                if let Err(e) = merged_doc.import(diff) {
                    tracing::warn!("Failed to import diff from {}: {:?}", root_did, e);
                }
            }

            // Update last seen diff for this root (for incremental sync next time)
            if let (Some(uri), Some(last_diff)) = (&root_uri, &pds_state.last_diff_ref) {
                updated_last_seen.insert(uri.clone(), last_diff.uri.clone().into_static());
            }

            // Track doc_ref from the first root we process (they should all match)
            if merged_doc_ref.is_none() {
                merged_doc_ref = Some(pds_state.doc_ref.clone());
            }

            // Track "our" root/diff refs for sync state (used when syncing back)
            // We want to track our own edit.root so subsequent diffs go to the right place
            let is_our_root = current_did.as_ref().is_some_and(|did| root_did == *did);

            if is_our_root {
                // This is our own root - use it for sync state
                our_root_ref = Some(pds_state.root_ref);
                our_last_diff_ref = pds_state.last_diff_ref;
            } else if our_root_ref.is_none() {
                // We don't have our own root yet - use the first one we find
                // (this handles the case where we're a new collaborator with no edit state)
                our_root_ref = Some(pds_state.root_ref);
                our_last_diff_ref = pds_state.last_diff_ref;
            }
        }
    }

    // Export merged state as new snapshot
    let merged_snapshot = merged_doc.export(loro::ExportMode::Snapshot).map_err(|e| {
        WeaverError::InvalidNotebook(format!("Failed to export merged snapshot: {}", e))
    })?;

    tracing::debug!(
        "load_all_edit_states_from_pds: merged document, snapshot size = {} bytes",
        merged_snapshot.len()
    );

    // If we found any roots, return the merged state (includes updated last_seen map)
    // Note: our_root_ref might be from another collaborator if we haven't created our own yet
    Ok(our_root_ref.map(|root_ref| PdsEditState {
        root_ref,
        last_diff_ref: our_last_diff_ref,
        root_snapshot: merged_snapshot.into(),
        diff_updates: vec![], // Already merged into snapshot
        last_seen_diffs: updated_last_seen,
        doc_ref: merged_doc_ref.expect("Should have at least one doc_ref if we have a root"),
    }))
}

/// Internal helper to load edit state given a root record ID.
///
/// If `after_rkey` is provided, only diffs with rkey > after_rkey are fetched.
/// This enables incremental sync by skipping diffs we've already imported.
async fn load_edit_state_from_root_id(
    fetcher: &Fetcher,
    root_id: RecordId<'static>,
    after_rkey: Option<&str>,
) -> Result<Option<PdsEditState>, WeaverError> {
    // Build root URI
    let root_uri = AtUri::new(&format_smolstr!(
        "at://{}/{}/{}",
        root_id.did,
        ROOT_NSID,
        root_id.rkey.as_ref()
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

    // Extract the DocRef from the root record
    let doc_ref = root_output.value.doc.into_static();

    // Fetch the root snapshot blob
    let root_snapshot = fetch_blob(
        fetcher,
        &root_id.did,
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
            last_seen_diffs: std::collections::HashMap::new(),
            doc_ref,
        }));
    }

    // Fetch all diffs and store in BTreeMap keyed by rkey (TID) for sorted order
    // TIDs are lexicographically sortable timestamps
    let mut diffs_by_rkey: BTreeMap<
        CowStr<'static>,
        (Diff<'static>, Cid<'static>, AtUri<'static>),
    > = BTreeMap::new();

    for diff_id in &diff_ids {
        let rkey_str: &str = diff_id.rkey.as_ref();

        // Skip diffs we've already seen (rkey/TID is lexicographically sortable by time)
        if let Some(after) = after_rkey {
            if rkey_str <= after {
                tracing::trace!("Skipping already-seen diff rkey: {}", rkey_str);
                continue;
            }
        }

        let diff_uri = AtUri::new(&format_smolstr!(
            "at://{}/{}/{}",
            diff_id.did,
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

    // Fetch all diff data in TID order (BTreeMap iterates in sorted order)
    // Diffs can be stored either inline or as blobs
    let mut diff_updates = Vec::new();
    let mut last_diff_ref = None;

    for (_rkey, (diff, cid, uri)) in &diffs_by_rkey {
        // Check for inline diff first, then fall back to blob
        let diff_bytes = if let Some(ref inline) = diff.inline_diff {
            inline.clone()
        } else if let Some(ref snapshot) = diff.snapshot {
            fetch_blob(fetcher, &root_id.did, snapshot.blob().cid()).await?
        } else {
            tracing::warn!("Diff has neither inline_diff nor snapshot, skipping");
            continue;
        };

        diff_updates.push(diff_bytes);

        // Track the last diff (will be the one with highest TID after iteration)
        last_diff_ref = Some(StrongRef::new().uri(uri.clone()).cid(cid.clone()).build());
    }

    Ok(Some(PdsEditState {
        root_ref,
        last_diff_ref,
        root_snapshot,
        diff_updates,
        last_seen_diffs: std::collections::HashMap::new(),
        doc_ref,
    }))
}

/// Load document state by merging local storage and PDS state.
///
/// Loads from localStorage and PDS (if available), then merges both using Loro's
/// CRDT merge. The result is a pre-merged LoroDoc that can be converted to an
/// EditorDocument inside a reactive context using `use_hook`.
///
/// For unpublished drafts, attempts to discover edit state via Constellation
/// using the synthetic draft URI.
pub async fn load_and_merge_document(
    fetcher: &Fetcher,
    draft_key: &str,
    entry_uri: Option<&AtUri<'_>>,
) -> Result<Option<LoadedDocState>, WeaverError> {
    use super::storage::load_snapshot_from_storage;

    // Load snapshot + entry_ref from localStorage
    let local_data = load_snapshot_from_storage(draft_key);

    // Load from PDS - for entries use multi-repo loading (all collaborators),
    // for drafts use single-repo loading (draft sharing requires knowing the URI)
    let pds_state = if let Some(uri) = entry_uri {
        // Published entry: load from ALL collaborators (multi-repo CRDT merge)
        let empty_last_seen = std::collections::HashMap::new();
        load_all_edit_states_from_pds(fetcher, uri, &empty_last_seen).await?
    } else if let Some(did) = fetcher.current_did().await {
        // Unpublished draft: single-repo for now
        // (draft sharing would require collaborator to know the draft URI)
        let draft_uri = build_draft_uri(&did, draft_key);
        load_edit_state_from_draft(fetcher, &draft_uri).await?
    } else {
        // Not authenticated, can't query PDS
        None
    };

    // Extract owner identity from entry URI for blob cache warming
    let owner_ident: Option<String> = entry_uri.map(|uri| match uri.authority() {
        AtIdentifier::Did(d) => d.as_ref().to_string(),
        AtIdentifier::Handle(h) => h.as_ref().to_string(),
    });

    match (local_data, pds_state) {
        (None, None) => Ok(None),

        (Some(local), None) => {
            // Only local state exists - build LoroDoc from snapshot
            tracing::debug!("Loaded document from localStorage only");
            let doc = LoroDoc::new();
            if let Err(e) = doc.import(&local.snapshot) {
                tracing::warn!("Failed to import local snapshot: {:?}", e);
            }

            let resolved_content =
                prefetch_embeds_from_doc(&doc, fetcher, owner_ident.as_deref()).await;

            Ok(Some(LoadedDocState {
                doc,
                entry_ref: local.entry_ref, // Restored from localStorage
                edit_root: None,
                last_diff: None,
                synced_version: None, // Local-only, never synced to PDS
                last_seen_diffs: std::collections::HashMap::new(),
                resolved_content,
                notebook_uri: local.notebook_uri, // Restored from localStorage
            }))
        }

        (None, Some(pds)) => {
            // Only PDS state exists - reconstruct from snapshot + diffs
            tracing::debug!("Loaded document from PDS only");
            let doc = LoroDoc::new();

            // Import root snapshot
            if let Err(e) = doc.import(&pds.root_snapshot) {
                tracing::warn!("Failed to import PDS root snapshot: {:?}", e);
            }

            // Apply all diffs in order
            for updates in &pds.diff_updates {
                if let Err(e) = doc.import(updates) {
                    tracing::warn!("Failed to apply diff update: {:?}", e);
                }
            }

            // Capture the version after loading all PDS state - this is our sync baseline
            let synced_version = Some(doc.oplog_vv());

            // Reconstruct entry_ref from the DocRef stored in edit.root
            let entry_ref = doc_ref_to_entry_ref(fetcher, &pds.doc_ref).await;
            if entry_ref.is_some() {
                tracing::debug!("Reconstructed entry_ref from PDS DocRef");
            }

            let resolved_content =
                prefetch_embeds_from_doc(&doc, fetcher, owner_ident.as_deref()).await;

            Ok(Some(LoadedDocState {
                doc,
                entry_ref,
                edit_root: Some(pds.root_ref),
                last_diff: pds.last_diff_ref,
                synced_version, // Just loaded from PDS, fully synced
                last_seen_diffs: pds.last_seen_diffs,
                resolved_content,
                notebook_uri: None, // PDS-only, notebook context comes from target_notebook
            }))
        }

        (Some(local), Some(pds)) => {
            // Both exist - merge using CRDT
            tracing::debug!("Merging document from localStorage and PDS");

            // First, reconstruct the PDS state to get its version vector
            let pds_doc = LoroDoc::new();
            if let Err(e) = pds_doc.import(&pds.root_snapshot) {
                tracing::warn!("Failed to import PDS root snapshot for VV: {:?}", e);
            }
            for updates in &pds.diff_updates {
                if let Err(e) = pds_doc.import(updates) {
                    tracing::warn!("Failed to apply PDS diff for VV: {:?}", e);
                }
            }
            let pds_version = pds_doc.oplog_vv();

            // Now create the merged doc
            let doc = LoroDoc::new();

            // Import local snapshot first
            if let Err(e) = doc.import(&local.snapshot) {
                tracing::warn!("Failed to import local snapshot: {:?}", e);
            }

            // Import PDS root snapshot - Loro will merge
            if let Err(e) = doc.import(&pds.root_snapshot) {
                tracing::warn!("Failed to merge PDS root snapshot: {:?}", e);
            }

            // Import all diffs
            for updates in &pds.diff_updates {
                if let Err(e) = doc.import(updates) {
                    tracing::warn!("Failed to merge PDS diff: {:?}", e);
                }
            }

            // Use the PDS version as our sync baseline - any local changes
            // beyond this will be detected as unsynced
            let resolved_content =
                prefetch_embeds_from_doc(&doc, fetcher, owner_ident.as_deref()).await;

            Ok(Some(LoadedDocState {
                doc,
                entry_ref: local.entry_ref, // Restored from localStorage
                edit_root: Some(pds.root_ref),
                last_diff: pds.last_diff_ref,
                synced_version: Some(pds_version),
                last_seen_diffs: pds.last_seen_diffs,
                resolved_content,
                notebook_uri: local.notebook_uri, // Restored from localStorage
            }))
        }
    }
}

// ============================================================================
// Sync UI Components
// ============================================================================

use crate::auth::AuthState;
use dioxus::prelude::*;

/// Sync status states for UI display.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SyncState {
    /// All local changes have been synced to PDS
    Synced,
    /// Currently syncing to PDS
    Syncing,
    /// Has local changes not yet synced
    Unsynced,
    /// Remote collaborator changes available
    RemoteChanges,
    /// Last sync failed
    Error,
    /// Not authenticated or sync disabled
    Disabled,
}

/// Props for the SyncStatus component.
#[derive(Props, Clone, PartialEq)]
pub struct SyncStatusProps {
    /// The editor document to sync
    pub document: EditorDocument,
    /// Draft key for this document
    pub draft_key: String,
    /// Auto-sync interval in milliseconds (0 to disable, default disabled)
    #[props(default = 0)]
    pub auto_sync_interval_ms: u32,
    /// Callback to refresh/reload document from collaborators
    #[props(default)]
    pub on_refresh: Option<EventHandler<()>>,
    /// Whether this is a collaborative document (has collaborators)
    #[props(default = false)]
    pub is_collaborative: bool,
}

/// Sync status indicator with auto-sync functionality.
///
/// Displays the current sync state and automatically syncs to PDS periodically.
/// Initially shows "Start syncing" until user activates sync, then auto-syncs.
#[component]
pub fn SyncStatus(props: SyncStatusProps) -> Element {
    let fetcher = use_context::<Fetcher>();
    let auth_state = use_context::<Signal<AuthState>>();

    let doc = props.document.clone();
    let draft_key = props.draft_key.clone();

    // Sync activated - true if sync has been started (either manually or doc already has edit_root)
    // Once activated, auto-sync is enabled
    let mut sync_activated = use_signal(|| {
        // If document already has an edit_root, syncing is already active
        props.document.edit_root().is_some()
    });

    // Sync state management
    let mut sync_state = use_signal(|| {
        if props.document.has_unsynced_changes() {
            SyncState::Unsynced
        } else {
            SyncState::Synced
        }
    });
    let mut last_error: Signal<Option<String>> = use_signal(|| None);

    // Check if we're authenticated (drafts can sync via DraftRef even without entry)
    let is_authenticated = auth_state.read().is_authenticated();

    // Auto-sync trigger signal - set to true to trigger a sync
    let mut trigger_sync = use_signal(|| false);

    // Auto-sync timer - only triggers after sync has been activated
    {
        let doc_for_check = doc.clone();

        // Use 30s interval for auto-sync once activated
        dioxus_sdk::time::use_interval(std::time::Duration::from_secs(30), move |_| {
            // Only auto-sync if activated
            if !*sync_activated.peek() {
                return;
            }
            // Only trigger if there are unsynced changes
            if doc_for_check.has_unsynced_changes() {
                trigger_sync.set(true);
            }
        });
    }

    // Collaborator poll timer - checks for collaborator updates periodically
    // For collaborative documents, poll every 60s
    // - If user has been idle ≥30s: auto-trigger refresh
    // - If user is actively editing: show RemoteChanges state
    {
        let is_collaborative = props.is_collaborative;
        let on_refresh = props.on_refresh.clone();
        let doc_for_idle = doc.clone();

        dioxus_sdk::time::use_interval(std::time::Duration::from_secs(60), move |_| {
            if !is_collaborative {
                return;
            }

            let idle_threshold = std::time::Duration::from_secs(30);

            // Check time since last edit
            let is_idle = match doc_for_idle.last_edit() {
                Some(edit_info) => edit_info.timestamp.elapsed() >= idle_threshold,
                None => true, // No edits yet = idle
            };

            if is_idle {
                // User is idle - safe to auto-refresh
                if let Some(ref handler) = on_refresh {
                    handler.call(());
                }
            } else {
                // User is actively editing - show remote changes indicator
                sync_state.set(SyncState::RemoteChanges);
            }
        });
    }

    // Update sync state when document changes
    // Note: We use peek() to avoid creating a reactive dependency on sync_state
    let doc_for_effect = doc.clone();
    use_effect(move || {
        // Check for unsynced changes (reads last_edit signal for reactivity)
        let _edit = doc_for_effect.last_edit();

        // Use peek to avoid reactive loop
        let current_state = *sync_state.peek();
        if current_state != SyncState::Syncing {
            if doc_for_effect.has_unsynced_changes() && current_state != SyncState::Unsynced {
                sync_state.set(SyncState::Unsynced);
            }
        }
    });

    // Sync effect - watches trigger_sync and performs sync when triggered
    let doc_for_sync = doc.clone();
    let draft_key_for_sync = draft_key.clone();
    let fetcher_for_sync = fetcher.clone();

    let doc_for_check = doc.clone();
    use_effect(move || {
        // Read trigger to create reactive dependency
        let should_sync = *trigger_sync.read();

        if !should_sync {
            return;
        }

        // Reset trigger immediately
        trigger_sync.set(false);

        // Check if already syncing
        if *sync_state.peek() == SyncState::Syncing {
            return;
        }

        // Check if authenticated (drafts can sync too via DraftRef)
        if !is_authenticated {
            return;
        }

        // Check if there are actually changes to sync
        if !doc_for_check.has_unsynced_changes() {
            // Already synced, just update state
            sync_state.set(SyncState::Synced);
            return;
        }

        sync_state.set(SyncState::Syncing);

        let mut doc = doc_for_sync.clone();
        let draft_key = draft_key_for_sync.clone();
        let fetcher = fetcher_for_sync.clone();

        // Spawn the async work
        spawn(async move {
            match sync_to_pds(&fetcher, &mut doc, &draft_key).await {
                Ok(SyncResult::NoChanges) => {
                    // No changes to sync - already up to date
                    sync_state.set(SyncState::Synced);
                    last_error.set(None);
                    tracing::debug!("No changes to sync");
                }
                Ok(_) => {
                    sync_state.set(SyncState::Synced);
                    last_error.set(None);
                    // Activate auto-sync after first successful sync
                    if !*sync_activated.peek() {
                        sync_activated.set(true);
                        tracing::debug!("Sync activated - auto-sync enabled");
                    }
                    tracing::debug!("Sync completed successfully");
                }
                Err(e) => {
                    sync_state.set(SyncState::Error);
                    last_error.set(Some(e.to_string()));
                    tracing::warn!("Sync failed: {}", e);
                }
            }
        });
    });

    // Determine display state (drafts can sync too via DraftRef)
    let is_activated = *sync_activated.read();
    let display_state = if !is_authenticated {
        SyncState::Disabled
    } else {
        *sync_state.read()
    };

    // Before activation: show "Start syncing" button
    // After activation: show normal sync states
    let (icon, label, class) = if !is_activated && is_authenticated {
        ("▶", "Start syncing", "sync-status start-sync")
    } else {
        match display_state {
            SyncState::Synced => ("✓", "Synced", "sync-status synced"),
            SyncState::Syncing => ("◌", "Syncing...", "sync-status syncing"),
            SyncState::Unsynced => ("●", "Unsynced", "sync-status unsynced"),
            SyncState::RemoteChanges => ("↓", "Updates", "sync-status remote-changes"),
            SyncState::Error => ("✕", "Sync error", "sync-status error"),
            SyncState::Disabled => ("○", "Sync disabled", "sync-status disabled"),
        }
    };

    // Long-press detection for deactivating sync
    let mut long_press_active = use_signal(|| false);
    #[cfg(target_arch = "wasm32")]
    let mut long_press_timeout: Signal<Option<gloo_timers::callback::Timeout>> =
        use_signal(|| None);

    let on_pointer_down = move |_: dioxus::events::PointerEvent| {
        // Only allow deactivation if sync is currently activated
        if !*sync_activated.peek() {
            return;
        }

        long_press_active.set(true);

        // Start 1 second timer for long press
        #[cfg(target_arch = "wasm32")]
        let timeout = gloo_timers::callback::Timeout::new(1000, move || {
            if *long_press_active.peek() {
                sync_activated.set(false);
                long_press_active.set(false);
                tracing::debug!("Sync deactivated via long press");
            }
        });
        #[cfg(target_arch = "wasm32")]
        long_press_timeout.set(Some(timeout));
    };

    let on_pointer_up = move |_: dioxus::events::PointerEvent| {
        long_press_active.set(false);
        // Cancel the timeout by dropping it
        #[cfg(target_arch = "wasm32")]
        long_press_timeout.set(None);
    };

    let on_pointer_leave = move |_: dioxus::events::PointerEvent| {
        long_press_active.set(false);
        #[cfg(target_arch = "wasm32")]
        long_press_timeout.set(None);
    };

    // Combined sync handler - pulls remote changes first if needed, then pushes local
    let doc_for_sync = doc.clone();
    let on_sync_click = {
        let on_refresh = props.on_refresh.clone();
        let current_state = display_state;
        move |_: dioxus::events::MouseEvent| {
            // Don't trigger click if long press just fired
            if !*sync_activated.peek() && *long_press_active.peek() {
                return;
            }

            if *sync_state.peek() == SyncState::Syncing {
                return; // Already syncing
            }
            // If there are remote changes, pull them first
            if current_state == SyncState::RemoteChanges {
                if let Some(ref handler) = on_refresh {
                    handler.call(());
                }
            }
            // Trigger sync if there are local changes
            if doc_for_sync.has_unsynced_changes() {
                trigger_sync.set(true);
            } else if current_state != SyncState::RemoteChanges {
                sync_state.set(SyncState::Synced);
            }
        }
    };

    // Show tooltip hint about long-press when sync is active
    let title = if is_activated {
        if let Some(ref err) = *last_error.read() {
            err.clone()
        } else {
            format!("{} (hold to stop syncing)", label)
        }
    } else {
        label.to_string()
    };

    rsx! {
        div {
            class: "{class}",
            title: "{title}",
            onclick: on_sync_click,
            onpointerdown: on_pointer_down,
            onpointerup: on_pointer_up,
            onpointerleave: on_pointer_leave,

            span { class: "sync-icon", "{icon}" }
            span { class: "sync-label", "{label}" }
        }
    }
}
