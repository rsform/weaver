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
    pub fn calculate(
        cursor_offset: usize,
        selection: Option<&Selection>,
        syntax_spans: &[SyntaxSpanInfo],
        paragraphs: &[ParagraphRender],
    ) -> Self {
        let mut visible = HashSet::new();

        for span in syntax_spans {
            // Find the paragraph containing this span for boundary clamping
            let para_bounds = find_paragraph_bounds(&span.char_range, paragraphs);

            let should_show = match span.syntax_type {
                SyntaxType::Inline => {
                    // Show if cursor within formatted span content OR adjacent to markers
                    // "Adjacent" means within 1 char of the syntax boundaries,
                    // clamped to paragraph bounds (paragraphs are split by newlines,
                    // so clamping to para bounds prevents cross-line extension)
                    let extended_start =
                        safe_extend_left(span.char_range.start, 1, para_bounds.as_ref());
                    let extended_end =
                        safe_extend_right(span.char_range.end, 1, para_bounds.as_ref());
                    let extended_range = extended_start..extended_end;

                    // Also show if cursor is anywhere in the formatted_range
                    // (the region between paired opening/closing markers)
                    // Extend by 1 char on BOTH sides for symmetric "approaching" behavior,
                    // clamped to paragraph bounds.
                    let in_formatted_region = span
                        .formatted_range
                        .as_ref()
                        .map(|r| {
                            let ext_start = safe_extend_left(r.start, 1, para_bounds.as_ref());
                            let ext_end = safe_extend_right(r.end, 1, para_bounds.as_ref());
                            cursor_offset >= ext_start && cursor_offset <= ext_end
                        })
                        .unwrap_or(false);

                    let in_extended = extended_range.contains(&cursor_offset);
                    let result = in_extended
                        || in_formatted_region
                        || selection_overlaps(selection, &span.char_range)
                        || span
                            .formatted_range
                            .as_ref()
                            .map(|r| selection_overlaps(selection, r))
                            .unwrap_or(false);

                    result
                }
                SyntaxType::Block => {
                    // Show if cursor anywhere in same paragraph (with slop for edge cases)
                    // The slop handles typing at the end of a heading like "# |"
                    let para_bounds = find_paragraph_bounds(&span.char_range, paragraphs);
                    let in_paragraph = para_bounds
                        .as_ref()
                        .map(|p| {
                            // Extend paragraph bounds by 1 char on each side for slop
                            let ext_start = p.start.saturating_sub(1);
                            let ext_end = p.end.saturating_add(1);
                            cursor_offset >= ext_start && cursor_offset <= ext_end
                        })
                        .unwrap_or(false);

                    in_paragraph || selection_overlaps(selection, &span.char_range)
                }
            };

            if should_show {
                visible.insert(span.syn_id.clone());
            }
        }

        tracing::debug!(
            target: "weaver::visibility",
            cursor_offset,
            total_spans = syntax_spans.len(),
            visible_count = visible.len(),
            "calculated visibility"
        );

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
#[allow(dead_code)]
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

/// Find the paragraph bounds containing a syntax span.
fn find_paragraph_bounds(
    syntax_range: &Range<usize>,
    paragraphs: &[ParagraphRender],
) -> Option<Range<usize>> {
    for para in paragraphs {
        // Skip gap paragraphs
        if para.syntax_spans.is_empty() && !para.char_range.is_empty() {
            continue;
        }

        if para.char_range.start <= syntax_range.start && syntax_range.end <= para.char_range.end {
            return Some(para.char_range.clone());
        }
    }
    None
}

/// Safely extend a position leftward by `amount` chars, clamped to paragraph bounds.
///
/// Paragraphs are already split by newlines, so clamping to paragraph bounds
/// naturally prevents extending across line boundaries.
fn safe_extend_left(pos: usize, amount: usize, para_bounds: Option<&Range<usize>>) -> usize {
    let min_pos = para_bounds.map(|p| p.start).unwrap_or(0);
    pos.saturating_sub(amount).max(min_pos)
}

/// Safely extend a position rightward by `amount` chars, clamped to paragraph bounds.
///
/// Paragraphs are already split by newlines, so clamping to paragraph bounds
/// naturally prevents extending across line boundaries.
fn safe_extend_right(pos: usize, amount: usize, para_bounds: Option<&Range<usize>>) -> usize {
    let max_pos = para_bounds.map(|p| p.end).unwrap_or(usize::MAX);
    pos.saturating_add(amount).min(max_pos)
}

/// Update syntax span visibility in the DOM based on cursor position.
///
/// Toggles the "hidden" class on syntax spans based on calculated visibility.
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub fn update_syntax_visibility(
    cursor_offset: usize,
    selection: Option<&Selection>,
    syntax_spans: &[SyntaxSpanInfo],
    paragraphs: &[ParagraphRender],
) {
    use wasm_bindgen::JsCast;

    let visibility = VisibilityState::calculate(cursor_offset, selection, syntax_spans, paragraphs);

    let Some(window) = web_sys::window() else {
        return;
    };
    let Some(document) = window.document() else {
        return;
    };

    // Single querySelectorAll instead of N individual queries
    let Ok(node_list) = document.query_selector_all("[data-syn-id]") else {
        return;
    };

    for i in 0..node_list.length() {
        let Some(node) = node_list.item(i) else {
            continue;
        };

        // Cast to Element to access attributes and class_list
        let Some(element) = node.dyn_ref::<web_sys::Element>() else {
            continue;
        };

        let Some(syn_id) = element.get_attribute("data-syn-id") else {
            continue;
        };

        let class_list = element.class_list();
        if visibility.is_visible(&syn_id) {
            let _ = class_list.remove_1("hidden");
        } else {
            let _ = class_list.add_1("hidden");
        }
    }
}

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub fn update_syntax_visibility(
    _cursor_offset: usize,
    _selection: Option<&Selection>,
    _syntax_spans: &[SyntaxSpanInfo],
    _paragraphs: &[ParagraphRender],
) {
    // No-op on non-wasm
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_span(
        syn_id: &str,
        start: usize,
        end: usize,
        syntax_type: SyntaxType,
    ) -> SyntaxSpanInfo {
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
            id: format!("test-{}-{}", start, end),
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
        assert!(
            vis.is_visible("s0"),
            "opening ** should be visible when cursor inside formatted region"
        );
        assert!(
            vis.is_visible("s1"),
            "closing ** should be visible when cursor inside formatted region"
        );

        // Cursor at position 2 (adjacent to opening **, start of "bold")
        let vis = VisibilityState::calculate(2, None, &spans, &paras);
        assert!(
            vis.is_visible("s0"),
            "opening ** should be visible when cursor adjacent at start of bold"
        );

        // Cursor at position 5 (adjacent to closing **, end of "bold")
        let vis = VisibilityState::calculate(5, None, &spans, &paras);
        assert!(
            vis.is_visible("s1"),
            "closing ** should be visible when cursor adjacent at end of bold"
        );
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
        assert!(
            !vis.is_visible("s0"),
            "opening ** should be hidden when no formatted_range and cursor not adjacent"
        );
        assert!(
            !vis.is_visible("s1"),
            "closing ** should be hidden when no formatted_range and cursor not adjacent"
        );
    }

    #[test]
    fn test_inline_visibility_cursor_adjacent() {
        // "test **bold** after"
        //       5  7
        let spans = vec![
            make_span("s0", 5, 7, SyntaxType::Inline), // ** at positions 5-6
        ];
        let paras = vec![make_para(0, 19, spans.clone())];

        // Cursor at position 4 (one before ** which starts at 5)
        let vis = VisibilityState::calculate(4, None, &spans, &paras);
        assert!(
            vis.is_visible("s0"),
            "** should be visible when cursor adjacent"
        );

        // Cursor at position 7 (one after ** which ends at 6, since range is exclusive)
        let vis = VisibilityState::calculate(7, None, &spans, &paras);
        assert!(
            vis.is_visible("s0"),
            "** should be visible when cursor adjacent after span"
        );
    }

    #[test]
    fn test_inline_visibility_cursor_far() {
        let spans = vec![make_span("s0", 10, 12, SyntaxType::Inline)];
        let paras = vec![make_para(0, 33, spans.clone())];

        // Cursor at position 0 (far from **)
        let vis = VisibilityState::calculate(0, None, &spans, &paras);
        assert!(
            !vis.is_visible("s0"),
            "** should be hidden when cursor far away"
        );
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
        assert!(
            vis.is_visible("s0"),
            "# should be visible when cursor in same paragraph"
        );
    }

    #[test]
    fn test_block_visibility_different_paragraph() {
        let spans = vec![make_span("s0", 0, 2, SyntaxType::Block)];
        let paras = vec![make_para(0, 10, spans.clone()), make_para(12, 30, vec![])];

        // Cursor at position 20 (in second paragraph)
        let vis = VisibilityState::calculate(20, None, &spans, &paras);
        assert!(
            !vis.is_visible("s0"),
            "# should be hidden when cursor in different paragraph"
        );
    }

    #[test]
    fn test_selection_reveals_syntax() {
        let spans = vec![make_span("s0", 5, 7, SyntaxType::Inline)];
        let paras = vec![make_para(0, 24, spans.clone())];

        // Selection overlaps the syntax span
        let selection = Selection {
            anchor: 3,
            head: 10,
        };
        let vis = VisibilityState::calculate(10, Some(&selection), &spans, &paras);
        assert!(
            vis.is_visible("s0"),
            "** should be visible when selection overlaps"
        );
    }

    #[test]
    fn test_paragraph_boundary_blocks_extension() {
        // Cursor in paragraph 2 should NOT reveal syntax in paragraph 1,
        // even if cursor is only 1 char after the paragraph boundary
        // (paragraph bounds clamp the extension)
        let spans = vec![
            make_span_with_range("s0", 0, 2, SyntaxType::Inline, 0..8), // opening **
            make_span_with_range("s1", 6, 8, SyntaxType::Inline, 0..8), // closing **
        ];
        let paras = vec![
            make_para(0, 8, spans.clone()), // "**bold**"
            make_para(9, 13, vec![]),       // "text" (after newline)
        ];

        // Cursor at position 9 (start of second paragraph)
        // Should NOT reveal the closing ** because para bounds clamp extension
        let vis = VisibilityState::calculate(9, None, &spans, &paras);
        assert!(
            !vis.is_visible("s1"),
            "closing ** should NOT be visible when cursor is in next paragraph"
        );
    }

    #[test]
    fn test_extension_clamps_to_paragraph() {
        // Syntax at very start of paragraph - extension left should stop at para start
        let spans = vec![make_span_with_range("s0", 0, 2, SyntaxType::Inline, 0..8)];
        let paras = vec![make_para(0, 8, spans.clone())];

        // Cursor at position 0 - should still see the opening **
        let vis = VisibilityState::calculate(0, None, &spans, &paras);
        assert!(
            vis.is_visible("s0"),
            "** at start should be visible when cursor at position 0"
        );
    }
}
