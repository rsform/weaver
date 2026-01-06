//! HTML writer for markdown editor - re-exports from weaver-editor-core.
//!
//! The core EditorWriter lives in weaver-editor-core. This module provides:
//! - Re-exports of core types for convenience
//! - App-specific EditorImageResolver for image URL resolution

pub mod embed;

// Re-export everything from core
pub use weaver_editor_core::{
    EditorRope, EditorWriter, EmbedContentProvider, ImageResolver, OffsetMapping, SegmentedWriter,
    SyntaxSpanInfo, SyntaxType, TextBuffer, WriterResult,
};

// App-specific image resolver
pub use embed::EditorImageResolver;
