//! Core data structures for the markdown editor.
//!
//! Uses Loro CRDT for text storage with built-in undo/redo support.

use loro::{LoroDoc, LoroResult, LoroText, UndoManager};

/// Single source of truth for editor state.
///
/// Contains the document text (backed by Loro CRDT), cursor position,
/// selection, and IME composition state.
#[derive(Debug)]
pub struct EditorDocument {
    /// The Loro document containing all editor state.
    /// Using full LoroDoc (not just LoroText) to support future
    /// expansion to blobs, metadata, etc.
    doc: LoroDoc,

    /// Handle to the text container within the doc.
    text: LoroText,

    /// Undo manager for the document.
    undo_mgr: UndoManager,

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
    /// Character offset in text (NOT byte offset!)
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

        let content = self.text.to_string();
        let mut last_newline_pos: Option<usize> = None;

        for (i, c) in content.chars().take(pos).enumerate() {
            if c == '\n' {
                last_newline_pos = Some(i);
            }
        }

        let chars_from_line_start = match last_newline_pos {
            Some(nl_pos) => pos - nl_pos - 1,
            None => pos,
        };

        chars_from_line_start <= BLOCK_SYNTAX_ZONE
    }

    /// Create a new editor document with the given content.
    pub fn new(content: String) -> Self {
        let doc = LoroDoc::new();
        let text = doc.get_text("content");

        // Insert initial content if any
        if !content.is_empty() {
            text.insert(0, &content).expect("failed to insert initial content");
        }

        // Set up undo manager with merge interval for batching keystrokes
        let mut undo_mgr = UndoManager::new(&doc);
        undo_mgr.set_merge_interval(300); // 300ms merge window
        undo_mgr.set_max_undo_steps(100);

        Self {
            doc,
            text,
            undo_mgr,
            cursor: CursorState {
                offset: 0,
                affinity: Affinity::Before,
            },
            selection: None,
            composition: None,
            last_edit: None,
        }
    }

    /// Get the underlying LoroText for read operations.
    pub fn loro_text(&self) -> &LoroText {
        &self.text
    }

    /// Convert the document to a string.
    pub fn to_string(&self) -> String {
        self.text.to_string()
    }

    /// Get the length of the document in characters.
    pub fn len_chars(&self) -> usize {
        self.text.len_unicode()
    }

    /// Get the length of the document in UTF-8 bytes.
    pub fn len_bytes(&self) -> usize {
        self.text.len_utf8()
    }

    /// Get the length of the document in UTF-16 code units.
    pub fn len_utf16(&self) -> usize {
        self.text.len_utf16()
    }

    /// Check if the document is empty.
    pub fn is_empty(&self) -> bool {
        self.text.len_unicode() == 0
    }

    /// Insert text and record edit info for incremental rendering.
    pub fn insert_tracked(&mut self, pos: usize, text: &str) -> LoroResult<()> {
        let in_block_syntax_zone = self.is_in_block_syntax_zone(pos);
        self.last_edit = Some(EditInfo {
            edit_char_pos: pos,
            inserted_len: text.chars().count(),
            deleted_len: 0,
            contains_newline: text.contains('\n'),
            in_block_syntax_zone,
        });
        self.text.insert(pos, text)
    }

    /// Remove text range and record edit info for incremental rendering.
    pub fn remove_tracked(&mut self, start: usize, len: usize) -> LoroResult<()> {
        let content = self.text.to_string();
        let end = start + len;
        let contains_newline = content
            .chars()
            .skip(start)
            .take(len)
            .any(|c| c == '\n');
        let in_block_syntax_zone = self.is_in_block_syntax_zone(start);

        self.last_edit = Some(EditInfo {
            edit_char_pos: start,
            inserted_len: 0,
            deleted_len: len,
            contains_newline,
            in_block_syntax_zone,
        });
        self.text.delete(start, len)
    }

    /// Replace text (delete then insert) and record combined edit info.
    pub fn replace_tracked(&mut self, start: usize, len: usize, text: &str) -> LoroResult<()> {
        let content = self.text.to_string();
        let delete_has_newline = content
            .chars()
            .skip(start)
            .take(len)
            .any(|c| c == '\n');
        let in_block_syntax_zone = self.is_in_block_syntax_zone(start);

        self.last_edit = Some(EditInfo {
            edit_char_pos: start,
            inserted_len: text.chars().count(),
            deleted_len: len,
            contains_newline: delete_has_newline || text.contains('\n'),
            in_block_syntax_zone,
        });

        // Use splice for atomic replace
        self.text.splice(start, len, text)?;
        Ok(())
    }

    /// Undo the last operation.
    /// Returns true if an undo was performed.
    pub fn undo(&mut self) -> LoroResult<bool> {
        self.undo_mgr.undo()
    }

    /// Redo the last undone operation.
    /// Returns true if a redo was performed.
    pub fn redo(&mut self) -> LoroResult<bool> {
        self.undo_mgr.redo()
    }

    /// Check if undo is available.
    pub fn can_undo(&self) -> bool {
        self.undo_mgr.can_undo()
    }

    /// Check if redo is available.
    pub fn can_redo(&self) -> bool {
        self.undo_mgr.can_redo()
    }

    /// Get a slice of the document text.
    /// Returns None if the range is invalid.
    pub fn slice(&self, start: usize, end: usize) -> Option<String> {
        self.text.slice(start, end).ok()
    }
}

// EditorDocument can't derive Clone because LoroDoc/LoroText/UndoManager don't implement Clone.
// This is intentional - the document should be the single source of truth.

impl Clone for EditorDocument {
    fn clone(&self) -> Self {
        // Create a new document with the same content
        let content = self.to_string();
        let mut new_doc = Self::new(content);
        new_doc.cursor = self.cursor;
        new_doc.selection = self.selection;
        new_doc.composition = self.composition.clone();
        new_doc.last_edit = self.last_edit.clone();
        new_doc
    }
}
