//! Loro-backed text buffer implementing core editor traits.

use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;

use loro::{
    cursor::{Cursor, PosType, Side},
    LoroDoc, LoroText, UndoManager as LoroUndoManager, VersionVector,
};
use smol_str::{SmolStr, ToSmolStr};
use web_time::Instant;
use weaver_editor_core::{EditInfo, TextBuffer, UndoManager};

use crate::CrdtError;

/// Mutable state that must be shared across clones.
struct LoroTextBufferInner {
    undo_mgr: LoroUndoManager,
    last_edit: Option<EditInfo>,
    loro_cursor: Option<Cursor>,
}

/// Loro-backed text buffer with undo/redo support.
///
/// Wraps a `LoroDoc` with a text container and provides implementations
/// of the `TextBuffer` and `UndoManager` traits from weaver-editor-core.
///
/// Also provides CRDT-aware cursor tracking that survives remote edits
/// and undo/redo operations.
///
/// Cloning is cheap and clones share all mutable state (undo history,
/// last edit info, cursor position).
#[derive(Clone)]
pub struct LoroTextBuffer {
    doc: LoroDoc,
    content: LoroText,
    inner: Rc<RefCell<LoroTextBufferInner>>,
}

impl LoroTextBuffer {
    /// Create a new empty buffer.
    pub fn new() -> Self {
        let doc = LoroDoc::new();
        let content = doc.get_text("content");
        let loro_cursor = content.get_cursor(0, Side::default());

        Self {
            inner: Rc::new(RefCell::new(LoroTextBufferInner {
                undo_mgr: LoroUndoManager::new(&doc),
                last_edit: None,
                loro_cursor,
            })),
            doc,
            content,
        }
    }

    /// Create a buffer from an existing Loro snapshot.
    pub fn from_snapshot(snapshot: &[u8]) -> Result<Self, CrdtError> {
        let doc = LoroDoc::new();
        doc.import(snapshot)?;
        let content = doc.get_text("content");
        let loro_cursor = content.get_cursor(0, Side::default());

        Ok(Self {
            inner: Rc::new(RefCell::new(LoroTextBufferInner {
                undo_mgr: LoroUndoManager::new(&doc),
                last_edit: None,
                loro_cursor,
            })),
            doc,
            content,
        })
    }

    /// Create a buffer from an existing LoroDoc with a specific text container key.
    ///
    /// Useful for shared documents where multiple text fields exist in the same doc.
    /// The doc is cloned (cheap - Arc-backed) so the buffer shares state with the original.
    pub fn from_doc(doc: LoroDoc, key: &str) -> Self {
        let content = doc.get_text(key);
        let loro_cursor = content.get_cursor(0, Side::default());

        Self {
            inner: Rc::new(RefCell::new(LoroTextBufferInner {
                undo_mgr: LoroUndoManager::new(&doc),
                last_edit: None,
                loro_cursor,
            })),
            doc,
            content,
        }
    }

    /// Get the underlying Loro document.
    pub fn doc(&self) -> &LoroDoc {
        &self.doc
    }

    /// Get the text container.
    pub fn content(&self) -> &LoroText {
        &self.content
    }

    /// Export full snapshot.
    pub fn export_snapshot(&self) -> Vec<u8> {
        self.doc
            .export(loro::ExportMode::Snapshot)
            .expect("snapshot export should not fail")
    }

    /// Export updates since given version.
    pub fn export_updates_since(&self, version: &VersionVector) -> Option<Vec<u8>> {
        use std::borrow::Cow;

        let current_vv = self.doc.oplog_vv();

        if *version == current_vv {
            return None;
        }

        let updates = self
            .doc
            .export(loro::ExportMode::Updates {
                from: Cow::Owned(version.clone()),
            })
            .ok()?;

        if updates.is_empty() {
            return None;
        }

        Some(updates)
    }

    /// Import remote changes.
    pub fn import(&mut self, data: &[u8]) -> Result<(), CrdtError> {
        self.doc.import(data)?;
        Ok(())
    }

    /// Get current version vector.
    pub fn version(&self) -> VersionVector {
        self.doc.oplog_vv()
    }

    // --- Cursor management ---

    /// Sync the Loro cursor to track a specific char offset.
    /// Call this after local edits where you know the new cursor position.
    pub fn sync_cursor(&self, offset: usize) {
        self.inner.borrow_mut().loro_cursor = self.content.get_cursor(offset, Side::default());
    }

    /// Resolve the Loro cursor to its current char offset.
    /// Call this after undo/redo or remote edits where the position may have shifted.
    /// Returns None if no cursor is set or resolution fails.
    pub fn resolve_cursor(&self) -> Option<usize> {
        let inner = self.inner.borrow();
        let cursor = inner.loro_cursor.as_ref()?;
        let result = self.doc.get_cursor_pos(cursor).ok()?;
        Some(result.current.pos.min(self.content.len_unicode()))
    }

    /// Get a clone of the Loro cursor for serialization.
    pub fn loro_cursor(&self) -> Option<Cursor> {
        self.inner.borrow().loro_cursor.clone()
    }

    /// Set the Loro cursor (used when restoring from storage).
    pub fn set_loro_cursor(&self, cursor: Option<Cursor>) {
        self.inner.borrow_mut().loro_cursor = cursor;
    }
}

impl Default for LoroTextBuffer {
    fn default() -> Self {
        Self::new()
    }
}

impl TextBuffer for LoroTextBuffer {
    fn len_bytes(&self) -> usize {
        self.content.len_utf8()
    }

    fn len_chars(&self) -> usize {
        self.content.len_unicode()
    }

