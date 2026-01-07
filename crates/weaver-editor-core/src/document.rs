//! Core editor document trait and implementations.
//!
//! Defines the `EditorDocument` trait for abstracting editor behavior,
//! allowing different storage strategies (plain fields vs Signals) while
//! sharing the core editing logic.

use std::ops::Range;

use smol_str::SmolStr;
use web_time::Instant;

use crate::text::TextBuffer;
use crate::types::{BLOCK_SYNTAX_ZONE, CompositionState, CursorState, EditInfo, Selection};
use crate::undo::UndoManager;

/// Core trait for editor documents.
///
/// Defines the interface for any editor implementation. Different backends
/// can implement this trait with different storage strategies:
/// - `PlainEditor<T>`: Simple field-based storage
/// - Reactive implementations: Use Signals/state management
///
/// The trait is generic over the buffer type, which must implement both
/// `TextBuffer` (for text operations) and `UndoManager` (for undo/redo).
pub trait EditorDocument {
    /// The buffer type used for text storage and undo.
    type Buffer: TextBuffer + UndoManager;

    // === Required: Buffer access ===

    /// Get a reference to the underlying buffer.
    fn buffer(&self) -> &Self::Buffer;

    /// Get a mutable reference to the underlying buffer.
    fn buffer_mut(&mut self) -> &mut Self::Buffer;

    // === Required: Cursor/selection state ===

    /// Get the current cursor state.
    fn cursor(&self) -> CursorState;

    /// Set the cursor state.
    fn set_cursor(&mut self, cursor: CursorState);

    /// Get the current selection, if any.
    fn selection(&self) -> Option<Selection>;

    /// Set the selection.
    fn set_selection(&mut self, selection: Option<Selection>);

    // === Required: Edit tracking ===

    /// Get the last edit info, if any.
    fn last_edit(&self) -> Option<EditInfo>;

    /// Set the last edit info.
    fn set_last_edit(&mut self, edit: Option<EditInfo>);

    // === Required: Composition (IME) state ===

    /// Get the current composition state.
    fn composition(&self) -> Option<CompositionState>;

    /// Set the composition state.
    fn set_composition(&mut self, composition: Option<CompositionState>);

    /// Get the timestamp when composition last ended (Safari timing workaround).
    ///
    /// Returns None if composition never ended or implementation doesn't track it.
    fn composition_ended_at(&self) -> Option<web_time::Instant>;

    /// Record that composition ended now (Safari timing workaround).
    ///
    /// Implementations that don't need Safari workarounds can make this a no-op.
    fn set_composition_ended_now(&mut self);

    // === Required: Cursor snap hint ===

    /// Get the pending snap direction hint.
    ///
    /// This hints which direction the cursor should snap after an edit
    /// when the cursor lands on invisible syntax. Forward for insertions
    /// (snap toward new content), backward for deletions (snap toward
    /// remaining content).
    fn pending_snap(&self) -> Option<crate::SnapDirection>;

    /// Set the pending snap direction hint.
    fn set_pending_snap(&mut self, snap: Option<crate::SnapDirection>);

    // === Provided: Convenience accessors ===

    /// Get the cursor offset.
    fn cursor_offset(&self) -> usize {
        self.cursor().offset
    }

    /// Set just the cursor offset, preserving other cursor state.
    fn set_cursor_offset(&mut self, offset: usize) {
        let mut cursor = self.cursor();
        cursor.offset = offset;
        self.set_cursor(cursor);
    }

    /// Get the full content as a String.
    fn content_string(&self) -> String {
        self.buffer().to_string()
    }

    /// Get length in characters.
    fn len_chars(&self) -> usize {
        self.buffer().len_chars()
    }

    /// Get length in bytes.
    fn len_bytes(&self) -> usize {
        self.buffer().len_bytes()
    }

    /// Check if document is empty.
    fn is_empty(&self) -> bool {
        self.buffer().len_chars() == 0
    }

