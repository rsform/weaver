//! Loro-backed text buffer implementing core editor traits.

use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;

use loro::{LoroDoc, LoroText, UndoManager as LoroUndoManager, VersionVector};
use smol_str::{SmolStr, ToSmolStr};
use weaver_editor_core::{TextBuffer, UndoManager};

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
        self.content.to_string().len()
    }

    fn len_chars(&self) -> usize {
        self.content.len_unicode()
    }

    fn insert(&mut self, char_offset: usize, text: &str) {
        self.content.insert(char_offset, text).ok();
    }

    fn delete(&mut self, char_range: Range<usize>) {
        self.content.delete(char_range.start, char_range.len()).ok();
    }

    fn slice(&self, char_range: Range<usize>) -> Option<SmolStr> {
        let s = self.content.to_string();
        let chars: Vec<char> = s.chars().collect();

        if char_range.end > chars.len() {
            return None;
        }

        let slice: String = chars[char_range].iter().collect();
        Some(slice.to_smolstr())
    }

    fn char_at(&self, char_offset: usize) -> Option<char> {
        let s = self.content.to_string();
        s.chars().nth(char_offset)
    }

    fn to_string(&self) -> String {
        self.content.to_string()
    }

    fn char_to_byte(&self, char_offset: usize) -> usize {
        let s = self.content.to_string();
        s.char_indices()
            .nth(char_offset)
            .map(|(i, _)| i)
            .unwrap_or(s.len())
    }

    fn byte_to_char(&self, byte_offset: usize) -> usize {
        let s = self.content.to_string();
        s[..byte_offset.min(s.len())].chars().count()
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
