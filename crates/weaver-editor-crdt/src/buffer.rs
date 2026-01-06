//! Loro-backed text buffer implementing core editor traits.

use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;

use loro::{cursor::PosType, LoroDoc, LoroText, UndoManager as LoroUndoManager, VersionVector};
use smol_str::{SmolStr, ToSmolStr};
use web_time::Instant;
use weaver_editor_core::{EditInfo, TextBuffer, UndoManager};

use crate::CrdtError;

/// Loro-backed text buffer with undo/redo support.
///
/// Wraps a `LoroDoc` with a text container and provides implementations
/// of the `TextBuffer` and `UndoManager` traits from weaver-editor-core.
#[derive(Clone)]
pub struct LoroTextBuffer {
    doc: LoroDoc,
    content: LoroText,
    undo_mgr: Rc<RefCell<LoroUndoManager>>,
    last_edit: Option<EditInfo>,
}

impl LoroTextBuffer {
    /// Create a new empty buffer.
    pub fn new() -> Self {
        let doc = LoroDoc::new();
        let content = doc.get_text("content");
        let undo_mgr = Rc::new(RefCell::new(LoroUndoManager::new(&doc)));

        Self {
            doc,
            content,
            undo_mgr,
            last_edit: None,
        }
    }

    /// Create a buffer from an existing Loro snapshot.
    pub fn from_snapshot(snapshot: &[u8]) -> Result<Self, CrdtError> {
        let doc = LoroDoc::new();
        doc.import(snapshot)?;
        let content = doc.get_text("content");
        let undo_mgr = Rc::new(RefCell::new(LoroUndoManager::new(&doc)));

        Ok(Self {
            doc,
            content,
            undo_mgr,
            last_edit: None,
        })
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

        self.last_edit = Some(EditInfo {
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

        self.last_edit = Some(EditInfo {
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

    fn last_edit(&self) -> Option<&EditInfo> {
        self.last_edit.as_ref()
    }
}

impl UndoManager for LoroTextBuffer {
    fn can_undo(&self) -> bool {
        self.undo_mgr.borrow().can_undo()
    }

    fn can_redo(&self) -> bool {
        self.undo_mgr.borrow().can_redo()
    }

    fn undo(&mut self) -> bool {
        self.undo_mgr.borrow_mut().undo().is_ok()
    }

    fn redo(&mut self) -> bool {
        self.undo_mgr.borrow_mut().redo().is_ok()
    }

    fn clear_history(&mut self) {
        // Loro's UndoManager doesn't have a clear method
        // Create a new one to effectively clear history
        self.undo_mgr = Rc::new(RefCell::new(LoroUndoManager::new(&self.doc)));
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
}
