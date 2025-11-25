//! LocalStorage persistence for the editor.
//!
//! Stores both human-readable content (for debugging) and the full CRDT
//! snapshot (for undo history preservation across sessions).

#[cfg(all(target_family = "wasm", target_os = "unknown"))]
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
#[cfg(all(target_family = "wasm", target_os = "unknown"))]
use gloo_storage::{LocalStorage, Storage};
use loro::cursor::Cursor;
use serde::{Deserialize, Serialize};

use super::document::EditorDocument;

/// Editor snapshot for persistence.
///
/// Stores both human-readable content and CRDT snapshot for best of both worlds:
/// - `content`: Human-readable text for debugging
/// - `snapshot`: Base64-encoded CRDT state for document history
/// - `cursor`: Loro Cursor (serialized as JSON) for stable cursor position
/// - `cursor_offset`: Fallback cursor position if Loro cursor can't be restored
///
/// Note: Undo/redo is session-only (UndoManager state is ephemeral).
/// For cross-session "undo", use time travel via `doc.checkout(frontiers)`.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct EditorSnapshot {
    /// Human-readable document content (for debugging/fallback)
    pub content: String,
    /// Base64-encoded CRDT snapshot
    pub snapshot: Option<String>,
    /// Loro Cursor for stable cursor position tracking
    pub cursor: Option<Cursor>,
    /// Fallback cursor offset (used if Loro cursor can't be restored)
    pub cursor_offset: usize,
}

#[cfg(all(target_family = "wasm", target_os = "unknown"))]
const STORAGE_KEY: &str = "weaver_editor_draft";

/// Save editor state to LocalStorage (WASM only).
#[cfg(all(target_family = "wasm", target_os = "unknown"))]
pub fn save_to_storage(doc: &EditorDocument) -> Result<(), gloo_storage::errors::StorageError> {
    let snapshot_bytes = doc.export_snapshot();
    let snapshot_b64 = if snapshot_bytes.is_empty() {
        None
    } else {
        Some(BASE64.encode(&snapshot_bytes))
    };

    let snapshot = EditorSnapshot {
        content: doc.to_string(),
        snapshot: snapshot_b64,
        cursor: doc.loro_cursor().cloned(),
        cursor_offset: doc.cursor.offset,
    };
    LocalStorage::set(STORAGE_KEY, &snapshot)
}

/// Load editor state from LocalStorage (WASM only).
/// Returns an EditorDocument restored from CRDT snapshot if available,
/// otherwise falls back to just the text content.
#[cfg(all(target_family = "wasm", target_os = "unknown"))]
pub fn load_from_storage() -> Option<EditorDocument> {
    let snapshot: EditorSnapshot = LocalStorage::get(STORAGE_KEY).ok()?;

    // Try to restore from CRDT snapshot first
    if let Some(ref snapshot_b64) = snapshot.snapshot {
        if let Ok(snapshot_bytes) = BASE64.decode(snapshot_b64) {
            let doc = EditorDocument::from_snapshot(
                &snapshot_bytes,
                snapshot.cursor.clone(),
                snapshot.cursor_offset,
            );
            // Verify the content matches (sanity check)
            if doc.to_string() == snapshot.content {
                return Some(doc);
            }
            tracing::warn!("Snapshot content mismatch, falling back to text content");
        }
    }

    // Fallback: create new doc from text content
    let mut doc = EditorDocument::new(snapshot.content);
    doc.cursor.offset = snapshot.cursor_offset.min(doc.len_chars());
    doc.sync_loro_cursor();
    Some(doc)
}

/// Clear editor state from LocalStorage (WASM only).
#[cfg(all(target_family = "wasm", target_os = "unknown"))]
#[allow(dead_code)]
pub fn clear_storage() {
    LocalStorage::delete(STORAGE_KEY);
}

// Stub implementations for non-WASM targets
#[cfg(not(all(target_family = "wasm", target_os = "unknown")))]
pub fn save_to_storage(_doc: &EditorDocument) -> Result<(), String> {
    Ok(())
}

#[cfg(not(all(target_family = "wasm", target_os = "unknown")))]
pub fn load_from_storage() -> Option<EditorDocument> {
    None
}

#[cfg(not(all(target_family = "wasm", target_os = "unknown")))]
#[allow(dead_code)]
pub fn clear_storage() {}
