//! Conditional syntax visibility based on cursor position.
//!
//! Implements Obsidian-style formatting character visibility: syntax markers
//! are hidden when cursor is not near them, revealed when cursor approaches.

use super::document::Selection;
use super::paragraph::ParagraphRender;
use super::writer::{SyntaxSpanInfo, SyntaxType};
use std::collections::HashSet;
use std::ops::Range;

/// Determines which syntax spans should be visible based on cursor/selection.
#[derive(Debug, Clone, Default)]
pub struct VisibilityState {
    /// Set of syn_ids that should be visible
    pub visible_span_ids: HashSet<String>,
}

impl VisibilityState {
    /// Calculate visibility based on cursor position and selection.
    ///
    /// # Arguments
    /// - `cursor_offset`: Current cursor position (char offset)
    /// - `selection`: Optional selection range
    /// - `syntax_spans`: All syntax spans in the document
    /// - `paragraphs`: All paragraphs (for block-level visibility lookup)
    pub fn calculate(
        cursor_offset: usize,
        selection: Option<&Selection>,
        syntax_spans: &[SyntaxSpanInfo],
        paragraphs: &[ParagraphRender],
    ) -> Self {
        let mut visible = HashSet::new();

        for span in syntax_spans {
            let should_show = match span.syntax_type {
                SyntaxType::Inline => {
                    // Show if cursor within formatted span content OR adjacent to markers
                    // "Adjacent" means within 1 char of the syntax boundaries
                    let extended_range = span.char_range.start.saturating_sub(1)
                        ..span.char_range.end.saturating_add(1);

                    // Also show if cursor is anywhere in the formatted_range
                    // (the region between paired opening/closing markers)
                    let in_formatted_region = span
                        .formatted_range
                        .as_ref()
                        .map(|r| r.contains(&cursor_offset))
                        .unwrap_or(false);

                    extended_range.contains(&cursor_offset)
                        || in_formatted_region
                        || selection_overlaps(selection, &span.char_range)
                        || span
                            .formatted_range
                            .as_ref()
                            .map(|r| selection_overlaps(selection, r))
                            .unwrap_or(false)
                }
                SyntaxType::Block => {
                    // Show if cursor anywhere in same paragraph
                    cursor_in_same_paragraph(cursor_offset, &span.char_range, paragraphs)
                        || selection_overlaps(selection, &span.char_range)
                }
            };

            if should_show {
                visible.insert(span.syn_id.clone());
            }
        }

        Self {
            visible_span_ids: visible,
        }
    }

    /// Check if a specific span should be visible.
    pub fn is_visible(&self, syn_id: &str) -> bool {
        self.visible_span_ids.contains(syn_id)
    }
}

/// Check if selection overlaps with a char range.
fn selection_overlaps(selection: Option<&Selection>, range: &Range<usize>) -> bool {
    let Some(sel) = selection else {
        return false;
    };

    let sel_start = sel.anchor.min(sel.head);
    let sel_end = sel.anchor.max(sel.head);

    // Check if ranges overlap
    sel_start < range.end && sel_end > range.start
}

