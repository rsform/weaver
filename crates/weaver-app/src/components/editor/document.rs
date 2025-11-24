//! Core data structures for the markdown editor.

use jumprope::JumpRopeBuf;

/// Single source of truth for editor state.
///
/// Contains the document text, cursor position, selection, and IME composition state.
#[derive(Clone, Debug)]
pub struct EditorDocument {
    /// The rope storing document text (uses char offsets, not bytes).
    /// Uses JumpRopeBuf to batch consecutive edits for performance.
    pub rope: JumpRopeBuf,

    /// Current cursor position (char offset)
    pub cursor: CursorState,

    /// Active selection if any
    pub selection: Option<Selection>,

    /// IME composition state (for Phase 3)
    pub composition: Option<CompositionState>,
}

/// Cursor state including position and affinity.
#[derive(Clone, Debug, Copy)]
pub struct CursorState {
    /// Character offset in rope (NOT byte offset!)
    pub offset: usize,

    /// Prefer left/right when at boundary (for vertical cursor movement)
    pub affinity: Affinity,
}

/// Cursor affinity for vertical movement.
#[derive(Clone, Debug, Copy, PartialEq, Eq)]
pub enum Affinity {
    Before,
    After,
}

/// Text selection with anchor and head positions.
#[derive(Clone, Debug, Copy)]
pub struct Selection {
    /// Where selection started
    pub anchor: usize,
    /// Where cursor is now
    pub head: usize,
}

/// IME composition state (for international text input).
#[derive(Clone, Debug)]
pub struct CompositionState {
    pub start_offset: usize,
    pub text: String,
}

impl EditorDocument {
    /// Create a new editor document with the given content.
    pub fn new(content: String) -> Self {
        Self {
            rope: JumpRopeBuf::from(content.as_str()),
            cursor: CursorState {
                offset: 0,
                affinity: Affinity::Before,
            },
            selection: None,
            composition: None,
        }
    }

    /// Convert the document to a string.
    pub fn to_string(&self) -> String {
        self.rope.to_string()
    }

    /// Get the length of the document in characters.
    pub fn len_chars(&self) -> usize {
        self.rope.len_chars()
    }

    /// Check if the document is empty.
    pub fn is_empty(&self) -> bool {
        self.rope.len_chars() == 0
    }
}
