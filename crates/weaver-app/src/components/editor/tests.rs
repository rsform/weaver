//! Snapshot tests for the markdown editor rendering pipeline.

use super::offset_map::{OffsetMapping, find_mapping_for_char};
use super::paragraph::ParagraphRender;
use super::render::render_paragraphs;
use jumprope::JumpRopeBuf;
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
    let rope = JumpRopeBuf::from(input);
    let paragraphs = render_paragraphs(&rope);
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
    // Bug #11: Gap paragraphs should be created for inter-block whitespace
    let result = render_test("# Title\n\nContent");

    // Check that char ranges cover the full document without gaps
    let mut prev_end = 0;
    for para in &result {
        // Allow gaps to be filled by gap paragraphs
        if para.char_range.0 > prev_end {
            // This would be a gap - but gap paragraphs should fill it
            panic!(
                "Gap in char ranges: {}..{} missing coverage",
                prev_end, para.char_range.0
            );
        }
        prev_end = para.char_range.1;
    }
}

// =============================================================================
// Char Range Coverage Tests
// =============================================================================

#[test]
fn test_char_range_full_coverage() {
    // Verify that char ranges cover entire document
    let input = "Hello\n\nWorld";
    let rope = JumpRopeBuf::from(input);
    let paragraphs = render_paragraphs(&rope);

    let doc_len = rope.len_chars();

    // Collect all ranges
    let mut ranges: Vec<_> = paragraphs.iter().map(|p| p.char_range.clone()).collect();
    ranges.sort_by_key(|r| r.start);

    // Check coverage
    let mut covered = 0;
    for range in &ranges {
        assert!(
            range.start <= covered,
            "Gap at position {}, next range starts at {}",
            covered,
            range.start
        );
        covered = covered.max(range.end);
    }

    assert!(
        covered >= doc_len,
        "Ranges don't cover full document: covered {} of {}",
        covered,
        doc_len
    );
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

use super::render::render_paragraphs_incremental;

#[test]
fn test_incremental_renders_same_as_full() {
    // Incremental render with no cache should produce same result as full render
    let input = "# Heading\n\nParagraph with **bold**\n\n- List item";
    let rope = JumpRopeBuf::from(input);

    let full = render_paragraphs(&rope);
    let (incremental, _cache) = render_paragraphs_incremental(&rope, None, None);

    // Compare HTML output (hashes may differ due to caching internals)
    assert_eq!(
        full.len(),
        incremental.len(),
        "Different paragraph count: full={}, incr={}",
        full.len(),
        incremental.len()
    );

    for (i, (f, inc)) in full.iter().zip(incremental.iter()).enumerate() {
        assert_eq!(
            f.html, inc.html,
            "Paragraph {} HTML differs:\nFull: {}\nIncr: {}",
            i, f.html, inc.html
        );
        assert_eq!(
            f.byte_range, inc.byte_range,
            "Paragraph {} byte_range differs",
            i
        );
        assert_eq!(
            f.char_range, inc.char_range,
            "Paragraph {} char_range differs",
            i
        );
    }
}

#[test]
fn test_incremental_cache_reuse() {
    // Verify cache is populated and can be reused
    let input = "First para\n\nSecond para";
    let rope = JumpRopeBuf::from(input);

    let (paras1, cache1) = render_paragraphs_incremental(&rope, None, None);
    assert!(!cache1.paragraphs.is_empty(), "Cache should be populated");

    // Second render with same content should reuse cache
    let (paras2, _cache2) = render_paragraphs_incremental(&rope, Some(&cache1), None);

    // Should produce identical output
    assert_eq!(paras1.len(), paras2.len());
    for (p1, p2) in paras1.iter().zip(paras2.iter()) {
        assert_eq!(p1.html, p2.html);
    }
}
