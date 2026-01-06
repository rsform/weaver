//! Undo/redo management for editor operations.
//!
//! Provides:
//! - `UndoManager` trait for abstracting undo implementations
//! - `UndoableBuffer<T>` - wraps a TextBuffer and provides undo/redo

use std::ops::Range;

use smol_str::{SmolStr, ToSmolStr};

use crate::text::TextBuffer;

/// Trait for managing undo/redo operations.
///
/// Implementations must actually perform the undo/redo, not just track state.
/// For local editing, use `UndoableBuffer<T>` which wraps a TextBuffer.
/// For Loro, wrap LoroText + loro::UndoManager together.
pub trait UndoManager {
    /// Check if undo is available.
    fn can_undo(&self) -> bool;

    /// Check if redo is available.
    fn can_redo(&self) -> bool;

    /// Perform undo. Returns true if successful.
    fn undo(&mut self) -> bool;

    /// Perform redo. Returns true if successful.
    fn redo(&mut self) -> bool;

    /// Clear all undo/redo history.
    fn clear_history(&mut self);
}

/// A recorded edit operation for undo/redo.
#[derive(Debug, Clone)]
struct EditOperation {
    /// Character position where edit occurred
    pos: usize,
    /// Text that was deleted (empty for pure insertions)
    deleted: SmolStr,
    /// Text that was inserted (empty for pure deletions)
    inserted: SmolStr,
}

/// A TextBuffer wrapper that tracks edits and provides undo/redo.
///
/// This is the standard way to get undo support for local editing.
/// All mutations go through this wrapper, which records them for undo.
pub struct UndoableBuffer<T> {
    buffer: T,
    undo_stack: Vec<EditOperation>,
    redo_stack: Vec<EditOperation>,
    max_steps: usize,
}

impl<T: Clone> Clone for UndoableBuffer<T> {
    fn clone(&self) -> Self {
        Self {
            buffer: self.buffer.clone(),
            undo_stack: self.undo_stack.clone(),
            redo_stack: self.redo_stack.clone(),
            max_steps: self.max_steps,
        }
    }
}

impl<T: TextBuffer + Default> Default for UndoableBuffer<T> {
    fn default() -> Self {
        Self::new(T::default(), 100)
    }
}

impl<T: TextBuffer> UndoableBuffer<T> {
    /// Create a new undoable buffer wrapping the given buffer.
    pub fn new(buffer: T, max_steps: usize) -> Self {
        Self {
            buffer,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            max_steps,
        }
    }

    /// Get a reference to the inner buffer.
    pub fn inner(&self) -> &T {
        &self.buffer
    }

    /// Get a mutable reference to the inner buffer.
    /// WARNING: Edits made directly bypass undo tracking!
    pub fn inner_mut(&mut self) -> &mut T {
        &mut self.buffer
    }

    /// Record an operation (called internally by TextBuffer impl).
    fn record_op(&mut self, pos: usize, deleted: &str, inserted: &str) {
        // Clear redo stack on new edit
        self.redo_stack.clear();

        let op = EditOperation {
            pos,
            deleted: deleted.to_smolstr(),
            inserted: inserted.to_smolstr(),
        };

        self.undo_stack.push(op);

        // Trim if over max
        while self.undo_stack.len() > self.max_steps {
            self.undo_stack.remove(0);
        }
    }
}

// Implement TextBuffer by delegating to inner buffer + recording operations
impl<T: TextBuffer> TextBuffer for UndoableBuffer<T> {
    fn len_bytes(&self) -> usize {
        self.buffer.len_bytes()
    }

    fn len_chars(&self) -> usize {
        self.buffer.len_chars()
    }

    fn insert(&mut self, char_offset: usize, text: &str) {
        self.record_op(char_offset, "", text);
        self.buffer.insert(char_offset, text);
    }

    fn delete(&mut self, char_range: Range<usize>) {
        // Get the text being deleted for undo
        let deleted = self
            .buffer
            .slice(char_range.clone())
            .map(|s| s.to_string())
            .unwrap_or_default();
        self.record_op(char_range.start, &deleted, "");
        self.buffer.delete(char_range);
    }

    fn slice(&self, char_range: Range<usize>) -> Option<SmolStr> {
        self.buffer.slice(char_range)
    }

    fn char_at(&self, char_offset: usize) -> Option<char> {
        self.buffer.char_at(char_offset)
    }

    fn to_string(&self) -> String {
        self.buffer.to_string()
    }

    fn char_to_byte(&self, char_offset: usize) -> usize {
        self.buffer.char_to_byte(char_offset)
    }

    fn byte_to_char(&self, byte_offset: usize) -> usize {
        self.buffer.byte_to_char(byte_offset)
    }

    fn last_edit(&self) -> Option<&crate::types::EditInfo> {
        self.buffer.last_edit()
    }
}

