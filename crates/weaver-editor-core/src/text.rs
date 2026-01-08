//! Text buffer abstraction for editor storage.
//!
//! The `TextBuffer` trait provides a common interface for text storage,
//! allowing the editor to work with different backends (ropey for local,
//! Loro for CRDT collaboration).

use smol_str::{SmolStr, ToSmolStr};
use std::ops::Range;
use web_time::Instant;

use crate::types::{EditInfo, BLOCK_SYNTAX_ZONE};

/// A text buffer that supports efficient editing and offset conversion.
///
/// All offsets are in Unicode scalar values (chars), not bytes or UTF-16.
pub trait TextBuffer {
    /// Total length in bytes (UTF-8).
    fn len_bytes(&self) -> usize;

    /// Total length in chars (Unicode scalar values).
    fn len_chars(&self) -> usize;

    /// Check if empty.
    fn is_empty(&self) -> bool {
        self.len_chars() == 0
    }

    /// Insert text at char offset.
    fn insert(&mut self, char_offset: usize, text: &str);

    /// Append text at end.
    ///
    /// Default implementation calls insert at len_chars(). Override if
    /// the underlying buffer has a more efficient append operation.
    fn push(&mut self, text: &str) {
        self.insert(self.len_chars(), text);
    }

    /// Delete char range.
    fn delete(&mut self, char_range: Range<usize>);

    /// Replace char range with text.
    fn replace(&mut self, char_range: Range<usize>, text: &str) {
        self.delete(char_range.clone());
        self.insert(char_range.start, text);
    }

    /// Get a slice as SmolStr. Returns None if range is invalid.
    ///
    /// SmolStr is used for efficiency: strings ≤23 bytes are stored inline
    /// (no heap allocation), longer strings are Arc'd (cheap to clone).
    fn slice(&self, char_range: Range<usize>) -> Option<SmolStr>;

    /// Get character at offset. Returns None if out of bounds.
    fn char_at(&self, char_offset: usize) -> Option<char>;

    /// Convert entire buffer to String.
    fn to_string(&self) -> String;

    /// Convert char offset to byte offset.
    fn char_to_byte(&self, char_offset: usize) -> usize;

    /// Convert byte offset to char offset.
    fn byte_to_char(&self, byte_offset: usize) -> usize;

    /// Get info about the last edit operation, if any.
    fn last_edit(&self) -> Option<EditInfo>;

    /// Check if a char offset is in the block-syntax zone (first few chars of a line).
    fn is_in_block_syntax_zone(&self, offset: usize) -> bool {
        if offset <= BLOCK_SYNTAX_ZONE {
            return true;
        }

        // Get slice of the search range and look for newline.
        let search_start = offset.saturating_sub(BLOCK_SYNTAX_ZONE + 1);
        match self.slice(search_start..offset) {
            Some(s) => match s.rfind('\n') {
                Some(pos) => {
                    // Distance from character after newline to current offset
                    let newline_abs_pos = search_start + pos;
                    let dist = offset.saturating_sub(newline_abs_pos + 1);
                    dist <= BLOCK_SYNTAX_ZONE
                }
                None => false, // No newline in range, offset > BLOCK_SYNTAX_ZONE.
            },
            None => false,
        }
    }
}

/// Ropey-backed text buffer for local editing.
///
/// Provides O(log n) editing operations and offset conversions.
#[derive(Clone)]
pub struct EditorRope {
    rope: ropey::Rope,
    last_edit: Option<EditInfo>,
}

impl Default for EditorRope {
    fn default() -> Self {
        Self {
            rope: ropey::Rope::default(),
            last_edit: None,
        }
    }
}

impl EditorRope {
    /// Create a new empty rope.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create from string.
    pub fn from_str(s: &str) -> Self {
        Self {
            rope: ropey::Rope::from_str(s),
            last_edit: None,
        }
    }

    /// Get a reference to the underlying rope (for advanced operations).
    pub fn rope(&self) -> &ropey::Rope {
        &self.rope
    }

    /// Get a rope slice for zero-copy iteration over chunks.
    ///
    /// Use this when you need to iterate over the text without allocating,
    /// e.g., for hashing or character-by-character processing.
    pub fn rope_slice(&self, char_range: Range<usize>) -> Option<ropey::RopeSlice<'_>> {
        if char_range.end > self.rope.len_chars() {
            return None;
        }
        Some(self.rope.slice(char_range))
    }
}

impl TextBuffer for EditorRope {
    fn len_bytes(&self) -> usize {
        self.rope.len_bytes()
    }

    fn len_chars(&self) -> usize {
        self.rope.len_chars()
    }

