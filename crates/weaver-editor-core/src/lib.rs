//! weaver-editor-core: Pure Rust editor logic without framework dependencies.
//!
//! This crate provides:
//! - `TextBuffer` trait for text storage abstraction
//! - `EditorRope` - ropey-backed implementation
//! - `UndoableBuffer<T>` - TextBuffer wrapper with undo/redo
//! - `EditorDocument` trait - interface for editor implementations
//! - `PlainEditor<T>` - simple field-based EditorDocument impl
//! - Rendering types and offset mapping utilities

pub mod document;
pub mod offset_map;
pub mod paragraph;
pub mod render;
pub mod syntax;
pub mod text;
pub mod types;
pub mod undo;
pub mod visibility;
pub mod writer;

pub use offset_map::{
    OffsetMapping, RenderResult, SnapDirection, SnappedPosition, find_mapping_for_byte,
    find_mapping_for_char, find_nearest_valid_position, is_valid_cursor_position,
};
pub use paragraph::{ParagraphRender, hash_source, make_paragraph_id};
pub use smol_str::SmolStr;
pub use syntax::{SyntaxSpanInfo, SyntaxType, classify_syntax};
pub use text::{EditorRope, TextBuffer};
pub use types::{
    Affinity, CompositionState, CursorState, EditInfo, EditorImage, Selection, BLOCK_SYNTAX_ZONE,
};
pub use document::{EditorDocument, PlainEditor};
pub use render::{EmbedContentProvider, ImageResolver, WikilinkValidator};
pub use undo::{UndoManager, UndoableBuffer};
pub use visibility::VisibilityState;
pub use writer::{EditorImageResolver, EditorWriter, SegmentedWriter, WriterResult};
