//! LocalStorage persistence for the editor.
//!
//! Stores both human-readable content (for debugging) and the full CRDT
//! snapshot (for undo history preservation across sessions).
//!
//! ## Storage key strategy (localStorage)
//!
//! - New entries: `"new:{tid}"` where tid is a timestamp-based ID
//! - Editing existing: `"{at-uri}"` the full AT-URI of the entry
//!
//! ## PDS canonical format
//!
//! When syncing to PDS via DraftRef, keys are transformed to canonical
//! format: `"{did}:{rkey}"` for discoverability and topic derivation.
//! This transformation happens in sync.rs `build_doc_ref()`.

#[cfg(all(target_family = "wasm", target_os = "unknown"))]
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use dioxus::prelude::*;
#[cfg(all(target_family = "wasm", target_os = "unknown"))]
use gloo_storage::{LocalStorage, Storage};
use jacquard::IntoStatic;
use jacquard::smol_str::{SmolStr, ToSmolStr};
use jacquard::types::string::{AtUri, Cid};
use loro::cursor::Cursor;
use serde::{Deserialize, Serialize};
use weaver_api::com_atproto::repo::strong_ref::StrongRef;

use super::document::EditorDocument;

/// Prefix for all draft storage keys.
pub const DRAFT_KEY_PREFIX: &str = "weaver_draft:";

/// Editor snapshot for persistence.
///
/// Stores both human-readable content and CRDT snapshot for best of both worlds:
/// - `content`: Human-readable text for debugging
/// - `title`: Entry title for debugging/display in drafts list
/// - `snapshot`: Base64-encoded CRDT state for document history (includes all embeds)
/// - `cursor`: Loro Cursor (serialized as JSON) for stable cursor position
/// - `cursor_offset`: Fallback cursor position if Loro cursor can't be restored
/// - `editing_uri`: AT-URI if editing an existing entry
///
/// Note: Undo/redo is session-only (UndoManager state is ephemeral).
/// For cross-session "undo", use time travel via `doc.checkout(frontiers)`.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct EditorSnapshot {
    /// Human-readable document content (for debugging/fallback)
    pub content: String,

    /// Entry title (for debugging/display in drafts list)
    #[serde(default)]
    pub title: SmolStr,

    /// Base64-encoded CRDT snapshot (contains ALL fields including embeds)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<String>,

    /// Loro Cursor for stable cursor position tracking
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<Cursor>,

    /// Fallback cursor offset (used if Loro cursor can't be restored)
    #[serde(default)]
    pub cursor_offset: usize,

    /// AT-URI if editing an existing entry (None for new entries)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub editing_uri: Option<SmolStr>,

    /// CID of the entry if editing an existing entry
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub editing_cid: Option<SmolStr>,

    /// AT-URI of the notebook this draft belongs to (for re-publishing)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notebook_uri: Option<SmolStr>,
}

/// Build the full storage key from a draft key.
fn storage_key(key: &str) -> String {
    format!("{}{}", DRAFT_KEY_PREFIX, key)
}

/// Save editor state to LocalStorage (WASM only).
#[cfg(all(target_family = "wasm", target_os = "unknown"))]
pub fn save_to_storage(
    doc: &EditorDocument,
    key: &str,
) -> Result<(), gloo_storage::errors::StorageError> {
    let export_start = crate::perf::now();
    let snapshot_bytes = doc.export_snapshot();
    let export_ms = crate::perf::now() - export_start;

    let encode_start = crate::perf::now();
    let snapshot_b64 = if snapshot_bytes.is_empty() {
        None
    } else {
        Some(BASE64.encode(&snapshot_bytes))
    };
    let encode_ms = crate::perf::now() - encode_start;

    let snapshot = EditorSnapshot {
        content: doc.content(),
        title: doc.title().into(),
        snapshot: snapshot_b64,
        cursor: doc.loro_cursor().cloned(),
        cursor_offset: doc.cursor.read().offset,
        editing_uri: doc.entry_ref().map(|r| r.uri.to_smolstr()),
        editing_cid: doc.entry_ref().map(|r| r.cid.to_smolstr()),
        notebook_uri: doc.notebook_uri(),
    };

    let write_start = crate::perf::now();
    let result = LocalStorage::set(storage_key(key), &snapshot);
    let write_ms = crate::perf::now() - write_start;

    tracing::debug!(
        export_ms,
        encode_ms,
        write_ms,
        bytes = snapshot_bytes.len(),
        "save_to_storage timing"
    );

    result
}

