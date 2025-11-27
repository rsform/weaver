//! Markdown editor component with Obsidian-style formatting visibility.
//!
//! This module implements a WYSIWYG-like markdown editor where formatting
//! characters are hidden contextually based on cursor position, while still
//! editing plain markdown text under the hood.

mod component;
mod cursor;
mod document;
mod dom_sync;
mod formatting;
mod input;
mod log_buffer;
mod offset_map;
mod paragraph;
mod platform;
mod publish;
mod render;
mod report;
mod storage;
mod toolbar;
mod visibility;
mod writer;

#[cfg(test)]
mod tests;

// Main component
pub use component::MarkdownEditor;

// Document types
#[allow(unused_imports)]
pub use document::{Affinity, CompositionState, CursorState, EditorDocument, Selection};

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
pub use writer::{SyntaxSpanInfo, SyntaxType, WriterResult};

// Storage
#[allow(unused_imports)]
pub use storage::{
    DRAFT_KEY_PREFIX, EditorSnapshot, clear_all_drafts, delete_draft, list_drafts,
    load_from_storage, save_to_storage,
};

// UI components
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
