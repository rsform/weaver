//! Snapshot tests for the markdown editor rendering pipeline.

use super::offset_map::{OffsetMapping, find_mapping_for_char};
use super::paragraph::ParagraphRender;
use super::render::render_paragraphs_incremental;
use loro::LoroDoc;
use serde::Serialize;

/// Serializable version of ParagraphRender for snapshot testing.
#[derive(Debug, Serialize)]
struct TestParagraph {
    byte_range: (usize, usize),
    char_range: (usize, usize),
    html: String,
    offset_map: Vec<TestOffsetMapping>,
    source_hash: u64,
}

impl From<&ParagraphRender> for TestParagraph {
    fn from(p: &ParagraphRender) -> Self {
        TestParagraph {
            byte_range: (p.byte_range.start, p.byte_range.end),
            char_range: (p.char_range.start, p.char_range.end),
            html: p.html.clone(),
            offset_map: p.offset_map.iter().map(TestOffsetMapping::from).collect(),
            source_hash: p.source_hash,
        }
    }
}

/// Serializable version of OffsetMapping for snapshot testing.
#[derive(Debug, Serialize)]
struct TestOffsetMapping {
    byte_range: (usize, usize),
    char_range: (usize, usize),
    node_id: String,
    char_offset_in_node: usize,
    child_index: Option<usize>,
    utf16_len: usize,
}

impl From<&OffsetMapping> for TestOffsetMapping {
    fn from(m: &OffsetMapping) -> Self {
        TestOffsetMapping {
            byte_range: (m.byte_range.start, m.byte_range.end),
            char_range: (m.char_range.start, m.char_range.end),
            node_id: m.node_id.clone(),
            char_offset_in_node: m.char_offset_in_node,
            child_index: m.child_index,
            utf16_len: m.utf16_len,
        }
    }
}

/// Helper: render markdown and convert to serializable test output.
fn render_test(input: &str) -> Vec<TestParagraph> {
    let doc = LoroDoc::new();
    let text = doc.get_text("content");
    text.insert(0, input).unwrap();
    let (paragraphs, _cache) = render_paragraphs_incremental(&text, None, None);
    paragraphs.iter().map(TestParagraph::from).collect()
}

// =============================================================================
// Basic Paragraph Tests
// =============================================================================

#[test]
fn test_single_paragraph() {
    let result = render_test("Hello world");
    insta::assert_yaml_snapshot!(result);
}

#[test]
fn test_two_paragraphs() {
    let result = render_test("First paragraph.\n\nSecond paragraph.");
    insta::assert_yaml_snapshot!(result);
}

#[test]
fn test_three_paragraphs() {
    let result = render_test("One.\n\nTwo.\n\nThree.");
    insta::assert_yaml_snapshot!(result);
}

// =============================================================================
// Block Element Tests
// =============================================================================

#[test]
fn test_heading_h1() {
    let result = render_test("# Heading 1");
    insta::assert_yaml_snapshot!(result);
}

#[test]
fn test_heading_levels() {
    let result = render_test("# H1\n\n## H2\n\n### H3\n\n#### H4");
    insta::assert_yaml_snapshot!(result);
}

#[test]
fn test_code_block_fenced() {
    let result = render_test("```rust\nfn main() {}\n```");
    insta::assert_yaml_snapshot!(result);
}

#[test]
fn test_unordered_list() {
    let result = render_test("- Item 1\n- Item 2\n- Item 3");
    insta::assert_yaml_snapshot!(result);
}

#[test]
fn test_ordered_list() {
    let result = render_test("1. First\n2. Second\n3. Third");
    insta::assert_yaml_snapshot!(result);
}

#[test]
fn test_nested_list() {
    let result = render_test("- Parent\n  - Child 1\n  - Child 2\n- Another parent");
    insta::assert_yaml_snapshot!(result);
}

#[test]
fn test_blockquote() {
    let result = render_test("> This is a quote\n>\n> With multiple lines");
    insta::assert_yaml_snapshot!(result);
}

// =============================================================================
// Inline Formatting Tests
// =============================================================================

#[test]
fn test_bold() {
    let result = render_test("Some **bold** text");
    insta::assert_yaml_snapshot!(result);
}

#[test]
fn test_italic() {
    let result = render_test("Some *italic* text");
    insta::assert_yaml_snapshot!(result);
}

#[test]
fn test_inline_code() {
    let result = render_test("Some `code` here");
    insta::assert_yaml_snapshot!(result);
}

#[test]
fn test_bold_italic() {
    let result = render_test("Some ***bold italic*** text");
    insta::assert_yaml_snapshot!(result);
}

