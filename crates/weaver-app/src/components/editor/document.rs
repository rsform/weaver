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

    /// Most recent edit info for incremental rendering optimization.
    /// Used to determine if we can skip full re-parsing.
    pub last_edit: Option<EditInfo>,
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

/// Information about the most recent edit, used for incremental rendering optimization.
#[derive(Clone, Debug, Default)]
pub struct EditInfo {
    /// Character offset where the edit occurred
    pub edit_char_pos: usize,
    /// Number of characters inserted
    pub inserted_len: usize,
    /// Number of characters deleted
    pub deleted_len: usize,
    /// Whether the edit contains a newline (boundary-affecting)
    pub contains_newline: bool,
    /// Whether the edit is in the block-syntax zone of a line (first ~6 chars).
    /// Edits here could affect block-level syntax like headings, lists, code fences.
    pub in_block_syntax_zone: bool,
}

/// Max distance from line start where block syntax can appear.
/// Covers: `######` (6), ```` ``` ```` (3), `> ` (2), `- ` (2), `999. ` (5)
const BLOCK_SYNTAX_ZONE: usize = 6;

impl EditorDocument {
    /// Check if a character position is within the block-syntax zone of its line.
    fn is_in_block_syntax_zone(&self, pos: usize) -> bool {
        if pos == 0 {
            return true;
        }

        // Find distance from previous newline by scanning forward and tracking last newline
        let rope = self.rope.borrow();
        let mut last_newline_pos: Option<usize> = None;

        for (i, c) in rope.slice_chars(0..pos).enumerate() {
            if c == '\n' {
                last_newline_pos = Some(i);
            }
        }

        let chars_from_line_start = match last_newline_pos {
            Some(nl_pos) => pos - nl_pos - 1, // -1 because newline itself is not part of current line
            None => pos, // No newline found, distance is from document start
        };

        chars_from_line_start <= BLOCK_SYNTAX_ZONE
    }

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
            last_edit: None,
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

    /// Insert text and record edit info for incremental rendering.
    pub fn insert_tracked(&mut self, pos: usize, text: &str) {
        let in_block_syntax_zone = self.is_in_block_syntax_zone(pos);
        self.last_edit = Some(EditInfo {
            edit_char_pos: pos,
            inserted_len: text.chars().count(),
            deleted_len: 0,
            contains_newline: text.contains('\n'),
            in_block_syntax_zone,
        });
        self.rope.insert(pos, text);
    }

    /// Remove text range and record edit info for incremental rendering.
    pub fn remove_tracked(&mut self, range: std::ops::Range<usize>) {
        // Check if deleted region contains newline - borrow inner JumpRope
        let contains_newline = self.rope.borrow().slice_chars(range.clone()).any(|c| c == '\n');
        let in_block_syntax_zone = self.is_in_block_syntax_zone(range.start);
        self.last_edit = Some(EditInfo {
            edit_char_pos: range.start,
            inserted_len: 0,
            deleted_len: range.end - range.start,
            contains_newline,
            in_block_syntax_zone,
        });
        self.rope.remove(range);
    }

    /// Replace text (delete then insert) and record combined edit info.
    pub fn replace_tracked(&mut self, range: std::ops::Range<usize>, text: &str) {
        let delete_has_newline = self.rope.borrow().slice_chars(range.clone()).any(|c| c == '\n');
        let in_block_syntax_zone = self.is_in_block_syntax_zone(range.start);
        self.last_edit = Some(EditInfo {
            edit_char_pos: range.start,
            inserted_len: text.chars().count(),
            deleted_len: range.end - range.start,
            contains_newline: delete_has_newline || text.contains('\n'),
            in_block_syntax_zone,
        });
        self.rope.remove(range);
        self.rope.insert(self.last_edit.as_ref().unwrap().edit_char_pos, text);
    }
}