/// Load editor state from LocalStorage (WASM only).
///
/// Returns an EditorDocument restored from CRDT snapshot if available,
/// otherwise falls back to just the text content.
#[cfg(all(target_family = "wasm", target_os = "unknown"))]
pub fn load_from_storage(key: &str) -> Option<EditorDocument> {
    let snapshot: EditorSnapshot = LocalStorage::get(storage_key(key)).ok()?;

    // Parse entry_ref from the snapshot (requires both URI and CID)
    let entry_ref = snapshot
        .editing_uri
        .as_ref()
        .zip(snapshot.editing_cid.as_ref())
        .and_then(|(uri_str, cid_str)| {
            let uri = AtUri::new(uri_str).ok()?.into_static();
            let cid = Cid::new(cid_str.as_bytes()).ok()?.into_static();
            Some(StrongRef::new().uri(uri).cid(cid).build())
        });

    // Try to restore from CRDT snapshot first
    if let Some(ref snapshot_b64) = snapshot.snapshot {
        if let Ok(snapshot_bytes) = BASE64.decode(snapshot_b64) {
            let mut doc = EditorDocument::from_snapshot(
                &snapshot_bytes,
                snapshot.cursor.clone(),
                snapshot.cursor_offset,
            );
            // Verify the content matches (sanity check)
            if doc.content() == snapshot.content {
                doc.set_entry_ref(entry_ref.clone());
                if let Some(notebook_uri) = snapshot.notebook_uri {
                    doc.set_notebook_uri(Some(notebook_uri));
                }
                return Some(doc);
            }
            tracing::warn!("Snapshot content mismatch, falling back to text content");
        }
    }

    // Fallback: create new doc from text content
    let mut doc = EditorDocument::new(snapshot.content);
    doc.cursor.write().offset = snapshot.cursor_offset.min(doc.len_chars());
    doc.sync_loro_cursor();
    doc.set_entry_ref(entry_ref);
    if let Some(notebook_uri) = snapshot.notebook_uri {
        doc.set_notebook_uri(Some(notebook_uri));
    }
    Some(doc)
}

/// Data loaded from localStorage snapshot.
pub struct LocalSnapshotData {
    /// The raw CRDT snapshot bytes
    pub snapshot: Vec<u8>,
    /// Entry StrongRef if editing an existing entry
    pub entry_ref: Option<StrongRef<'static>>,
    /// Notebook URI for re-publishing
    pub notebook_uri: Option<SmolStr>,
}

/// Load snapshot data from LocalStorage (WASM only).
///
/// Unlike `load_from_storage`, this doesn't create an EditorDocument and is safe
/// to call outside of reactive context. Use with `load_and_merge_document`.
#[cfg(all(target_family = "wasm", target_os = "unknown"))]
pub fn load_snapshot_from_storage(key: &str) -> Option<LocalSnapshotData> {
    let snapshot: EditorSnapshot = LocalStorage::get(storage_key(key)).ok()?;

    // Try to get CRDT snapshot bytes
    let snapshot_bytes = snapshot
        .snapshot
        .as_ref()
        .and_then(|b64| BASE64.decode(b64).ok())?;

    // Try to reconstruct entry_ref from stored URI + CID
    let entry_ref = snapshot
        .editing_uri
        .as_ref()
        .zip(snapshot.editing_cid.as_ref())
        .and_then(|(uri_str, cid_str)| {
            let uri = AtUri::new(uri_str).ok()?.into_static();
            let cid = Cid::new(cid_str.as_bytes()).ok()?.into_static();
            Some(StrongRef::new().uri(uri).cid(cid).build())
        });

    Some(LocalSnapshotData {
        snapshot: snapshot_bytes,
        entry_ref,
        notebook_uri: snapshot.notebook_uri,
    })
}

/// Load snapshot data from LocalStorage (non-WASM stub).
#[cfg(not(all(target_family = "wasm", target_os = "unknown")))]
pub fn load_snapshot_from_storage(_key: &str) -> Option<LocalSnapshotData> {
    None
}

/// Delete a draft from LocalStorage (WASM only).
#[cfg(all(target_family = "wasm", target_os = "unknown"))]
pub fn delete_draft(key: &str) {
    LocalStorage::delete(storage_key(key));
}