/// Check if cursor is in the same paragraph as a syntax span.
fn cursor_in_same_paragraph(
    cursor_offset: usize,
    syntax_range: &Range<usize>,
    paragraphs: &[ParagraphRender],
) -> bool {
    // Find which paragraph contains the syntax span
    for para in paragraphs {
        // Skip gap paragraphs (they have no syntax spans)
        if para.syntax_spans.is_empty() && !para.char_range.is_empty() {
            continue;
        }

        // Check if this paragraph contains the syntax span
        if para.char_range.start <= syntax_range.start && syntax_range.end <= para.char_range.end {
            // Check if cursor is also in this paragraph
            return para.char_range.contains(&cursor_offset);
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_span(syn_id: &str, start: usize, end: usize, syntax_type: SyntaxType) -> SyntaxSpanInfo {
        SyntaxSpanInfo {
            syn_id: syn_id.to_string(),
            char_range: start..end,
            syntax_type,
            formatted_range: None,
        }
    }

    fn make_span_with_range(
        syn_id: &str,
        start: usize,
        end: usize,
        syntax_type: SyntaxType,
        formatted_range: Range<usize>,
    ) -> SyntaxSpanInfo {
        SyntaxSpanInfo {
            syn_id: syn_id.to_string(),
            char_range: start..end,
            syntax_type,
            formatted_range: Some(formatted_range),
        }
    }

    fn make_para(start: usize, end: usize, syntax_spans: Vec<SyntaxSpanInfo>) -> ParagraphRender {
        ParagraphRender {
            byte_range: start..end,
            char_range: start..end,
            html: String::new(),
            offset_map: vec![],
            syntax_spans,
            source_hash: 0,
        }
    }

    #[test]
    fn test_inline_visibility_cursor_inside() {
        // **bold** at chars 0-2 (opening **) and 6-8 (closing **)
        // Text positions: 0-1 = **, 2-5 = bold, 6-7 = **
        // formatted_range is 0..8 (the whole **bold** region)
        let spans = vec![
            make_span_with_range("s0", 0, 2, SyntaxType::Inline, 0..8), // opening **
            make_span_with_range("s1", 6, 8, SyntaxType::Inline, 0..8), // closing **
        ];
        let paras = vec![make_para(0, 8, spans.clone())];

        // Cursor at position 4 (middle of "bold", inside formatted region)
        let vis = VisibilityState::calculate(4, None, &spans, &paras);
        assert!(vis.is_visible("s0"), "opening ** should be visible when cursor inside formatted region");
        assert!(vis.is_visible("s1"), "closing ** should be visible when cursor inside formatted region");

        // Cursor at position 2 (adjacent to opening **, start of "bold")
        let vis = VisibilityState::calculate(2, None, &spans, &paras);
        assert!(vis.is_visible("s0"), "opening ** should be visible when cursor adjacent at start of bold");

        // Cursor at position 5 (adjacent to closing **, end of "bold")
        let vis = VisibilityState::calculate(5, None, &spans, &paras);
        assert!(vis.is_visible("s1"), "closing ** should be visible when cursor adjacent at end of bold");
    }

    #[test]
    fn test_inline_visibility_without_formatted_range() {
        // Test without formatted_range - just adjacency-based visibility
        let spans = vec![
            make_span("s0", 0, 2, SyntaxType::Inline), // opening ** (no formatted_range)
            make_span("s1", 6, 8, SyntaxType::Inline), // closing ** (no formatted_range)
        ];
        let paras = vec![make_para(0, 8, spans.clone())];

        // Cursor at position 4 (middle of "bold", not adjacent to either marker)
        let vis = VisibilityState::calculate(4, None, &spans, &paras);
        assert!(!vis.is_visible("s0"), "opening ** should be hidden when no formatted_range and cursor not adjacent");
        assert!(!vis.is_visible("s1"), "closing ** should be hidden when no formatted_range and cursor not adjacent");
    }

    #[test]
    fn test_inline_visibility_cursor_adjacent() {
        let spans = vec![
            make_span("s0", 5, 7, SyntaxType::Inline), // ** at positions 5-6
        ];
        let paras = vec![make_para(0, 20, spans.clone())];

        // Cursor at position 4 (one before ** which starts at 5)
        let vis = VisibilityState::calculate(4, None, &spans, &paras);
        assert!(vis.is_visible("s0"), "** should be visible when cursor adjacent");

        // Cursor at position 7 (one after ** which ends at 6, since range is exclusive)
        let vis = VisibilityState::calculate(7, None, &spans, &paras);
        assert!(vis.is_visible("s0"), "** should be visible when cursor adjacent after span");
    }

    #[test]
    fn test_inline_visibility_cursor_far() {
        let spans = vec![
            make_span("s0", 10, 12, SyntaxType::Inline),
        ];
        let paras = vec![make_para(0, 30, spans.clone())];

        // Cursor at position 0 (far from **)
        let vis = VisibilityState::calculate(0, None, &spans, &paras);
        assert!(!vis.is_visible("s0"), "** should be hidden when cursor far away");
    }

    #[test]
    fn test_block_visibility_same_paragraph() {
        // # at start of heading
        let spans = vec![
            make_span("s0", 0, 2, SyntaxType::Block), // "# "
        ];
        let paras = vec![
            make_para(0, 10, spans.clone()), // heading paragraph
            make_para(12, 30, vec![]),       // next paragraph
        ];

        // Cursor at position 5 (inside heading)
        let vis = VisibilityState::calculate(5, None, &spans, &paras);
        assert!(vis.is_visible("s0"), "# should be visible when cursor in same paragraph");
    }

    #[test]
    fn test_block_visibility_different_paragraph() {
        let spans = vec![
            make_span("s0", 0, 2, SyntaxType::Block),
        ];
        let paras = vec![
            make_para(0, 10, spans.clone()),
            make_para(12, 30, vec![]),
        ];

        // Cursor at position 20 (in second paragraph)
        let vis = VisibilityState::calculate(20, None, &spans, &paras);
        assert!(!vis.is_visible("s0"), "# should be hidden when cursor in different paragraph");
    }

    #[test]
    fn test_selection_reveals_syntax() {
        let spans = vec![
            make_span("s0", 5, 7, SyntaxType::Inline),
        ];
        let paras = vec![make_para(0, 20, spans.clone())];

        // Selection overlaps the syntax span
        let selection = Selection { anchor: 3, head: 10 };
        let vis = VisibilityState::calculate(10, Some(&selection), &spans, &paras);
        assert!(vis.is_visible("s0"), "** should be visible when selection overlaps");
    }
}
