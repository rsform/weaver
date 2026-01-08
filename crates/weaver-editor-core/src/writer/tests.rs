//! Snapshot tests for EditorWriter output.
//!
//! These tests exercise edge cases in paragraph rendering, cursor positioning,
//! and offset mapping.

use markdown_weaver::Parser;

use crate::text::EditorRope;
use crate::weaver_renderer;

use super::EditorWriter;

/// Helper to render markdown and return HTML segments + paragraph ranges.
fn render_markdown(source: &str) -> RenderOutput {
    let rope = EditorRope::from(source);
    let parser = Parser::new_ext(source, weaver_renderer::default_md_options()).into_offset_iter();

    let writer: EditorWriter<'_, _, _, (), (), ()> =
        EditorWriter::new(source, &rope, parser).with_auto_incrementing_prefix(0);

    let result = writer.run().expect("render failed");

    RenderOutput {
        html_segments: result.html_segments,
        paragraph_ranges: result
            .paragraph_ranges
            .into_iter()
            .map(|(byte_range, char_range)| ParagraphRange {
                byte_start: byte_range.start,
                byte_end: byte_range.end,
                char_start: char_range.start,
                char_end: char_range.end,
            })
            .collect(),
        offset_maps: result
            .offset_maps_by_paragraph
            .into_iter()
            .map(|maps| {
                maps.into_iter()
                    .map(|m| OffsetMapEntry {
                        byte_range: format!("{}..{}", m.byte_range.start, m.byte_range.end),
                        char_range: format!("{}..{}", m.char_range.start, m.char_range.end),
                        node_id: m.node_id.to_string(),
                        char_offset_in_node: m.char_offset_in_node,
                        utf16_len: m.utf16_len,
                    })
                    .collect()
            })
            .collect(),
    }
}

#[derive(Debug, serde::Serialize)]
struct RenderOutput {
    html_segments: Vec<String>,
    paragraph_ranges: Vec<ParagraphRange>,
    offset_maps: Vec<Vec<OffsetMapEntry>>,
}

#[derive(Debug, serde::Serialize)]
struct ParagraphRange {
    byte_start: usize,
    byte_end: usize,
    char_start: usize,
    char_end: usize,
}

#[derive(Debug, serde::Serialize)]
struct OffsetMapEntry {
    byte_range: String,
    char_range: String,
    node_id: String,
    char_offset_in_node: usize,
    utf16_len: usize,
}

// === Trailing paragraph tests ===

#[test]
fn test_single_paragraph_no_trailing() {
    let output = render_markdown("hello world");
    insta::assert_yaml_snapshot!(output);
}

#[test]
fn test_single_paragraph_single_newline() {
    let output = render_markdown("hello world\n");
    insta::assert_yaml_snapshot!(output);
}

#[test]
fn test_single_paragraph_double_newline() {
    // This should create a synthetic trailing paragraph
    let output = render_markdown("hello world\n\n");
    insta::assert_yaml_snapshot!(output);
}

#[test]
fn test_single_paragraph_triple_newline() {
    // Multiple trailing newlines
    let output = render_markdown("hello world\n\n\n");
    insta::assert_yaml_snapshot!(output);
}

#[test]
fn test_two_paragraphs() {
    let output = render_markdown("first\n\nsecond");
    insta::assert_yaml_snapshot!(output);
}

#[test]
fn test_two_paragraphs_trailing() {
    // Two paragraphs plus trailing newlines
    let output = render_markdown("first\n\nsecond\n\n");
    insta::assert_yaml_snapshot!(output);
}

// === Multiple blank lines tests ===

#[test]
fn test_three_enters() {
    // Simulates pressing enter 3 times from empty
    let output = render_markdown("\n\n\n");
    insta::assert_yaml_snapshot!(output);
}

#[test]
fn test_four_enters() {
    // Bug report: 4th enter moves cursor backwards
    let output = render_markdown("\n\n\n\n");
    insta::assert_yaml_snapshot!(output);
}

#[test]
fn test_text_then_four_enters() {
    let output = render_markdown("test\n\n\n\n");
    insta::assert_yaml_snapshot!(output);
}

#[test]
fn test_many_blank_lines() {
    // Bug report: arrow keys in many blank lines jumps to top
    let output = render_markdown("start\n\n\n\n\n\nend");
    insta::assert_yaml_snapshot!(output);
}

// === Blockquote tests ===

#[test]
fn test_blockquote_simple() {
    let output = render_markdown("> quote");
    insta::assert_yaml_snapshot!(output);
}

#[test]
fn test_blockquote_with_trailing() {
    let output = render_markdown("> quote\n\n");
    insta::assert_yaml_snapshot!(output);
}

#[test]
fn test_blockquote_multiline() {
    let output = render_markdown("> line one\n> line two");
    insta::assert_yaml_snapshot!(output);
}

#[test]
fn test_blockquote_then_text() {
    // Bug report: can't type on right side of >
    let output = render_markdown("> quote\n\ntext after");
    insta::assert_yaml_snapshot!(output);
}

// === Wikilink tests ===

#[test]
fn test_wikilink_simple() {
    let output = render_markdown("[[link]]");
    insta::assert_yaml_snapshot!(output);
}

#[test]
fn test_wikilink_partial() {
    // Partial wikilink
    let output = render_markdown("[[word");
    insta::assert_yaml_snapshot!(output);
}

#[test]
fn test_wikilink_nested_brackets() {
    // Bug report: [[][]] structure causes trouble
    let output = render_markdown("[[][]]");
    insta::assert_yaml_snapshot!(output);
}

#[test]
fn test_wikilink_partial_inside_full() {
    // Partial link inside full link
    let output = render_markdown("[[outer[[inner]]outer]]");
    insta::assert_yaml_snapshot!(output);
}

#[test]
fn test_many_opening_brackets() {
    // From the screenshot - many [[ in sequence
    let output = render_markdown("[[[[[[[[[[[[z]]\n]]]]");
    insta::assert_yaml_snapshot!(output);
}

// === Soft break / line continuation tests ===

#[test]
fn test_soft_break() {
    let output = render_markdown("line one\nline two");
    insta::assert_yaml_snapshot!(output);
}

#[test]
fn test_hard_break() {
    // Two trailing spaces = hard break
    let output = render_markdown("line one  \nline two");
    insta::assert_yaml_snapshot!(output);
}