/// List all draft keys from LocalStorage (WASM only).
///
/// Returns a list of (key, title, editing_uri) tuples for all saved drafts.
#[cfg(all(target_family = "wasm", target_os = "unknown"))]
pub fn list_drafts() -> Vec<(String, String, Option<String>)> {
    let mut drafts = Vec::new();

    // gloo_storage doesn't have a direct way to iterate keys,
    // so we use web_sys directly
    if let Some(storage) = web_sys::window()
        .and_then(|w| w.local_storage().ok())
        .flatten()
    {
        let len = storage.length().unwrap_or(0);
        for i in 0..len {
            if let Ok(Some(key)) = storage.key(i) {
                if key.starts_with(DRAFT_KEY_PREFIX) {
                    // Try to load just the metadata
                    if let Ok(snapshot) = LocalStorage::get::<EditorSnapshot>(&key) {
                        let draft_key = key.strip_prefix(DRAFT_KEY_PREFIX).unwrap_or(&key);
                        drafts.push((
                            draft_key.to_string(),
                            snapshot.title.to_string(),
                            snapshot.editing_uri.map(|s| s.to_string()),
                        ));
                    }
                }
            }
        }
    }

    drafts
}

/// Delete a draft stub record from PDS.
///
/// This deletes the sh.weaver.edit.draft record, making the draft
/// invisible in listDrafts. Edit history (edit.root, edit.diff) is
/// preserved for potential recovery.
#[cfg(all(target_family = "wasm", target_os = "unknown"))]
pub async fn delete_draft_from_pds(
    fetcher: &crate::fetch::Fetcher,
    draft_key: &str,
) -> Result<(), weaver_common::WeaverError> {
    use jacquard::prelude::XrpcClient;
    use jacquard::types::ident::AtIdentifier;
    use jacquard::types::recordkey::RecordKey;
    use jacquard::types::string::Nsid;
    use weaver_api::com_atproto::repo::delete_record::DeleteRecord;

    // Only delete if authenticated
    let Some(did) = fetcher.current_did().await else {
        tracing::debug!("Not authenticated, skipping PDS draft deletion");
        return Ok(());
    };

    // Extract rkey from draft_key
    let rkey_str = if let Some(tid) = draft_key.strip_prefix("new:") {
        tid.to_string()
    } else if draft_key.starts_with("at://") {
        draft_key.split('/').last().unwrap_or(draft_key).to_string()
    } else {
        draft_key.to_string()
    };

    let rkey = RecordKey::any(&rkey_str)
        .map_err(|e| weaver_common::WeaverError::InvalidNotebook(e.to_string()))?;

    // Build the delete request
    let request = DeleteRecord::new()
        .repo(AtIdentifier::Did(did))
        .collection(Nsid::new_static("sh.weaver.edit.draft").unwrap())
        .rkey(rkey)
        .build();

    // Execute deletion
    let client = fetcher.get_client();
    match client.send(request).await {
        Ok(_) => {
            tracing::info!("Deleted draft stub from PDS: {}", draft_key);
            Ok(())
        }
        Err(e) => {
            // Log but don't fail - draft may not exist on PDS
            tracing::warn!("Failed to delete draft from PDS (may not exist): {}", e);
            Ok(())
        }
    }
}

/// Non-WASM stub for delete_draft_from_pds
#[cfg(not(all(target_family = "wasm", target_os = "unknown")))]
pub async fn delete_draft_from_pds(
    _fetcher: &crate::fetch::Fetcher,
    _draft_key: &str,
) -> Result<(), weaver_common::WeaverError> {
    Ok(())
}

/// Clear all editor drafts from LocalStorage (WASM only).
#[cfg(all(target_family = "wasm", target_os = "unknown"))]
#[allow(dead_code)]
pub fn clear_all_drafts() {
    for (key, _, _) in list_drafts() {
        delete_draft(&key);
    }
}

// Stub implementations for non-WASM targets
#[cfg(not(all(target_family = "wasm", target_os = "unknown")))]
pub fn save_to_storage(_doc: &EditorDocument, _key: &str) -> Result<(), String> {
    Ok(())
}

#[cfg(not(all(target_family = "wasm", target_os = "unknown")))]
pub fn load_from_storage(_key: &str) -> Option<EditorDocument> {
    None
}

#[cfg(not(all(target_family = "wasm", target_os = "unknown")))]
pub fn delete_draft(_key: &str) {}

#[cfg(not(all(target_family = "wasm", target_os = "unknown")))]
pub fn list_drafts() -> Vec<(String, String, Option<String>)> {
    Vec::new()
}

#[cfg(not(all(target_family = "wasm", target_os = "unknown")))]
#[allow(dead_code)]
pub fn clear_all_drafts() {}