#[test]
fn test_multiple_inline_formats() {
    let result = render_test("**Bold** and *italic* and `code`");
    insta::assert_yaml_snapshot!(result);
}

// =============================================================================
// Gap Paragraph Tests
// =============================================================================

#[test]
fn test_gap_between_blocks() {
    // Verify gap paragraphs are inserted for whitespace between blocks
    let result = render_test("# Heading\n\nParagraph below");
    // Should have: heading, gap for \n\n, paragraph
    insta::assert_yaml_snapshot!(result);
}

#[test]
fn test_multiple_blank_lines() {
    let result = render_test("First\n\n\n\nSecond");
    // Extra blank lines should be captured in gap paragraphs
    insta::assert_yaml_snapshot!(result);
}

// =============================================================================
// Edge Case Tests
// =============================================================================

#[test]
fn test_empty_document() {
    let result = render_test("");
    insta::assert_yaml_snapshot!(result);
}

#[test]
fn test_only_newlines() {
    let result = render_test("\n\n\n");
    insta::assert_yaml_snapshot!(result);
}

#[test]
fn test_trailing_single_newline() {
    let result = render_test("Hello\n");
    insta::assert_yaml_snapshot!(result);
}

#[test]
fn test_trailing_double_newline() {
    let result = render_test("Hello\n\n");
    insta::assert_yaml_snapshot!(result);
}

#[test]
fn test_hard_break() {
    // Two trailing spaces + newline = hard break
    let result = render_test("Line one  \nLine two");
    insta::assert_yaml_snapshot!(result);
}

#[test]
fn test_unicode_emoji() {
    let result = render_test("Hello 🎉 world");
    insta::assert_yaml_snapshot!(result);
}

#[test]
fn test_unicode_cjk() {
    let result = render_test("你好世界");
    insta::assert_yaml_snapshot!(result);
}

#[test]
fn test_mixed_unicode_ascii() {
    let result = render_test("Hello 你好 world 🎉");
    insta::assert_yaml_snapshot!(result);
}

// =============================================================================
// Offset Map Lookup Tests
// =============================================================================

#[test]
fn test_find_mapping_exact_start() {
    let mappings = vec![OffsetMapping {
        byte_range: 0..5,
        char_range: 0..5,
        node_id: "n0".to_string(),
        char_offset_in_node: 0,
        child_index: None,
        utf16_len: 5,
    }];

    let result = find_mapping_for_char(&mappings, 0);
    assert!(result.is_some());
    let (mapping, _) = result.unwrap();
    assert_eq!(mapping.char_range, 0..5);
}

#[test]
fn test_find_mapping_exact_end_inclusive() {
    // Bug #1 regression: cursor at end of range should match
    let mappings = vec![OffsetMapping {
        byte_range: 0..5,
        char_range: 0..5,
        node_id: "n0".to_string(),
        char_offset_in_node: 0,
        child_index: None,
        utf16_len: 5,
    }];

    // Position 5 should match the range 0..5 (end-inclusive for cursor)
    let result = find_mapping_for_char(&mappings, 5);
    assert!(result.is_some(), "cursor at end of range should match");
}

#[test]
fn test_find_mapping_middle() {
    let mappings = vec![OffsetMapping {
        byte_range: 0..10,
        char_range: 0..10,
        node_id: "n0".to_string(),
        char_offset_in_node: 0,
        child_index: None,
        utf16_len: 10,
    }];

    let result = find_mapping_for_char(&mappings, 5);
    assert!(result.is_some());
}

#[test]
fn test_find_mapping_before_first() {
    let mappings = vec![OffsetMapping {
        byte_range: 5..10,
        char_range: 5..10,
        node_id: "n0".to_string(),
        char_offset_in_node: 0,
        child_index: None,
        utf16_len: 5,
    }];

    // Position 2 is before the first mapping
    let result = find_mapping_for_char(&mappings, 2);
    assert!(result.is_none());
}

#[test]
fn test_find_mapping_after_last() {
    let mappings = vec![OffsetMapping {
        byte_range: 0..5,
        char_range: 0..5,
        node_id: "n0".to_string(),
        char_offset_in_node: 0,
        child_index: None,
        utf16_len: 5,
    }];

    // Position 10 is after the last mapping
    let result = find_mapping_for_char(&mappings, 10);
    assert!(result.is_none());
}

#[test]
fn test_find_mapping_empty() {
    let mappings: Vec<OffsetMapping> = vec![];
    let result = find_mapping_for_char(&mappings, 0);
    assert!(result.is_none());
}