    fn insert(&mut self, char_offset: usize, text: &str) {
        let in_block_syntax_zone = self.is_in_block_syntax_zone(char_offset);
        let contains_newline = text.contains('\n');

        self.rope.insert(char_offset, text);

        self.last_edit = Some(EditInfo {
            edit_char_pos: char_offset,
            inserted_len: text.chars().count(),
            deleted_len: 0,
            contains_newline,
            in_block_syntax_zone,
            doc_len_after: self.rope.len_chars(),
            timestamp: Instant::now(),
        });
    }

    // Ropey's insert is O(log n) regardless of position, so push is the same.
    // Override for consistency with trait.
    fn push(&mut self, text: &str) {
        self.insert(self.rope.len_chars(), text);
    }

    fn delete(&mut self, char_range: Range<usize>) {
        let in_block_syntax_zone = self.is_in_block_syntax_zone(char_range.start);
        let contains_newline = self
            .slice(char_range.clone())
            .map(|s| s.contains('\n'))
            .unwrap_or(false);
        let deleted_len = char_range.len();

        self.rope.remove(char_range.clone());

        self.last_edit = Some(EditInfo {
            edit_char_pos: char_range.start,
            inserted_len: 0,
            deleted_len,
            contains_newline,
            in_block_syntax_zone,
            doc_len_after: self.rope.len_chars(),
            timestamp: Instant::now(),
        });
    }

    fn slice(&self, char_range: Range<usize>) -> Option<SmolStr> {
        if char_range.end > self.len_chars() {
            return None;
        }
        Some(self.rope.slice(char_range).to_smolstr())
    }

    fn char_at(&self, char_offset: usize) -> Option<char> {
        if char_offset >= self.len_chars() {
            return None;
        }
        Some(self.rope.char(char_offset))
    }

    fn to_string(&self) -> String {
        self.rope.to_string()
    }

    fn char_to_byte(&self, char_offset: usize) -> usize {
        self.rope.char_to_byte(char_offset)
    }

    fn byte_to_char(&self, byte_offset: usize) -> usize {
        self.rope.byte_to_char(byte_offset)
    }

    fn last_edit(&self) -> Option<EditInfo> {
        self.last_edit
    }

    fn is_in_block_syntax_zone(&self, offset: usize) -> bool {
        if offset > self.rope.len_chars() {
            return false;
        }
        let line_num = self.rope.char_to_line(offset);
        let line_start = self.rope.line_to_char(line_num);
        (offset - line_start) <= BLOCK_SYNTAX_ZONE
    }
}

impl From<&str> for EditorRope {
    fn from(s: &str) -> Self {
        Self::from_str(s)
    }
}

impl From<String> for EditorRope {
    fn from(s: String) -> Self {
        Self::from_str(&s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_operations() {
        let mut rope = EditorRope::from_str("hello world");
        assert_eq!(rope.len_chars(), 11);
        assert_eq!(rope.to_string(), "hello world");

        rope.insert(5, " beautiful");
        assert_eq!(rope.to_string(), "hello beautiful world");

        // " beautiful" is 10 chars at positions 5..15
        rope.delete(5..15);
        assert_eq!(rope.to_string(), "hello world");
    }

    #[test]
    fn test_char_at() {
        let rope = EditorRope::from_str("hello");
        assert_eq!(rope.char_at(0), Some('h'));
        assert_eq!(rope.char_at(4), Some('o'));
        assert_eq!(rope.char_at(5), None);
    }

    #[test]
    fn test_slice() {
        let rope = EditorRope::from_str("hello world");
        assert_eq!(rope.slice(0..5).as_deref(), Some("hello"));
        assert_eq!(rope.slice(6..11).as_deref(), Some("world"));
        assert_eq!(rope.slice(0..100), None);
    }

    #[test]
    fn test_offset_conversion() {
        // "hello 🌍" - emoji is 4 bytes, 1 char
        let rope = EditorRope::from_str("hello 🌍");
        assert_eq!(rope.len_chars(), 7); // h e l l o   🌍
        assert_eq!(rope.len_bytes(), 10); // 6 + 4

        assert_eq!(rope.char_to_byte(6), 6); // before emoji
        assert_eq!(rope.char_to_byte(7), 10); // after emoji
        assert_eq!(rope.byte_to_char(6), 6);
        assert_eq!(rope.byte_to_char(10), 7);
    }

    #[test]
    fn test_replace() {
        let mut rope = EditorRope::from_str("hello world");
        rope.replace(6..11, "rust");
        assert_eq!(rope.to_string(), "hello rust");
    }
}
