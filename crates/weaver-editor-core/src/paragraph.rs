//! Paragraph-level rendering for incremental updates.
//!
//! Paragraphs are discovered during markdown rendering by tracking
//! Tag::Paragraph events. This allows updating only changed paragraphs in the DOM.

use smol_str::{SmolStr, format_smolstr};

use crate::offset_map::OffsetMapping;
use crate::syntax::SyntaxSpanInfo;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::ops::Range;

/// A rendered paragraph with its source range and offset mappings.
#[derive(Debug, Clone, PartialEq)]
pub struct ParagraphRender {
    /// Stable content-based ID for DOM diffing (format: `p-{index}`)
    pub id: SmolStr,

    /// Source byte range in the text buffer
    pub byte_range: Range<usize>,

    /// Source char range in the text buffer
    pub char_range: Range<usize>,

    /// Rendered HTML content (without wrapper div)
    pub html: String,

    /// Offset mappings for this paragraph
    pub offset_map: Vec<OffsetMapping>,

    /// Syntax spans for conditional visibility
    pub syntax_spans: Vec<SyntaxSpanInfo>,

    /// Hash of source text for quick change detection
    pub source_hash: u64,
}

impl ParagraphRender {
    /// Check if this paragraph contains a given byte offset.
    pub fn contains_byte(&self, offset: usize) -> bool {
        self.byte_range.contains(&offset)
    }

    /// Check if this paragraph contains a given char offset.
    pub fn contains_char(&self, offset: usize) -> bool {
        self.char_range.contains(&offset)
    }

    /// Get the length in chars.
    pub fn char_len(&self) -> usize {
        self.char_range.len()
    }

    /// Get the length in bytes.
    pub fn byte_len(&self) -> usize {
        self.byte_range.len()
    }
}

/// Simple hash function for source text comparison.
///
/// Used to quickly detect if paragraph content has changed.
pub fn hash_source(text: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

/// Generate a paragraph ID from monotonic counter.
///
/// IDs are stable across content changes - only position/cursor determines identity.
pub fn make_paragraph_id(index: usize) -> SmolStr {
    format_smolstr!("p-{}", index)
}

#[cfg(test)]
mod tests {
    use smol_str::ToSmolStr;

    use super::*;

    #[test]
    fn test_hash_source() {
        let h1 = hash_source("hello world");
        let h2 = hash_source("hello world");
        let h3 = hash_source("hello world!");

        assert_eq!(h1, h2);
        assert_ne!(h1, h3);
    }

    #[test]
    fn test_make_paragraph_id() {
        assert_eq!(make_paragraph_id(0), "p-0");
        assert_eq!(make_paragraph_id(42), "p-42");
    }

    #[test]
    fn test_paragraph_contains() {
        let para = ParagraphRender {
            id: "p-0".to_smolstr(),
            byte_range: 10..50,
            char_range: 10..50,
            html: String::new(),
            offset_map: vec![],
            syntax_spans: vec![],
            source_hash: 0,
        };

        assert!(!para.contains_byte(9));
        assert!(para.contains_byte(10));
        assert!(para.contains_byte(25));
        assert!(para.contains_byte(49));
        assert!(!para.contains_byte(50));

        assert!(!para.contains_char(9));
        assert!(para.contains_char(10));
        assert!(para.contains_char(25));
        assert!(!para.contains_char(50));
    }
}
