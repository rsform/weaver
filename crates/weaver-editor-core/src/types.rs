//! Core editor types: cursor, selection, composition, and edit tracking.
//!
//! These types are framework-agnostic and can be used with any text buffer implementation.

use std::ops::Range;

use jacquard::types::string::AtUri;
use weaver_api::sh_weaver::embed::images::Image;
use web_time::Instant;

/// Image stored in the editor, with optional publish state tracking.
#[derive(Clone, Debug)]
pub struct EditorImage {
    /// The lexicon Image type (deserialized via from_json_value)
    pub image: Image<'static>,
    /// AT-URI of the PublishedBlob record (for cleanup on publish/delete).
    /// None for existing images that are already in an entry record.
    pub published_blob_uri: Option<AtUri<'static>>,
}

/// Cursor state including position and affinity.
#[derive(Clone, Debug, Copy, PartialEq, Eq)]
pub struct CursorState {
    /// Character offset in text (NOT byte offset!)
    pub offset: usize,

    /// Prefer left/right when at boundary (for vertical cursor movement)
    pub affinity: Affinity,
}

impl Default for CursorState {
    fn default() -> Self {
        Self {
            offset: 0,
            affinity: Affinity::Before,
        }
    }
}

impl CursorState {
    /// Create a new cursor at the given offset.
    pub fn new(offset: usize) -> Self {
        Self {
            offset,
            affinity: Affinity::Before,
        }
    }

    /// Create a cursor with specific affinity.
    pub fn with_affinity(offset: usize, affinity: Affinity) -> Self {
        Self { offset, affinity }
    }
}

/// Cursor affinity for vertical movement.
///
/// When navigating vertically, the cursor needs to know which side of a line
/// break it prefers. `Before` means stick to the end of the previous line,
/// `After` means stick to the start of the next line.
#[derive(Clone, Debug, Copy, PartialEq, Eq, Default)]
pub enum Affinity {
    #[default]
    Before,
    After,
}

/// Text selection with anchor and head positions.
///
/// The anchor is where the selection started, the head is where the cursor is now.
/// They may be in any order - use `start()` and `end()` for ordered bounds.
#[derive(Clone, Debug, Copy, PartialEq, Eq)]
pub struct Selection {
    /// Where selection started
    pub anchor: usize,
    /// Where cursor is now
    pub head: usize,
}

impl Selection {
    /// Create a new selection.
    pub fn new(anchor: usize, head: usize) -> Self {
        Self { anchor, head }
    }

    /// Create a collapsed selection (cursor position).
    pub fn collapsed(offset: usize) -> Self {
        Self {
            anchor: offset,
            head: offset,
        }
    }

    /// Get the start (lower bound) of the selection.
    pub fn start(&self) -> usize {
        self.anchor.min(self.head)
    }

    /// Get the end (upper bound) of the selection.
    pub fn end(&self) -> usize {
        self.anchor.max(self.head)
    }

    /// Check if the selection is collapsed (empty, cursor only).
    pub fn is_collapsed(&self) -> bool {
        self.anchor == self.head
    }

    /// Check if an offset is within the selection.
    pub fn contains(&self, offset: usize) -> bool {
        offset >= self.start() && offset < self.end()
    }

    /// Get the selection length.
    pub fn len(&self) -> usize {
        self.end() - self.start()
    }

    /// Check if empty (same as is_collapsed).
    pub fn is_empty(&self) -> bool {
        self.is_collapsed()
    }

    /// Convert to a Range<usize> (ordered).
    pub fn to_range(&self) -> Range<usize> {
        self.start()..self.end()
    }

    /// Check if the selection is backwards (head before anchor).
    pub fn is_backwards(&self) -> bool {
        self.head < self.anchor
    }
}

/// IME composition state (for international text input).
///
/// During IME composition, the user is building up a string of characters
/// that hasn't been committed yet. This tracks where that composition
/// started and what text is currently being composed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompositionState {
    /// Character offset where composition started
    pub start_offset: usize,
    /// Current composition text (uncommitted)
    pub text: String,
}

impl CompositionState {
    /// Create a new composition state.
    pub fn new(start_offset: usize, text: String) -> Self {
        Self { start_offset, text }
    }

    /// Get the end offset of the composition.
    pub fn end_offset(&self) -> usize {
        self.start_offset + self.text.chars().count()
    }

    /// Check if an offset is within the composition.
    pub fn contains(&self, offset: usize) -> bool {
        offset >= self.start_offset && offset < self.end_offset()
    }
}

