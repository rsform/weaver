//! Paragraph-level rendering for incremental updates.
//!
//! Paragraphs are discovered during markdown rendering by tracking
//! Tag::Paragraph events. This allows updating only changed paragraphs in the DOM.

use super::offset_map::OffsetMapping;
use super::writer::SyntaxSpanInfo;
use jumprope::JumpRopeBuf;
use std::ops::Range;

/// A rendered paragraph with its source range and offset mappings.
#[derive(Debug, Clone, PartialEq)]
pub struct ParagraphRender {
    /// Source byte range in the rope
    pub byte_range: Range<usize>,

    /// Source char range in the rope
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

/// Simple hash function for source text comparison
pub fn hash_source(text: &str) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

/// Extract substring from rope as String
pub fn rope_slice_to_string(rope: &JumpRopeBuf, range: Range<usize>) -> String {
    let rope_borrow = rope.borrow();
    let mut result = String::new();

    for substr in rope_borrow.slice_substrings(range) {
        result.push_str(substr);
    }

    result
}

