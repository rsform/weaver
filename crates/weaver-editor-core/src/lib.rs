//! weaver-editor-core: Pure Rust editor logic without framework dependencies.
//!
//! This crate provides:
//! - `TextBuffer` trait for text storage abstraction
//! - `EditorRope` - ropey-backed implementation
//! - `EditorDocument<T>` - generic document with undo support
//! - Rendering, actions, formatting - all generic over TextBuffer

pub mod offset_map;
pub mod paragraph;
pub mod syntax;
pub mod text;
pub mod types;
pub mod visibility;

pub use offset_map::{
    OffsetMapping, RenderResult, SnapDirection, SnappedPosition, find_mapping_for_byte,
    find_mapping_for_char, find_nearest_valid_position, is_valid_cursor_position,
};
pub use paragraph::{ParagraphRender, hash_source, make_paragraph_id};
pub use smol_str::SmolStr;
pub use syntax::{SyntaxSpanInfo, SyntaxType, classify_syntax};
pub use text::{EditorRope, TextBuffer};
pub use types::{
    Affinity, CompositionState, CursorState, EditInfo, Selection, BLOCK_SYNTAX_ZONE,
};
pub use visibility::VisibilityState;
