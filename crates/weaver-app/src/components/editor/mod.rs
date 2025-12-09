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
mod offset_map;
mod paragraph;
mod platform;
mod publish;
mod render;
mod report;
mod storage;
mod sync;
mod toolbar;
mod visibility;
#[cfg(all(target_family = "wasm", target_os = "unknown"))]
mod worker;
mod writer;

#[cfg(test)]
mod tests;

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

// Rendering
#[allow(unused_imports)]
pub use offset_map::{OffsetMapping, RenderResult, find_mapping_for_byte};
#[allow(unused_imports)]
pub use paragraph::ParagraphRender;
#[allow(unused_imports)]
pub use render::{RenderCache, render_paragraphs_incremental};
#[allow(unused_imports)]
pub use writer::{EditorImageResolver, ImageResolver, SyntaxSpanInfo, SyntaxType, WriterResult};

// Storage
#[allow(unused_imports)]
pub use storage::{
    DRAFT_KEY_PREFIX, EditorSnapshot, clear_all_drafts, delete_draft, list_drafts,
    load_from_storage, load_snapshot_from_storage, save_to_storage,
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

// Worker
#[cfg(all(target_family = "wasm", target_os = "unknown"))]
pub use worker::{
    EditorReactor, EmbedWorker, EmbedWorkerInput, EmbedWorkerOutput, WorkerInput, WorkerOutput,
};

// Collab coordinator
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub use collab::CollabCoordinator;