    fn insert(&mut self, char_offset: usize, text: &str) {
        let in_block_syntax_zone = self.is_in_block_syntax_zone(char_offset);
        let contains_newline = text.contains('\n');

        self.content.insert(char_offset, text).ok();

        self.inner.borrow_mut().last_edit = Some(EditInfo {
            edit_char_pos: char_offset,
            inserted_len: text.chars().count(),
            deleted_len: 0,
            contains_newline,
            in_block_syntax_zone,
            doc_len_after: self.content.len_unicode(),
            timestamp: Instant::now(),
        });
    }

    fn delete(&mut self, char_range: Range<usize>) {
        let in_block_syntax_zone = self.is_in_block_syntax_zone(char_range.start);
        let contains_newline = self
            .slice(char_range.clone())
            .map(|s| s.contains('\n'))
            .unwrap_or(false);
        let deleted_len = char_range.len();

        self.content.delete(char_range.start, deleted_len).ok();

        self.inner.borrow_mut().last_edit = Some(EditInfo {
            edit_char_pos: char_range.start,
            inserted_len: 0,
            deleted_len,
            contains_newline,
            in_block_syntax_zone,
            doc_len_after: self.content.len_unicode(),
            timestamp: Instant::now(),
        });
    }

    fn slice(&self, char_range: Range<usize>) -> Option<SmolStr> {
        if char_range.end > self.content.len_unicode() {
            return None;
        }
        self.content
            .slice(char_range.start, char_range.end)
            .ok()
            .map(|s| s.to_smolstr())
    }

    fn char_at(&self, char_offset: usize) -> Option<char> {
        self.content.char_at(char_offset).ok()
    }

    fn to_string(&self) -> String {
        self.content.to_string()
    }

    fn char_to_byte(&self, char_offset: usize) -> usize {
        self.content
            .convert_pos(char_offset, PosType::Unicode, PosType::Bytes)
            .unwrap_or(self.content.len_utf8())
    }

    fn byte_to_char(&self, byte_offset: usize) -> usize {
        self.content
            .convert_pos(byte_offset, PosType::Bytes, PosType::Unicode)
            .unwrap_or(self.content.len_unicode())
    }

    fn last_edit(&self) -> Option<EditInfo> {
        self.inner.borrow().last_edit
    }
}

impl UndoManager for LoroTextBuffer {
    fn can_undo(&self) -> bool {
        self.inner.borrow().undo_mgr.can_undo()
    }

    fn can_redo(&self) -> bool {
        self.inner.borrow().undo_mgr.can_redo()
    }

    fn undo(&mut self) -> bool {
        self.inner.borrow_mut().undo_mgr.undo().is_ok()
    }

    fn redo(&mut self) -> bool {
        self.inner.borrow_mut().undo_mgr.redo().is_ok()
    }

    fn clear_history(&mut self) {
        self.inner.borrow_mut().undo_mgr = LoroUndoManager::new(&self.doc);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_operations() {
        let mut buffer = LoroTextBuffer::new();

        buffer.insert(0, "Hello");
        assert_eq!(buffer.to_string(), "Hello");

        buffer.insert(5, " World");
        assert_eq!(buffer.to_string(), "Hello World");

        buffer.delete(5..6);
        assert_eq!(buffer.to_string(), "HelloWorld");
    }

    #[test]
    fn test_snapshot_roundtrip() {
        let mut buffer = LoroTextBuffer::new();
        buffer.insert(0, "Test content");

        let snapshot = buffer.export_snapshot();
        let restored = LoroTextBuffer::from_snapshot(&snapshot).unwrap();

        assert_eq!(restored.to_string(), "Test content");
    }

    #[test]
    fn test_slice() {
        let mut buffer = LoroTextBuffer::new();
        buffer.insert(0, "Hello World");

        assert_eq!(buffer.slice(0..5).as_deref(), Some("Hello"));
        assert_eq!(buffer.slice(6..11).as_deref(), Some("World"));
        assert_eq!(buffer.slice(0..100), None);
    }

    #[test]
    fn test_offset_conversion() {
        let mut buffer = LoroTextBuffer::new();
        buffer.insert(0, "hello 🌍");

        assert_eq!(buffer.len_chars(), 7); // h e l l o   🌍
        assert_eq!(buffer.len_bytes(), 10); // 6 + 4

        assert_eq!(buffer.char_to_byte(6), 6); // before emoji
        assert_eq!(buffer.char_to_byte(7), 10); // after emoji
    }

    #[test]
    fn test_clone_shares_state() {
        let mut buffer1 = LoroTextBuffer::new();
        buffer1.insert(0, "Hello");

        let buffer2 = buffer1.clone();

        // Both should see the same last_edit
        assert_eq!(buffer1.last_edit(), buffer2.last_edit());

        // Edit through buffer1
        buffer1.insert(5, " World");

        // buffer2 should see the updated last_edit (shared state)
        assert_eq!(buffer1.last_edit(), buffer2.last_edit());
        assert_eq!(buffer2.last_edit().unwrap().inserted_len, 6);
    }

    #[test]
    fn test_cursor_management() {
        let mut buffer = LoroTextBuffer::new();
        buffer.insert(0, "Hello World");

        // Sync cursor to position 5
        buffer.sync_cursor(5);
        assert_eq!(buffer.resolve_cursor(), Some(5));

        // Insert text before cursor - cursor should shift
        buffer.insert(0, "Hi ");
        // After insert, cursor tracked by Loro should have shifted
        let pos = buffer.resolve_cursor().unwrap();
        assert_eq!(pos, 8); // 5 + 3 = 8
    }
}
