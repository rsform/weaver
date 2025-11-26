//! Core data structures for the markdown editor.
//!
//! Uses Loro CRDT for text storage with built-in undo/redo support.

use loro::{cursor::{Cursor, Side}, ExportMode, LoroDoc, LoroResult, LoroText, UndoManager};

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

    /// Current cursor position (char offset) - fast local cache.
    /// This is the authoritative position for immediate operations.
    pub cursor: CursorState,

    /// CRDT-aware cursor that tracks position through remote edits and undo/redo.
    /// Recreated after our own edits, queried after undo/redo/remote edits.
    loro_cursor: Option<Cursor>,

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
    /// Document length (in chars) after this edit was applied.
    /// Used to detect stale edit info - if current doc length doesn't match,
    /// the edit info is from a previous render cycle and shouldn't be used.
    pub doc_len_after: usize,
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

        // Create initial Loro cursor at position 0
        let loro_cursor = text.get_cursor(0, Side::default());

        Self {
            doc,
            text,
            undo_mgr,
            cursor: CursorState {
                offset: 0,
                affinity: Affinity::Before,
            },
            loro_cursor,
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
        let len_before = self.text.len_unicode();
        let result = self.text.insert(pos, text);
        let len_after = self.text.len_unicode();
        self.last_edit = Some(EditInfo {
            edit_char_pos: pos,
            inserted_len: len_after.saturating_sub(len_before),
            deleted_len: 0,
            contains_newline: text.contains('\n'),
            in_block_syntax_zone,
            doc_len_after: len_after,
        });
        result
    }

    /// Push text to end of document. Faster than insert for appending.
    pub fn push_tracked(&mut self, text: &str) -> LoroResult<()> {
        let pos = self.text.len_unicode();
        let in_block_syntax_zone = self.is_in_block_syntax_zone(pos);
        let result = self.text.push_str(text);
        let len_after = self.text.len_unicode();
        self.last_edit = Some(EditInfo {
            edit_char_pos: pos,
            inserted_len: text.chars().count(),
            deleted_len: 0,
            contains_newline: text.contains('\n'),
            in_block_syntax_zone,
            doc_len_after: len_after,
        });
        result
    }

    /// Remove text range and record edit info for incremental rendering.
    pub fn remove_tracked(&mut self, start: usize, len: usize) -> LoroResult<()> {
        let content = self.text.to_string();
        let contains_newline = content
            .chars()
            .skip(start)
            .take(len)
            .any(|c| c == '\n');
        let in_block_syntax_zone = self.is_in_block_syntax_zone(start);

        let result = self.text.delete(start, len);
        self.last_edit = Some(EditInfo {
            edit_char_pos: start,
            inserted_len: 0,
            deleted_len: len,
            contains_newline,
            in_block_syntax_zone,
            doc_len_after: self.text.len_unicode(),
        });
        result
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

        let len_before = self.text.len_unicode();
        // Use splice for atomic replace
        self.text.splice(start, len, text)?;
        let len_after = self.text.len_unicode();

        // inserted_len = (len_after - len_before) + deleted_len
        // because: len_after = len_before - deleted + inserted
        let inserted_len = (len_after + len).saturating_sub(len_before);

        self.last_edit = Some(EditInfo {
            edit_char_pos: start,
            inserted_len,
            deleted_len: len,
            contains_newline: delete_has_newline || text.contains('\n'),
            in_block_syntax_zone,
            doc_len_after: len_after,
        });
        Ok(())
    }

    /// Undo the last operation.
    /// Returns true if an undo was performed.
    /// Automatically updates cursor position from the Loro cursor.
    pub fn undo(&mut self) -> LoroResult<bool> {
        // Sync Loro cursor to current position BEFORE undo
        // so it tracks through the undo operation
        self.sync_loro_cursor();

        let result = self.undo_mgr.undo()?;
        if result {
            // After undo, query Loro cursor for new position
            self.sync_cursor_from_loro();
        }
        Ok(result)
    }

    /// Redo the last undone operation.
    /// Returns true if a redo was performed.
    /// Automatically updates cursor position from the Loro cursor.
    pub fn redo(&mut self) -> LoroResult<bool> {
        // Sync Loro cursor to current position BEFORE redo
        self.sync_loro_cursor();

        let result = self.undo_mgr.redo()?;
        if result {
            // After redo, query Loro cursor for new position
            self.sync_cursor_from_loro();
        }
        Ok(result)
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

    /// Sync the Loro cursor to the current cursor.offset position.
    /// Call this after OUR edits where we know the new cursor position.
    pub fn sync_loro_cursor(&mut self) {
        self.loro_cursor = self.text.get_cursor(self.cursor.offset, Side::default());
    }

    /// Update cursor.offset from the Loro cursor's tracked position.
    /// Call this after undo/redo or remote edits where the position may have shifted.
    /// Returns the new offset, or None if the cursor couldn't be resolved.
    pub fn sync_cursor_from_loro(&mut self) -> Option<usize> {
        let loro_cursor = self.loro_cursor.as_ref()?;
        let result = self.doc.get_cursor_pos(loro_cursor).ok()?;
        let new_offset = result.current.pos;
        self.cursor.offset = new_offset.min(self.len_chars());
        Some(self.cursor.offset)
    }

    /// Get the Loro cursor for serialization.
    pub fn loro_cursor(&self) -> Option<&Cursor> {
        self.loro_cursor.as_ref()
    }

    /// Set the Loro cursor (used when restoring from storage).
    pub fn set_loro_cursor(&mut self, cursor: Option<Cursor>) {
        self.loro_cursor = cursor;
        // Sync cursor.offset from the restored Loro cursor
        if self.loro_cursor.is_some() {
            self.sync_cursor_from_loro();
        }
    }

    /// Export the document as a binary snapshot.
    /// This captures all CRDT state including undo history.
    pub fn export_snapshot(&self) -> Vec<u8> {
        self.doc.export(ExportMode::Snapshot).unwrap_or_default()
    }

    /// Get the current state frontiers for change detection.
    /// Frontiers represent the "version" of the document state.
    pub fn state_frontiers(&self) -> loro::Frontiers {
        self.doc.state_frontiers()
    }

    /// Create a new EditorDocument from a binary snapshot.
    /// Falls back to empty document if import fails.
    ///
    /// If `loro_cursor` is provided, it will be used to restore the cursor position.
    /// Otherwise, falls back to `fallback_offset`.
    ///
    /// Note: Undo/redo is session-only. The UndoManager tracks operations as they
    /// happen in real-time; it cannot rebuild history from imported CRDT ops.
    /// For cross-session "undo", use time travel via `doc.checkout(frontiers)`.
    pub fn from_snapshot(
        snapshot: &[u8],
        loro_cursor: Option<Cursor>,
        fallback_offset: usize,
    ) -> Self {
        let doc = LoroDoc::new();

        if !snapshot.is_empty() {
            if let Err(e) = doc.import(snapshot) {
                tracing::warn!("Failed to import snapshot: {:?}, creating empty doc", e);
            }
        }

        let text = doc.get_text("content");

        // Set up undo manager - tracks operations from this point forward only
        let mut undo_mgr = UndoManager::new(&doc);
        undo_mgr.set_merge_interval(300);
        undo_mgr.set_max_undo_steps(100);

        // Try to restore cursor from Loro cursor, fall back to offset
        let max_offset = text.len_unicode();
        let cursor_offset = if let Some(ref lc) = loro_cursor {
            doc.get_cursor_pos(lc)
                .map(|r| r.current.pos)
                .unwrap_or(fallback_offset)
        } else {
            fallback_offset
        };

        let cursor = CursorState {
            offset: cursor_offset.min(max_offset),
            affinity: Affinity::Before,
        };

        // If no Loro cursor provided, create one at the restored position
        let loro_cursor = loro_cursor.or_else(|| text.get_cursor(cursor.offset, Side::default()));

        Self {
            doc,
            text,
            undo_mgr,
            cursor,
            loro_cursor,
            selection: None,
            composition: None,
            last_edit: None,
        }
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
        // Recreate Loro cursor at the same position in the new doc
        new_doc.sync_loro_cursor();
        new_doc.selection = self.selection;
        new_doc.composition = self.composition.clone();
        new_doc.last_edit = self.last_edit.clone();
        new_doc
    }
}
