//! Markdown editor component with Obsidian-style formatting visibility.
//!
//! This module implements a WYSIWYG-like markdown editor where formatting
//! characters are hidden contextually based on cursor position, while still
//! editing plain markdown text under the hood.

mod actions;
mod beforeinput;
mod collab;
mod component;
mod document;
mod dom_sync;
mod image_upload;
mod input;
mod log_buffer;
mod publish;
mod report;
mod storage;
mod sync;
mod toolbar;

#[cfg(test)]
mod tests;

// Re-export DOM update strategy constant from browser crate.
pub(crate) use weaver_editor_browser::FORCE_INNERHTML_UPDATE;

// Main component
pub use component::MarkdownEditor;

// Document types
#[allow(unused_imports)]
pub use document::{
    Affinity, CompositionState, CursorState, LoadedDocState, Selection, SignalEditorDocument,
};

// Formatting - re-export from core
#[allow(unused_imports)]
pub use weaver_editor_core::{FormatAction, apply_formatting};

// Rendering - re-export core types
#[allow(unused_imports)]
pub use weaver_editor_core::{
    EditorImageResolver, EditorRope, EditorWriter, EmbedContentProvider, ImageResolver,
    OffsetMapping, ParagraphRender, RenderCache, RenderResult, SegmentedWriter, SyntaxSpanInfo,
    SyntaxType, TextBuffer, WriterResult, find_mapping_for_byte, render_paragraphs_incremental,
};

// Storage
#[allow(unused_imports)]
pub use storage::{
    DRAFT_KEY_PREFIX, EditorSnapshot, clear_all_drafts, delete_draft, delete_draft_from_pds,
    list_drafts, load_from_storage, load_snapshot_from_storage, save_to_storage,
};

// Sync
#[allow(unused_imports)]
pub use sync::{
    PdsEditState, RemoteDraft, SyncState, SyncStatus, list_drafts_from_pds,
    load_and_merge_document, load_edit_state_from_pds, sync_to_pds,
};

// UI components
pub use image_upload::{ImageUploadButton, UploadedImage};
pub use publish::PublishButton;
pub use report::ReportButton;
#[allow(unused_imports)]
pub use toolbar::EditorToolbar;

// Visibility
#[allow(unused_imports)]
pub use weaver_editor_core::VisibilityState;

// Logging
#[allow(unused_imports)]
pub use log_buffer::LogCaptureLayer;

// Worker types from weaver-editor-crdt
#[cfg(all(target_family = "wasm", target_os = "unknown"))]
pub use weaver_editor_crdt::{EditorReactor, WorkerInput, WorkerOutput};
// Embed worker from weaver-embed-worker
#[cfg(all(target_family = "wasm", target_os = "unknown"))]
pub use weaver_embed_worker::{EmbedWorker, EmbedWorkerInput, EmbedWorkerOutput};

// Collab coordinator
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub use collab::CollabCoordinator;