#[test]
fn test_find_mapping_invisible_snaps() {
    // Invisible content should flag should_snap=true
    let mappings = vec![OffsetMapping {
        byte_range: 0..2,
        char_range: 0..2,
        node_id: "n0".to_string(),
        char_offset_in_node: 0,
        child_index: None,
        utf16_len: 0, // invisible
    }];

    let result = find_mapping_for_char(&mappings, 1);
    assert!(result.is_some());
    let (_, should_snap) = result.unwrap();
    assert!(should_snap, "invisible content should trigger snap");
}

// =============================================================================
// Regression Tests (from status doc bugs)
// =============================================================================

#[test]
fn regression_bug6_heading_as_paragraph_boundary() {
    // Bug #6: Headings should be tracked as paragraph boundaries
    let result = render_test("# Heading\n\nParagraph");

    // Should have at least 2 content paragraphs (heading + paragraph)
    // Plus potential gap paragraphs
    assert!(
        result.len() >= 2,
        "heading should create separate paragraph"
    );

    // First paragraph should contain heading
    assert!(
        result[0].html.contains("<h1>") || result[0].html.contains("Heading"),
        "first paragraph should be heading"
    );
}

#[test]
fn regression_bug8_inline_formatting_no_double_syntax() {
    // Bug #8: Inline formatting should not produce double **
    let result = render_test("some **bold** text");

    // Count occurrences of ** in HTML
    let html = &result[0].html;
    let double_star_count = html.matches("**").count();

    // Should have exactly 2 occurrences (opening and closing, wrapped in spans)
    // The bug was producing 4 (doubled emission)
    assert!(
        double_star_count <= 2,
        "should not have double ** syntax: found {} in {}",
        double_star_count,
        html
    );
}

#[test]
fn regression_bug9_lists_as_paragraph_boundary() {
    // Bug #9: Lists should be tracked as paragraph boundaries
    let result = render_test("Before\n\n- Item 1\n- Item 2\n\nAfter");

    // Should have paragraphs for: Before, list, After (plus gaps)
    let has_list = result
        .iter()
        .any(|p| p.html.contains("<li>") || p.html.contains("<ul>"));
    assert!(has_list, "list should be present in rendered output");
}

#[test]
fn regression_bug9_code_blocks_as_paragraph_boundary() {
    // Bug #9: Code blocks should be tracked as paragraph boundaries
    let result = render_test("Before\n\n```\ncode\n```\n\nAfter");

    let has_code = result
        .iter()
        .any(|p| p.html.contains("<pre>") || p.html.contains("<code>"));
    assert!(has_code, "code block should be present in rendered output");
}

#[test]
fn regression_bug11_gap_paragraphs_for_whitespace() {
    // Bug #11: Gap paragraphs should be created for EXTRA inter-block whitespace
    // Note: Headings consume trailing newline, so need 4 newlines total for gap > MIN_PARAGRAPH_BREAK

    // Test with extra whitespace (4 newlines = heading eats 1, leaves 3, gap = 3 > 2)
    let result = render_test("# Title\n\n\n\nContent"); // 4 newlines
    assert_eq!(result.len(), 3, "Expected 3 elements with extra whitespace");
    assert!(result[1].html.contains("gap-"), "Middle element should be a gap");

    // Test standard break (3 newlines = heading eats 1, leaves 2, gap = 2 = MIN, no gap element)
    let result2 = render_test("# Title\n\n\nContent"); // 3 newlines
    assert_eq!(result2.len(), 2, "Expected 2 elements with standard break equivalent");
}

// =============================================================================
// Char Range Coverage Tests
// =============================================================================

#[test]
fn test_char_range_coverage_allows_paragraph_breaks() {
    // Verify char ranges cover document content, allowing standard \n\n breaks
    // The MIN_PARAGRAPH_BREAK zone (2 chars) is intentionally not covered -
    // cursor snaps to adjacent paragraphs for standard breaks.
    // Only EXTRA whitespace beyond \n\n gets gap elements.
    let input = "Hello\n\nWorld";
    let doc = LoroDoc::new();
    let text = doc.get_text("content");
    text.insert(0, input).unwrap();
    let (paragraphs, _cache) = render_paragraphs_incremental(&text, None, None);

    // With standard \n\n break, we expect 2 paragraphs (no gap element)
    // Paragraph ranges include some trailing whitespace from markdown parsing
    assert_eq!(paragraphs.len(), 2, "Expected 2 paragraphs for standard break");

    // First paragraph ends before second starts, with gap for \n\n
    let gap_start = paragraphs[0].char_range.end;
    let gap_end = paragraphs[1].char_range.start;
    let gap_size = gap_end - gap_start;
    assert!(gap_size <= 2, "Gap should be at most MIN_PARAGRAPH_BREAK (2), got {}", gap_size);
}

