//! LocalStorage persistence for the editor.
//!
//! Stores both human-readable content (for debugging) and the full CRDT
//! snapshot (for undo history preservation across sessions).
//!
//! Storage key strategy:
//! - New entries: `"draft:new:{uuid}"`
//! - Editing existing: `"draft:{at-uri}"`

#[cfg(all(target_family = "wasm", target_os = "unknown"))]
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use dioxus::prelude::*;
#[cfg(all(target_family = "wasm", target_os = "unknown"))]
use gloo_storage::{LocalStorage, Storage};
use jacquard::IntoStatic;
use jacquard::types::string::AtUri;
use loro::cursor::Cursor;
use serde::{Deserialize, Serialize};

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
    pub title: String,

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
    pub editing_uri: Option<String>,
}

/// Build the full storage key from a draft key.
fn storage_key(key: &str) -> String {
    format!("{}{}", DRAFT_KEY_PREFIX, key)
}

/// Save editor state to LocalStorage (WASM only).
///
/// # Arguments
/// * `doc` - The editor document to save
/// * `key` - Storage key (e.g., "new:abc123" for new entries, or AT-URI for existing)
#[cfg(all(target_family = "wasm", target_os = "unknown"))]
pub fn save_to_storage(
    doc: &EditorDocument,
    key: &str,
) -> Result<(), gloo_storage::errors::StorageError> {
    let snapshot_bytes = doc.export_snapshot();
    let snapshot_b64 = if snapshot_bytes.is_empty() {
        None
    } else {
        Some(BASE64.encode(&snapshot_bytes))
    };

    let snapshot = EditorSnapshot {
        content: doc.content(),
        title: doc.title(),
        snapshot: snapshot_b64,
        cursor: doc.loro_cursor().cloned(),
        cursor_offset: doc.cursor.read().offset,
        editing_uri: doc.entry_uri().map(|u| u.to_string()),
    };
    LocalStorage::set(storage_key(key), &snapshot)
}

/// Load editor state from LocalStorage (WASM only).
///
/// Returns an EditorDocument restored from CRDT snapshot if available,
/// otherwise falls back to just the text content.
///
/// # Arguments
/// * `key` - Storage key (e.g., "new:abc123" for new entries, or AT-URI for existing)
#[cfg(all(target_family = "wasm", target_os = "unknown"))]
pub fn load_from_storage(key: &str) -> Option<EditorDocument> {
    let snapshot: EditorSnapshot = LocalStorage::get(storage_key(key)).ok()?;

    // Parse entry_uri from the snapshot
    let entry_uri = snapshot
        .editing_uri
        .as_ref()
        .and_then(|s| AtUri::new(s).ok())
        .map(|u| u.into_static());

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
                doc.set_entry_uri(entry_uri);
                return Some(doc);
            }
            tracing::warn!("Snapshot content mismatch, falling back to text content");
        }
    }

    // Fallback: create new doc from text content
    let mut doc = EditorDocument::new(snapshot.content);
    doc.cursor.write().offset = snapshot.cursor_offset.min(doc.len_chars());
    doc.sync_loro_cursor();
    doc.set_entry_uri(entry_uri);
    Some(doc)
}

/// Delete a draft from LocalStorage (WASM only).
///
/// # Arguments
/// * `key` - Storage key to delete
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
                        drafts.push((draft_key.to_string(), snapshot.title, snapshot.editing_uri));
                    }
                }
            }
        }
    }

    drafts
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
