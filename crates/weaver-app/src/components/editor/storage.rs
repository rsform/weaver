//! LocalStorage persistence for the editor.
//!
//! Only available on WASM targets.

#[cfg(all(target_family = "wasm", target_os = "unknown"))]
use gloo_storage::{LocalStorage, Storage};
use serde::{Deserialize, Serialize};

/// Editor snapshot for persistence.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct EditorSnapshot {
    pub content: String,
    pub cursor_offset: usize,
}

#[cfg(all(target_family = "wasm", target_os = "unknown"))]
const STORAGE_KEY: &str = "weaver_editor_draft";

/// Save editor state to LocalStorage (WASM only).
#[cfg(all(target_family = "wasm", target_os = "unknown"))]
pub fn save_to_storage(
    content: &str,
    cursor_offset: usize,
) -> Result<(), gloo_storage::errors::StorageError> {
    let snapshot = EditorSnapshot {
        content: content.to_string(),
        cursor_offset,
    };
    LocalStorage::set(STORAGE_KEY, &snapshot)
}

/// Load editor state from LocalStorage (WASM only).
#[cfg(all(target_family = "wasm", target_os = "unknown"))]
pub fn load_from_storage() -> Option<EditorSnapshot> {
    LocalStorage::get(STORAGE_KEY).ok()
}

/// Clear editor state from LocalStorage (WASM only).
#[cfg(all(target_family = "wasm", target_os = "unknown"))]
#[allow(dead_code)]
pub fn clear_storage() {
    LocalStorage::delete(STORAGE_KEY);
}

// Stub implementations for non-WASM targets
#[cfg(not(all(target_family = "wasm", target_os = "unknown")))]
pub fn save_to_storage(_content: &str, _cursor_offset: usize) -> Result<(), String> {
    Ok(())
}

#[cfg(not(all(target_family = "wasm", target_os = "unknown")))]
pub fn load_from_storage() -> Option<EditorSnapshot> {
    None
}

#[cfg(not(all(target_family = "wasm", target_os = "unknown")))]
#[allow(dead_code)]
pub fn clear_storage() {}
