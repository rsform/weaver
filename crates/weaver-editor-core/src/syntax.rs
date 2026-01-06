//! Syntax span tracking for conditional visibility.
//!
//! Tracks markdown syntax characters (like `**`, `#`, `>`) so they can be
//! shown/hidden based on cursor position (Obsidian-style editing).

use std::ops::Range;

use smol_str::SmolStr;

/// Classification of markdown syntax characters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyntaxType {
    /// Inline formatting: **, *, ~~, `, $, [, ], (, )
    Inline,
    /// Block formatting: #, >, -, *, 1., ```, ---
    Block,
}

/// Information about a syntax span for conditional visibility.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyntaxSpanInfo {
    /// Unique identifier for this syntax span (e.g., "s0", "s1")
    pub syn_id: SmolStr,
    /// Source char range this syntax covers (just this marker)
    pub char_range: Range<usize>,
    /// Whether this is inline or block-level syntax
    pub syntax_type: SyntaxType,
    /// For paired inline syntax (**, *, etc), the full formatted region
    /// from opening marker through content to closing marker.
    /// When cursor is anywhere in this range, the syntax is visible.
    pub formatted_range: Option<Range<usize>>,
}

impl SyntaxSpanInfo {
    /// Adjust all position fields by a character delta.
    ///
    /// This adjusts both `char_range` and `formatted_range` (if present) together,
    /// ensuring they stay in sync. Use this instead of manually adjusting fields
    /// to avoid forgetting one.
    pub fn adjust_positions(&mut self, char_delta: isize) {
        self.char_range.start = (self.char_range.start as isize + char_delta) as usize;
        self.char_range.end = (self.char_range.end as isize + char_delta) as usize;
        if let Some(ref mut fr) = self.formatted_range {
            fr.start = (fr.start as isize + char_delta) as usize;
            fr.end = (fr.end as isize + char_delta) as usize;
        }
    }

    /// Check if cursor is within the visibility range for this syntax.
    ///
    /// Returns true if the cursor should cause this syntax to be visible.
    /// Uses `formatted_range` if present (for paired syntax like **bold**),
    /// otherwise uses `char_range` (for standalone syntax like # heading).
    pub fn cursor_in_range(&self, cursor_pos: usize) -> bool {
        let range = self.formatted_range.as_ref().unwrap_or(&self.char_range);
        cursor_pos >= range.start && cursor_pos <= range.end
    }
}

/// Classify syntax text as inline or block level.
pub fn classify_syntax(text: &str) -> SyntaxType {
    let trimmed = text.trim_start();

    // Check for block-level markers
    if trimmed.starts_with('#')
        || trimmed.starts_with('>')
        || trimmed.starts_with("```")
        || trimmed.starts_with("---")
        || (trimmed.starts_with('-')
            && trimmed
                .chars()
                .nth(1)
                .map(|c| c.is_whitespace())
                .unwrap_or(false))
        || (trimmed.starts_with('*')
            && trimmed
                .chars()
                .nth(1)
                .map(|c| c.is_whitespace())
                .unwrap_or(false))
        || trimmed
            .chars()
            .next()
            .map(|c| c.is_ascii_digit())
            .unwrap_or(false)
            && trimmed.contains('.')
    {
        SyntaxType::Block
    } else {
        SyntaxType::Inline
    }
}

#[cfg(test)]
mod tests {
    use smol_str::ToSmolStr;

    use super::*;

    #[test]
    fn test_classify_block_syntax() {
        assert_eq!(classify_syntax("# "), SyntaxType::Block);
        assert_eq!(classify_syntax("## "), SyntaxType::Block);
        assert_eq!(classify_syntax("> "), SyntaxType::Block);
        assert_eq!(classify_syntax("- "), SyntaxType::Block);
        assert_eq!(classify_syntax("* "), SyntaxType::Block);
        assert_eq!(classify_syntax("1. "), SyntaxType::Block);
        assert_eq!(classify_syntax("```"), SyntaxType::Block);
        assert_eq!(classify_syntax("---"), SyntaxType::Block);
    }

    #[test]
    fn test_classify_inline_syntax() {
        assert_eq!(classify_syntax("**"), SyntaxType::Inline);
        assert_eq!(classify_syntax("*"), SyntaxType::Inline);
        assert_eq!(classify_syntax("`"), SyntaxType::Inline);
        assert_eq!(classify_syntax("~~"), SyntaxType::Inline);
        assert_eq!(classify_syntax("["), SyntaxType::Inline);
        assert_eq!(classify_syntax("]("), SyntaxType::Inline);
    }

    #[test]
    fn test_adjust_positions() {
        let mut span = SyntaxSpanInfo {
            syn_id: "s0".to_smolstr(),
            char_range: 10..15,
            syntax_type: SyntaxType::Inline,
            formatted_range: Some(10..25),
        };

        span.adjust_positions(5);
        assert_eq!(span.char_range, 15..20);
        assert_eq!(span.formatted_range, Some(15..30));

        span.adjust_positions(-3);
        assert_eq!(span.char_range, 12..17);
        assert_eq!(span.formatted_range, Some(12..27));
    }

    #[test]
    fn test_cursor_in_range() {
        // Paired syntax with formatted_range
        let span = SyntaxSpanInfo {
            syn_id: "s0".to_smolstr(),
            char_range: 0..2, // **
            syntax_type: SyntaxType::Inline,
            formatted_range: Some(0..10), // **content**
        };

        assert!(span.cursor_in_range(0));
        assert!(span.cursor_in_range(5));
        assert!(span.cursor_in_range(10));
        assert!(!span.cursor_in_range(11));

        // Unpaired syntax without formatted_range
        let span = SyntaxSpanInfo {
            syn_id: "s1".to_smolstr(),
            char_range: 0..2, // ##
            syntax_type: SyntaxType::Block,
            formatted_range: None,
        };

        assert!(span.cursor_in_range(0));
        assert!(span.cursor_in_range(1));
        assert!(span.cursor_in_range(2));
        assert!(!span.cursor_in_range(3));
    }
}