/// Information about the most recent edit, used for incremental rendering optimization.
///
/// This tracks enough information to determine which paragraphs need re-rendering
/// after an edit, enabling efficient incremental updates instead of full re-renders.
#[derive(Clone, Debug)]
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
    /// Document length (in chars) after this edit was applied.
    /// Used to detect stale edit info - if current doc length doesn't match,
    /// the edit info is from a previous render cycle and shouldn't be used.
    pub doc_len_after: usize,
    /// When this edit occurred. Used for idle detection in collaborative sync.
    pub timestamp: Instant,
}

impl PartialEq for EditInfo {
    fn eq(&self, other: &Self) -> bool {
        // Compare all fields except timestamp (not meaningful for equality)
        self.edit_char_pos == other.edit_char_pos
            && self.inserted_len == other.inserted_len
            && self.deleted_len == other.deleted_len
            && self.contains_newline == other.contains_newline
            && self.in_block_syntax_zone == other.in_block_syntax_zone
            && self.doc_len_after == other.doc_len_after
    }
}

impl EditInfo {
    /// Check if this edit info is stale (doc has changed since this edit).
    pub fn is_stale(&self, current_doc_len: usize) -> bool {
        self.doc_len_after != current_doc_len
    }

    /// Check if this edit might affect paragraph boundaries.
    pub fn affects_boundaries(&self) -> bool {
        self.contains_newline || self.in_block_syntax_zone
    }

    /// Get the range that was affected by this edit.
    ///
    /// For insertions: the range of inserted text.
    /// For deletions: an empty range at the deletion point.
    /// For replacements: the range of inserted text.
    pub fn affected_range(&self) -> Range<usize> {
        self.edit_char_pos..self.edit_char_pos + self.inserted_len
    }
}

/// Max distance from line start where block syntax can appear.
/// Covers: `######` (6), ```` ``` ```` (3), `> ` (2), `- ` (2), `999. ` (5)
pub const BLOCK_SYNTAX_ZONE: usize = 6;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_selection_bounds() {
        // Forward selection
        let sel = Selection::new(5, 10);
        assert_eq!(sel.start(), 5);
        assert_eq!(sel.end(), 10);
        assert!(!sel.is_backwards());

        // Backward selection
        let sel = Selection::new(10, 5);
        assert_eq!(sel.start(), 5);
        assert_eq!(sel.end(), 10);
        assert!(sel.is_backwards());
    }

    #[test]
    fn test_selection_collapsed() {
        let sel = Selection::collapsed(7);
        assert!(sel.is_collapsed());
        assert!(sel.is_empty());
        assert_eq!(sel.len(), 0);
        assert_eq!(sel.start(), 7);
        assert_eq!(sel.end(), 7);
    }

    #[test]
    fn test_selection_contains() {
        let sel = Selection::new(5, 10);
        assert!(!sel.contains(4));
        assert!(sel.contains(5));
        assert!(sel.contains(7));
        assert!(sel.contains(9));
        assert!(!sel.contains(10)); // end is exclusive
    }

    #[test]
    fn test_selection_to_range() {
        let sel = Selection::new(10, 5);
        assert_eq!(sel.to_range(), 5..10);
    }

    #[test]
    fn test_composition_contains() {
        let comp = CompositionState::new(10, "你好".to_string());
        assert_eq!(comp.end_offset(), 12); // 2 chars
        assert!(!comp.contains(9));
        assert!(comp.contains(10));
        assert!(comp.contains(11));
        assert!(!comp.contains(12)); // end is exclusive
    }

    #[test]
    fn test_edit_info_stale() {
        let edit = EditInfo {
            edit_char_pos: 5,
            inserted_len: 3,
            deleted_len: 0,
            contains_newline: false,
            in_block_syntax_zone: false,
            doc_len_after: 100,
            timestamp: Instant::now(),
        };

        assert!(!edit.is_stale(100));
        assert!(edit.is_stale(101));
    }

    #[test]
    fn test_edit_info_affects_boundaries() {
        let edit = EditInfo {
            edit_char_pos: 0,
            inserted_len: 1,
            deleted_len: 0,
            contains_newline: false,
            in_block_syntax_zone: true,
            doc_len_after: 100,
            timestamp: Instant::now(),
        };
        assert!(edit.affects_boundaries());

        let edit = EditInfo {
            edit_char_pos: 50,
            inserted_len: 1,
            deleted_len: 0,
            contains_newline: true,
            in_block_syntax_zone: false,
            doc_len_after: 100,
            timestamp: Instant::now(),
        };
        assert!(edit.affects_boundaries());

        let edit = EditInfo {
            edit_char_pos: 50,
            inserted_len: 1,
            deleted_len: 0,
            contains_newline: false,
            in_block_syntax_zone: false,
            doc_len_after: 100,
            timestamp: Instant::now(),
        };
        assert!(!edit.affects_boundaries());
    }
}