#[test]
fn test_char_range_coverage_with_extra_whitespace() {
    // Extra whitespace beyond MIN_PARAGRAPH_BREAK (2) gets gap elements
    // Plain paragraphs don't consume trailing newlines like headings do
    let input = "Hello\n\n\n\nWorld"; // 4 newlines = gap of 4 > 2
    let doc = LoroDoc::new();
    let text = doc.get_text("content");
    text.insert(0, input).unwrap();
    let (paragraphs, _cache) = render_paragraphs_incremental(&text, None, None);

    // With extra newlines, we expect 3 elements: para, gap, para
    assert_eq!(paragraphs.len(), 3, "Expected 3 elements with extra whitespace");

    // Gap element should exist and cover whitespace zone
    let gap = &paragraphs[1];
    assert!(gap.html.contains("gap-"), "Second element should be a gap");

    // Gap should cover ALL whitespace (not just extra)
    assert_eq!(gap.char_range.start, paragraphs[0].char_range.end,
        "Gap should start where first paragraph ends");
    assert_eq!(gap.char_range.end, paragraphs[2].char_range.start,
        "Gap should end where second paragraph starts");
}

#[test]
fn test_node_ids_unique_across_paragraphs() {
    // Verify HTML id attributes are unique across paragraphs
    let result = render_test("# Heading\n\nParagraph with **bold**\n\n- List item");

    // Print rendered output for debugging failures
    for (i, para) in result.iter().enumerate() {
        eprintln!("--- Paragraph {} ---", i);
        eprintln!("char_range: {:?}", para.char_range);
        eprintln!("html: {}", para.html);
        eprintln!(
            "offset_map node_ids: {:?}",
            para.offset_map
                .iter()
                .map(|m| &m.node_id)
                .collect::<Vec<_>>()
        );
    }

    // Extract all id and data-node-id attributes from HTML
    let id_regex = regex::Regex::new(r#"(?:id|data-node-id)="([^"]+)""#).unwrap();

    let mut all_html_ids = std::collections::HashSet::new();
    for (para_idx, para) in result.iter().enumerate() {
        for cap in id_regex.captures_iter(&para.html) {
            let id = cap.get(1).unwrap().as_str();
            assert!(
                all_html_ids.insert(id.to_string()),
                "Duplicate HTML id '{}' in paragraph {}",
                id,
                para_idx
            );
        }
    }
}

#[test]
fn test_offset_mappings_reference_own_paragraph() {
    // Verify offset mappings only reference node IDs that exist in their paragraph's HTML
    let result = render_test("# Heading\n\nParagraph with **bold**\n\n- List item");

    let id_regex = regex::Regex::new(r#"(?:id|data-node-id)="([^"]+)""#).unwrap();

    for (para_idx, para) in result.iter().enumerate() {
        // Collect all node IDs in this paragraph's HTML
        let html_ids: std::collections::HashSet<_> = id_regex
            .captures_iter(&para.html)
            .map(|cap| cap.get(1).unwrap().as_str().to_string())
            .collect();

        // Verify each offset mapping references a node in this paragraph
        for mapping in &para.offset_map {
            assert!(
                html_ids.contains(&mapping.node_id),
                "Paragraph {} has offset mapping referencing '{}' but HTML only has {:?}\nHTML: {}",
                para_idx,
                mapping.node_id,
                html_ids,
                para.html
            );
        }
    }
}

// =============================================================================
// Incremental Rendering Tests
// =============================================================================

#[test]
fn test_incremental_cache_reuse() {
    // Verify cache is populated and can be reused
    let input = "First para\n\nSecond para";
    let doc = LoroDoc::new();
    let text = doc.get_text("content");
    text.insert(0, input).unwrap();

    let (paras1, cache1) = render_paragraphs_incremental(&text, None, None);
    assert!(!cache1.paragraphs.is_empty(), "Cache should be populated");

    // Second render with same content should reuse cache
    let (paras2, _cache2) = render_paragraphs_incremental(&text, Some(&cache1), None);

    // Should produce identical output
    assert_eq!(paras1.len(), paras2.len());
    for (p1, p2) in paras1.iter().zip(paras2.iter()) {
        assert_eq!(p1.html, p2.html);
    }
}

// =============================================================================
// Loro CRDT API Spike Tests
// =============================================================================

#[test]
fn test_loro_basic_text_operations() {
    use loro::LoroDoc;

    let doc = LoroDoc::new();
    let text = doc.get_text("content");

    // Insert
    text.insert(0, "Hello").unwrap();
    assert_eq!(text.to_string(), "Hello");
    assert_eq!(text.len_unicode(), 5);

    // Insert at position
    text.insert(5, " world").unwrap();
    assert_eq!(text.to_string(), "Hello world");
    assert_eq!(text.len_unicode(), 11);

    // Delete
    text.delete(5, 6).unwrap(); // delete " world"
    assert_eq!(text.to_string(), "Hello");
    assert_eq!(text.len_unicode(), 5);
}

#[test]
fn test_loro_unicode_handling() {
    use loro::LoroDoc;

    let doc = LoroDoc::new();
    let text = doc.get_text("content");

    // Insert unicode
    text.insert(0, "Hello 🎉 世界").unwrap();

    // Check lengths
    let content = text.to_string();
    assert_eq!(content, "Hello 🎉 世界");

    // Unicode length (chars)
    assert_eq!(text.len_unicode(), 10); // H e l l o   🎉   世 界

    // UTF-16 length (for DOM)
    // 🎉 is a surrogate pair (2 UTF-16 units), rest are 1 each
    assert_eq!(text.len_utf16(), 11); // 6 + 2 + 1 + 2 = 11

    // UTF-8 length (bytes)
    assert_eq!(text.len_utf8(), content.len());
}

#[test]
fn test_loro_undo_redo() {
    use loro::{LoroDoc, UndoManager};

    let doc = LoroDoc::new();
    let text = doc.get_text("content");
    let mut undo_mgr = UndoManager::new(&doc);

    // Type some text
    text.insert(0, "Hello").unwrap();
    doc.commit();

    text.insert(5, " world").unwrap();
    doc.commit();

    assert_eq!(text.to_string(), "Hello world");

    // Undo last change
    assert!(undo_mgr.can_undo());
    undo_mgr.undo().unwrap();
    assert_eq!(text.to_string(), "Hello");

    // Undo first change
    undo_mgr.undo().unwrap();
    assert_eq!(text.to_string(), "");

    // Redo
    assert!(undo_mgr.can_redo());
    undo_mgr.redo().unwrap();
    assert_eq!(text.to_string(), "Hello");

    undo_mgr.redo().unwrap();
    assert_eq!(text.to_string(), "Hello world");
}

#[test]
fn test_loro_char_to_utf16_conversion() {
    use loro::LoroDoc;

    let doc = LoroDoc::new();
    let text = doc.get_text("content");

    text.insert(0, "Hello 🎉 世界").unwrap();

    // Simulate char→UTF16 conversion for cursor positioning
    // Given a char offset, compute UTF-16 offset
    fn char_to_utf16(text: &loro::LoroText, char_pos: usize) -> usize {
        if char_pos == 0 {
            return 0;
        }
        // Fast path: if all ASCII, char == UTF-16
        if text.len_unicode() == text.len_utf16() {
            return char_pos;
        }
        // Slow path: get slice and count UTF-16 units
        match text.slice(0, char_pos) {
            Ok(slice) => slice.encode_utf16().count(),
            Err(_) => 0,
        }
    }

    // "Hello 🎉 世界"
    // Positions: H(0) e(1) l(2) l(3) o(4) ' '(5) 🎉(6) ' '(7) 世(8) 界(9)
    // UTF-16:    0     1    2    3    4     5     6,7    8     9    10

    assert_eq!(char_to_utf16(&text, 0), 0);
    assert_eq!(char_to_utf16(&text, 6), 6);  // before emoji
    assert_eq!(char_to_utf16(&text, 7), 8);  // after emoji (emoji is 2 UTF-16 units)
    assert_eq!(char_to_utf16(&text, 10), 11); // end
}

#[test]
fn test_loro_ascii_fast_path() {
    use loro::LoroDoc;

    let doc = LoroDoc::new();
    let text = doc.get_text("content");

    // Pure ASCII content
    text.insert(0, "Hello world, this is a test!").unwrap();

    // Verify fast path condition: all lengths equal for ASCII
    assert_eq!(text.len_unicode(), text.len_utf8());
    assert_eq!(text.len_unicode(), text.len_utf16());

    // Fast path should just return char_pos directly
    fn char_to_utf16(text: &loro::LoroText, char_pos: usize) -> usize {
        if char_pos == 0 {
            return 0;
        }
        if text.len_unicode() == text.len_utf16() {
            return char_pos; // fast path
        }
        text.slice(0, char_pos).map(|s| s.encode_utf16().count()).unwrap_or(0)
    }

    // All positions should be identity for ASCII
    for i in 0..=text.len_unicode() {
        assert_eq!(char_to_utf16(&text, i), i, "ASCII fast path failed at pos {}", i);
    }
}