impl<T: TextBuffer> UndoManager for UndoableBuffer<T> {
    fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    fn undo(&mut self) -> bool {
        let Some(op) = self.undo_stack.pop() else {
            return false;
        };

        // Apply inverse: delete what was inserted, insert what was deleted
        let inserted_chars = op.inserted.chars().count();
        if inserted_chars > 0 {
            self.buffer.delete(op.pos..op.pos + inserted_chars);
        }
        if !op.deleted.is_empty() {
            self.buffer.insert(op.pos, &op.deleted);
        }

        self.redo_stack.push(op);
        true
    }

    fn redo(&mut self) -> bool {
        let Some(op) = self.redo_stack.pop() else {
            return false;
        };

        // Re-apply original: delete what was deleted, insert what was inserted
        let deleted_chars = op.deleted.chars().count();
        if deleted_chars > 0 {
            self.buffer.delete(op.pos..op.pos + deleted_chars);
        }
        if !op.inserted.is_empty() {
            self.buffer.insert(op.pos, &op.inserted);
        }

        self.undo_stack.push(op);
        true
    }

    fn clear_history(&mut self) {
        self.undo_stack.clear();
        self.redo_stack.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::EditorRope;

    #[test]
    fn test_undoable_buffer_insert_undo() {
        let rope = EditorRope::from_str("hello");
        let mut buf = UndoableBuffer::new(rope, 100);

        assert_eq!(buf.to_string(), "hello");
        assert!(!buf.can_undo());

        // Insert " world"
        buf.insert(5, " world");
        assert_eq!(buf.to_string(), "hello world");
        assert!(buf.can_undo());

        // Undo
        assert!(buf.undo());
        assert_eq!(buf.to_string(), "hello");
        assert!(!buf.can_undo());
        assert!(buf.can_redo());

        // Redo
        assert!(buf.redo());
        assert_eq!(buf.to_string(), "hello world");
        assert!(buf.can_undo());
        assert!(!buf.can_redo());
    }

    #[test]
    fn test_undoable_buffer_delete_undo() {
        let rope = EditorRope::from_str("hello world");
        let mut buf = UndoableBuffer::new(rope, 100);

        // Delete " world"
        buf.delete(5..11);
        assert_eq!(buf.to_string(), "hello");
        assert!(buf.can_undo());

        // Undo
        assert!(buf.undo());
        assert_eq!(buf.to_string(), "hello world");
    }

    #[test]
    fn test_undoable_buffer_replace_undo() {
        let rope = EditorRope::from_str("hello world");
        let mut buf = UndoableBuffer::new(rope, 100);

        // Replace "world" with "rust"
        buf.delete(6..11);
        buf.insert(6, "rust");
        assert_eq!(buf.to_string(), "hello rust");

        // Undo insert
        assert!(buf.undo());
        assert_eq!(buf.to_string(), "hello ");

        // Undo delete
        assert!(buf.undo());
        assert_eq!(buf.to_string(), "hello world");
    }

    #[test]
    fn test_new_edit_clears_redo() {
        let rope = EditorRope::from_str("abc");
        let mut buf = UndoableBuffer::new(rope, 100);

        buf.insert(3, "d");
        assert!(buf.undo());
        assert!(buf.can_redo());

        // New edit should clear redo
        buf.insert(3, "e");
        assert!(!buf.can_redo());
    }

    #[test]
    fn test_max_steps() {
        let rope = EditorRope::from_str("");
        let mut buf = UndoableBuffer::new(rope, 3);

        buf.insert(0, "a");
        buf.insert(1, "b");
        buf.insert(2, "c");
        buf.insert(3, "d"); // should evict "a"

        assert_eq!(buf.to_string(), "abcd");

        // Should only be able to undo 3 times
        assert!(buf.undo()); // removes d
        assert!(buf.undo()); // removes c
        assert!(buf.undo()); // removes b
        assert!(!buf.undo()); // a was evicted

        assert_eq!(buf.to_string(), "a");
    }

    #[test]
    fn test_multiple_undo_redo_cycles() {
        let rope = EditorRope::from_str("");
        let mut buf = UndoableBuffer::new(rope, 100);

        buf.insert(0, "a");
        buf.insert(1, "b");
        buf.insert(2, "c");
        assert_eq!(buf.to_string(), "abc");

        // Undo all
        assert!(buf.undo());
        assert!(buf.undo());
        assert!(buf.undo());
        assert_eq!(buf.to_string(), "");

        // Redo all
        assert!(buf.redo());
        assert!(buf.redo());
        assert!(buf.redo());
        assert_eq!(buf.to_string(), "abc");

        // Partial undo then new edit
        assert!(buf.undo()); // "ab"
        assert!(buf.undo()); // "a"
        buf.insert(1, "x");
        assert_eq!(buf.to_string(), "ax");
        assert!(!buf.can_redo()); // redo cleared
    }
}
