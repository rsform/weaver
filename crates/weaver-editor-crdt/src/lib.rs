//! CRDT-backed editor with AT Protocol sync.
//!
//! This crate provides:
//! - `LoroTextBuffer`: Loro-backed text buffer implementing `TextBuffer` + `UndoManager`
//! - `CrdtDocument`: Trait for documents that can sync to AT Protocol PDS
//! - Generic sync logic for edit records (root/diff/draft)
//! - Worker implementation for off-main-thread CRDT operations
//! - Collab coordination types and helpers

mod buffer;
mod coordinator;
mod document;
mod error;
mod sync;

pub mod worker;

pub use buffer::LoroTextBuffer;
pub use coordinator::{
    CoordinatorState, PEER_DISCOVERY_INTERVAL_MS, SESSION_REFRESH_INTERVAL_MS, SESSION_TTL_MINUTES,
    compute_collab_topic,
};
pub use document::{CrdtDocument, SimpleCrdtDocument, SyncState};
pub use error::CrdtError;
pub use sync::{
    CreateRootResult, PdsEditState, RemoteDraft, SyncResult,
    build_draft_uri, create_diff, create_edit_root,
    find_all_edit_roots, find_diffs_for_root, find_edit_root_for_draft,
    list_drafts, load_all_edit_states, load_edit_state_from_draft,
    load_edit_state_from_entry, sync_to_pds,
};

// Re-export worker types
pub use worker::{WorkerInput, WorkerOutput};
#[cfg(all(target_family = "wasm", target_os = "unknown"))]
pub use worker::EditorReactor;

// Re-export Loro types that consumers need
pub use loro::{ExportMode, LoroDoc, LoroText, VersionVector};