    /// Get a slice of the content.
    fn slice(&self, range: Range<usize>) -> Option<SmolStr> {
        self.buffer().slice(range)
    }

    /// Get character at offset.
    fn char_at(&self, offset: usize) -> Option<char> {
        self.buffer().char_at(offset)
    }

    /// Convert char offset to byte offset.
    fn char_to_byte(&self, char_offset: usize) -> usize {
        self.buffer().char_to_byte(char_offset)
    }

    /// Convert byte offset to char offset.
    fn byte_to_char(&self, byte_offset: usize) -> usize {
        self.buffer().byte_to_char(byte_offset)
    }

    /// Get selected text, if any.
    fn selected_text(&self) -> Option<SmolStr> {
        self.selection()
            .and_then(|sel| self.buffer().slice(sel.to_range()))
    }

    // === Provided: Text operations ===

    /// Insert text at char offset, returning edit info.
    fn insert(&mut self, offset: usize, text: &str) -> EditInfo {
        let contains_newline = text.contains('\n');
        let in_block_syntax_zone = self.is_in_block_syntax_zone(offset);

        self.buffer_mut().insert(offset, text);

        let inserted_len = text.chars().count();
        self.set_cursor_offset(offset + inserted_len);

        let edit = EditInfo {
            edit_char_pos: offset,
            inserted_len,
            deleted_len: 0,
            contains_newline,
            in_block_syntax_zone,
            doc_len_after: self.buffer().len_chars(),
            timestamp: Instant::now(),
        };

        self.set_last_edit(Some(edit.clone()));
        edit
    }

    /// Delete char range, returning edit info.
    fn delete(&mut self, range: Range<usize>) -> EditInfo {
        let deleted_text = self.buffer().slice(range.clone());
        let contains_newline = deleted_text
            .as_ref()
            .map(|s| s.contains('\n'))
            .unwrap_or(false);
        let in_block_syntax_zone = self.is_in_block_syntax_zone(range.start);
        let deleted_len = range.end - range.start;

        self.buffer_mut().delete(range.clone());
        self.set_cursor_offset(range.start);

        let edit = EditInfo {
            edit_char_pos: range.start,
            inserted_len: 0,
            deleted_len,
            contains_newline,
            in_block_syntax_zone,
            doc_len_after: self.buffer().len_chars(),
            timestamp: Instant::now(),
        };

        self.set_last_edit(Some(edit.clone()));
        edit
    }

    /// Replace char range with text, returning edit info.
    fn replace(&mut self, range: Range<usize>, text: &str) -> EditInfo {
        let deleted_text = self.buffer().slice(range.clone());
        let deleted_contains_newline = deleted_text
            .as_ref()
            .map(|s| s.contains('\n'))
            .unwrap_or(false);
        let contains_newline = text.contains('\n') || deleted_contains_newline;
        let in_block_syntax_zone = self.is_in_block_syntax_zone(range.start);
        let deleted_len = range.end - range.start;

        self.buffer_mut().delete(range.clone());
        self.buffer_mut().insert(range.start, text);

        let inserted_len = text.chars().count();
        self.set_cursor_offset(range.start + inserted_len);

        let edit = EditInfo {
            edit_char_pos: range.start,
            inserted_len,
            deleted_len,
            contains_newline,
            in_block_syntax_zone,
            doc_len_after: self.buffer().len_chars(),
            timestamp: Instant::now(),
        };

        self.set_last_edit(Some(edit.clone()));
        edit
    }

    /// Append text at end of document.
    ///
    /// This is a fast path for appending - delegates to buffer's push()
    /// which may have an optimized implementation.
    fn push(&mut self, text: &str) -> EditInfo {
        let offset = self.buffer().len_chars();
        let contains_newline = text.contains('\n');
        let in_block_syntax_zone = self.is_in_block_syntax_zone(offset);

        self.buffer_mut().push(text);

        let inserted_len = text.chars().count();
        self.set_cursor_offset(offset + inserted_len);

        let edit = EditInfo {
            edit_char_pos: offset,
            inserted_len,
            deleted_len: 0,
            contains_newline,
            in_block_syntax_zone,
            doc_len_after: self.buffer().len_chars(),
            timestamp: Instant::now(),
        };

        self.set_last_edit(Some(edit.clone()));
        edit
    }

