//! Paragraph-level rendering for incremental updates.
//!
//! Re-exports core types and provides Loro-specific helpers.

use loro::LoroText;
use std::ops::Range;

// Re-export core types.
pub use weaver_editor_core::{ParagraphRender, hash_source, make_paragraph_id};

/// Extract substring from LoroText as String.
pub fn text_slice_to_string(text: &LoroText, range: Range<usize>) -> String {
    text.slice(range.start, range.end).unwrap_or_default()
}
