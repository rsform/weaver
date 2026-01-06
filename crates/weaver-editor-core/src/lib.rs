//! weaver-editor-core: Pure Rust editor logic without framework dependencies.
//!
//! This crate provides:
//! - `TextBuffer` trait for text storage abstraction
//! - `EditorRope` - ropey-backed implementation
//! - `UndoableBuffer<T>` - TextBuffer wrapper with undo/redo
//! - `EditorDocument` trait - interface for editor implementations
//! - `PlainEditor<T>` - simple field-based EditorDocument impl
//! - `EditorAction`, `InputType`, `Key` - platform-agnostic input/action types
//! - Rendering types and offset mapping utilities

pub mod actions;
pub mod document;
pub mod execute;
pub mod offset_map;
pub mod paragraph;
pub mod platform;
pub mod render;
pub mod render_cache;
pub mod syntax;
pub mod text;
pub mod text_helpers;
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
    Affinity, CompositionState, CursorRect, CursorState, EditInfo, EditorImage, Selection,
    SelectionRect, BLOCK_SYNTAX_ZONE,
};
pub use document::{EditorDocument, PlainEditor};
pub use render::{EmbedContentProvider, ImageResolver, WikilinkValidator};
pub use undo::{UndoManager, UndoableBuffer};
pub use visibility::VisibilityState;
pub use writer::{EditorImageResolver, EditorWriter, SegmentedWriter, WriterResult};
pub use platform::{CursorPlatform, CursorSync, PlatformError};
pub use actions::{
    EditorAction, FormatAction, InputType, Key, KeyCombo, KeybindingConfig, KeydownResult,
    Modifiers, Range,
};
pub use execute::execute_action;
pub use text_helpers::{
    ListContext, count_leading_zero_width, detect_list_context, find_line_end, find_line_start,
    find_word_boundary_backward, find_word_boundary_forward, is_list_item_empty,
    is_zero_width_char,
};
pub use render_cache::{
    CachedParagraph, IncrementalRenderResult, RenderCache, apply_delta, is_boundary_affecting,
    render_paragraphs_incremental,
};