    /// Delete the current selection, if any.
    fn delete_selection(&mut self) -> Option<EditInfo> {
        let sel = self.selection()?;
        self.set_selection(None);
        if sel.is_collapsed() {
            return None;
        }
        Some(self.delete(sel.to_range()))
    }

    // === Provided: Undo/Redo ===

    fn undo(&mut self) -> bool {
        self.buffer_mut().undo()
    }

    fn redo(&mut self) -> bool {
        self.buffer_mut().redo()
    }

    fn can_undo(&self) -> bool {
        self.buffer().can_undo()
    }

    fn can_redo(&self) -> bool {
        self.buffer().can_redo()
    }

    fn clear_history(&mut self) {
        self.buffer_mut().clear_history();
    }

    // === Provided: Helpers ===

    /// Check if offset is in the block-syntax zone (first ~6 chars of line).
    fn is_in_block_syntax_zone(&self, offset: usize) -> bool {
        let mut line_start = offset;
        while line_start > 0 {
            if let Some('\n') = self.buffer().char_at(line_start - 1) {
                break;
            }
            line_start -= 1;
        }
        offset - line_start < BLOCK_SYNTAX_ZONE
    }
}

/// Simple field-based implementation of EditorDocument.
///
/// Stores cursor, selection, and edit state as plain fields.
/// Use this for non-reactive contexts or as a base for testing.
#[derive(Clone)]
pub struct PlainEditor<T: TextBuffer + UndoManager> {
    buffer: T,
    cursor: CursorState,
    selection: Option<Selection>,
    last_edit: Option<EditInfo>,
    composition: Option<CompositionState>,
    composition_ended_at: Option<web_time::Instant>,
    pending_snap: Option<crate::SnapDirection>,
}

impl<T: TextBuffer + UndoManager + Default> Default for PlainEditor<T> {
    fn default() -> Self {
        Self::new(T::default())
    }
}

impl<T: TextBuffer + UndoManager> PlainEditor<T> {
    /// Create a new editor with the given buffer.
    pub fn new(buffer: T) -> Self {
        Self {
            buffer,
            cursor: CursorState::default(),
            selection: None,
            last_edit: None,
            composition: None,
            composition_ended_at: None,
            pending_snap: None,
        }
    }

    /// Get direct access to the inner buffer (bypasses trait).
    pub fn inner(&self) -> &T {
        &self.buffer
    }

    /// Get direct mutable access to the inner buffer (bypasses trait).
    pub fn inner_mut(&mut self) -> &mut T {
        &mut self.buffer
    }
}

impl<T: TextBuffer + UndoManager> EditorDocument for PlainEditor<T> {
    type Buffer = T;

    fn buffer(&self) -> &Self::Buffer {
        &self.buffer
    }

    fn buffer_mut(&mut self) -> &mut Self::Buffer {
        &mut self.buffer
    }

    fn cursor(&self) -> CursorState {
        self.cursor.clone()
    }

    fn set_cursor(&mut self, cursor: CursorState) {
        self.cursor = cursor;
    }

    fn selection(&self) -> Option<Selection> {
        self.selection.clone()
    }

    fn set_selection(&mut self, selection: Option<Selection>) {
        self.selection = selection;
    }

    fn last_edit(&self) -> Option<EditInfo> {
        self.last_edit.clone()
    }

    fn set_last_edit(&mut self, edit: Option<EditInfo>) {
        self.last_edit = edit;
    }

    fn composition(&self) -> Option<CompositionState> {
        self.composition.clone()
    }

    fn set_composition(&mut self, composition: Option<CompositionState>) {
        self.composition = composition;
    }

    fn composition_ended_at(&self) -> Option<web_time::Instant> {
        self.composition_ended_at
    }

