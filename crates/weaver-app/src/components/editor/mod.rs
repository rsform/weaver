//! Markdown editor component with Obsidian-style formatting visibility.
//!
//! This module implements a WYSIWYG-like markdown editor where formatting
//! characters are hidden contextually based on cursor position, while still
//! editing plain markdown text under the hood.

mod actions;
mod beforeinput;
mod collab;
mod component;
mod cursor;
mod document;
mod dom_sync;
mod formatting;
mod image_upload;
mod input;
mod log_buffer;
mod paragraph;
mod platform;
mod publish;
mod render;
mod report;
mod storage;
mod sync;
mod toolbar;
mod visibility;
mod writer;

#[cfg(test)]
mod tests;

/// When true, always update innerHTML even for cursor paragraph during typing.
/// This ensures syntax/formatting changes are immediately visible, but requires
/// using `Handled` (preventDefault) for InsertText to avoid double-insertion
/// from browser's default action racing with our innerHTML update.
///
/// TODO: Replace with granular detection of syntax/formatting changes to allow
/// PassThrough optimization when only text content changes.
pub(crate) const FORCE_INNERHTML_UPDATE: bool = true;

// Main component
pub use component::MarkdownEditor;

// Document types
#[allow(unused_imports)]
pub use document::{
    Affinity, CompositionState, CursorState, EditorDocument, LoadedDocState, Selection,
};

// Formatting
#[allow(unused_imports)]
pub use formatting::{FormatAction, apply_formatting, find_word_boundaries};

// Rendering - re-export core types
#[allow(unused_imports)]
pub use weaver_editor_core::{
    EditorRope, EditorWriter, EmbedContentProvider, ImageResolver, OffsetMapping, RenderResult,
    SegmentedWriter, SyntaxSpanInfo, SyntaxType, TextBuffer, WriterResult, find_mapping_for_byte,
};
#[allow(unused_imports)]
pub use paragraph::ParagraphRender;
#[allow(unused_imports)]
pub use render::{RenderCache, render_paragraphs_incremental};
// App-specific image resolver
#[allow(unused_imports)]
pub use writer::embed::EditorImageResolver;

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
pub use visibility::VisibilityState;

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