    fn set_composition_ended_now(&mut self) {
        self.composition_ended_at = Some(web_time::Instant::now());
    }

    fn pending_snap(&self) -> Option<crate::SnapDirection> {
        self.pending_snap
    }

    fn set_pending_snap(&mut self, snap: Option<crate::SnapDirection>) {
        self.pending_snap = snap;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EditorRope, UndoableBuffer};

    type TestEditor = PlainEditor<UndoableBuffer<EditorRope>>;

    fn make_editor(content: &str) -> TestEditor {
        let rope = EditorRope::from_str(content);
        let buf = UndoableBuffer::new(rope, 100);
        PlainEditor::new(buf)
    }

    #[test]
    fn test_basic_insert() {
        let mut editor = make_editor("hello");
        assert_eq!(editor.content_string(), "hello");

        let edit = editor.insert(5, " world");
        assert_eq!(editor.content_string(), "hello world");
        assert_eq!(edit.inserted_len, 6);
        assert_eq!(editor.cursor_offset(), 11);
    }

    #[test]
    fn test_delete() {
        let mut editor = make_editor("hello world");

        let edit = editor.delete(5..11);
        assert_eq!(editor.content_string(), "hello");
        assert_eq!(edit.deleted_len, 6);
        assert_eq!(editor.cursor_offset(), 5);
    }

    #[test]
    fn test_replace() {
        let mut editor = make_editor("hello world");

        let edit = editor.replace(6..11, "rust");
        assert_eq!(editor.content_string(), "hello rust");
        assert_eq!(edit.deleted_len, 5);
        assert_eq!(edit.inserted_len, 4);
    }

    #[test]
    fn test_undo_redo() {
        let mut editor = make_editor("hello");

        editor.insert(5, " world");
        assert_eq!(editor.content_string(), "hello world");

        assert!(editor.undo());
        assert_eq!(editor.content_string(), "hello");

        assert!(editor.redo());
        assert_eq!(editor.content_string(), "hello world");
    }

    #[test]
    fn test_selection() {
        let mut editor = make_editor("hello world");

        editor.set_selection(Some(Selection::new(0, 5)));
        assert_eq!(editor.selected_text(), Some("hello".into()));

        let edit = editor.delete_selection();
        assert!(edit.is_some());
        assert_eq!(editor.content_string(), " world");
        assert!(editor.selection().is_none());
    }

    #[test]
    fn test_block_syntax_zone() {
        let mut editor = make_editor("# heading\nparagraph");

        // Position 0 is in block syntax zone
        let edit = editor.insert(0, "x");
        assert!(edit.in_block_syntax_zone);

        // Position after newline (start of "paragraph") is also in zone
        // Original was "# heading\nparagraph", after insert "x# heading\nparagraph"
        // Position 11 is start of "paragraph" line
        let edit = editor.insert(11, "y");
        assert!(edit.in_block_syntax_zone);
    }

    #[test]
    fn test_composition_state() {
        let mut editor = make_editor("hello");

        assert!(editor.composition().is_none());

        let comp = CompositionState::new(5, "わ".into());
        editor.set_composition(Some(comp.clone()));

        assert_eq!(editor.composition(), Some(comp));

        editor.set_composition(None);
        assert!(editor.composition().is_none());
    }

    #[test]
    fn test_offset_conversions() {
        let editor = make_editor("héllo wörld"); // multi-byte chars

        // 'é' is 2 bytes, 'ö' is 2 bytes
        // chars: h é l l o   w ö r l d
        // idx:   0 1 2 3 4 5 6 7 8 9 10

        assert_eq!(editor.len_chars(), 11);
        assert!(editor.len_bytes() > 11); // multi-byte chars

        // char 1 ('é') starts at byte 1
        assert_eq!(editor.char_to_byte(1), 1);
        // char 2 ('l') starts after 'é' (2 bytes)
        assert_eq!(editor.char_to_byte(2), 3);
    }
}
