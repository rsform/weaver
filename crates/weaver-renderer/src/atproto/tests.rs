//! Tests for the AT Protocol ClientWriter
//!
//! These tests verify that ClientWriter produces the same output as StaticPageWriter
//! for core markdown rendering, particularly footnotes/sidenotes.

use super::writer::ClientWriter;
use markdown_weaver::Parser;
use markdown_weaver_escape::FmtWriter;

/// Helper: Render markdown to HTML using ClientWriter
fn render_markdown(input: &str) -> String {
    let options = crate::default_md_options();
    let parser = Parser::new_ext(input, options).into_offset_iter();
    let mut output = String::new();
    let writer: ClientWriter<'_, _, _, ()> = ClientWriter::new(parser, FmtWriter(&mut output), input);
    writer.run().unwrap();
    output
}

// =============================================================================
// Basic Rendering Tests
// =============================================================================

#[test]
fn test_smoke() {
    let output = render_markdown("Hello world");
    assert!(output.contains("Hello world"));
}

#[test]
fn test_paragraph_rendering() {
    let input = "This is a paragraph.\n\nThis is another paragraph.";
    let output = render_markdown(input);
    insta::assert_snapshot!(output);
}

#[test]
fn test_heading_rendering() {
    let input = "# Heading 1\n\n## Heading 2\n\n### Heading 3";
    let output = render_markdown(input);
    insta::assert_snapshot!(output);
}

#[test]
fn test_list_rendering() {
    let input = "- Item 1\n- Item 2\n  - Nested\n\n1. Ordered 1\n2. Ordered 2";
    let output = render_markdown(input);
    insta::assert_snapshot!(output);
}

#[test]
fn test_code_block_rendering() {
    let input = "```rust\nfn main() {\n    println!(\"Hello\");\n}\n```";
    let output = render_markdown(input);
    insta::assert_snapshot!(output);
}

#[test]
fn test_table_rendering() {
    let input = "| Left | Center | Right |\n|:-----|:------:|------:|\n| A | B | C |";
    let output = render_markdown(input);
    insta::assert_snapshot!(output);
}

#[test]
fn test_blockquote_rendering() {
    let input = "> This is a quote\n>\n> With multiple lines";
    let output = render_markdown(input);
    insta::assert_snapshot!(output);
}

#[test]
fn test_math_rendering() {
    let input = "Inline $x^2$ and display:\n\n$$\ny = mx + b\n$$";
    let output = render_markdown(input);
    insta::assert_snapshot!(output);
}

#[test]
fn test_empty_input() {
    let output = render_markdown("");
    assert_eq!(output, "");
}

#[test]
fn test_html_and_special_characters() {
    // ClientWriter wraps inline HTML in spans to contain embeds etc
    let input =
        "Text with <special> & some text. Valid tags: <em>emphasis</em> and <strong>bold</strong>";
    let output = render_markdown(input);
    assert!(output.contains("&amp;"));
    // Inline HTML gets wrapped in html-embed spans
    assert!(output.contains("html-embed-inline"));
    assert!(output.contains("<special>"));
}

#[test]
fn test_unicode_content() {
    let input = "Unicode: 你好 🎉 café";
    let output = render_markdown(input);
    assert!(output.contains("你好"));
    assert!(output.contains("🎉"));
    assert!(output.contains("café"));
}

// =============================================================================
// WeaverBlock Prefix Tests
// =============================================================================

#[test]
fn test_weaver_block_aside_class() {
    let input = "\n\n{.aside}\nThis paragraph should be in an aside.";
    let output = render_markdown(input);
    insta::assert_snapshot!(output);
}

#[test]
fn test_weaver_block_custom_class() {
    let input = "\n\n{.highlight}\nThis paragraph has a custom class.";
    let output = render_markdown(input);
    insta::assert_snapshot!(output);
}

#[test]
fn test_weaver_block_custom_attributes() {
    let input = "\n\n{.foo, width: 300px, data-test: value}\nParagraph with class and attributes.";
    let output = render_markdown(input);
    insta::assert_snapshot!(output);
}

#[test]
fn test_weaver_block_before_heading() {
    let input = "\n\n{.aside}\n## Heading in aside\n\nParagraph also in aside.";
    let output = render_markdown(input);
    insta::assert_snapshot!(output);
}

#[test]
fn test_weaver_block_before_blockquote() {
    let input = "\n\n{.aside}\n\n> This blockquote is in an aside.";
    let output = render_markdown(input);
    insta::assert_snapshot!(output);
}

#[test]
fn test_weaver_block_before_list() {
    let input = "\n\n{.aside}\n\n- Item 1\n- Item 2";
    let output = render_markdown(input);
    insta::assert_snapshot!(output);
}

#[test]
fn test_weaver_block_before_code_block() {
    let input = "\n\n{.aside}\n\n```rust\nfn main() {}\n```";
    let output = render_markdown(input);
    insta::assert_snapshot!(output);
}

#[test]
fn test_weaver_block_multiple_classes() {
    let input = "\n\n{.aside, .highlight, .important}\nMultiple classes applied.";
    let output = render_markdown(input);
    insta::assert_snapshot!(output);
}

#[test]
fn test_weaver_block_no_effect_on_following() {
    let input = "\n\n{.aside}\nFirst paragraph in aside.\n\nSecond paragraph NOT in aside.";
    let output = render_markdown(input);
    insta::assert_snapshot!(output);
}

// =============================================================================
// Footnote / Sidenote Tests
// =============================================================================

#[test]
fn test_footnote_traditional() {
    let input = "Here is some text[^1].\n[^1]: This is the footnote definition.";
    let output = render_markdown(input);
    insta::assert_snapshot!(output);
}

#[test]
fn test_footnote_sidenote_inline() {
    let input = "Here is text[^note]\n[^note]: Sidenote content.";
    let output = render_markdown(input);
    insta::assert_snapshot!(output);
}

#[test]
fn test_footnote_multiple() {
    let input = "First[^1] and second[^2] footnotes.\n[^1]: First note.\n[^2]: Second note.";
    let output = render_markdown(input);
    insta::assert_snapshot!(output);
}

#[test]
fn test_footnote_with_inline_formatting() {
    let input = "Text with footnote[^fmt].\n[^fmt]: Note with **bold** and *italic*.";
    let output = render_markdown(input);
    insta::assert_snapshot!(output);
}

#[test]
fn test_footnote_named() {
    let input = "Reference[^my-note].\n[^my-note]: Named footnote content.";
    let output = render_markdown(input);
    insta::assert_snapshot!(output);
}

#[test]
fn test_footnote_in_blockquote() {
    let input = "> Quote with footnote[^q].\n[^q]: Footnote for quote.";
    let output = render_markdown(input);
    insta::assert_snapshot!(output);
}

// =============================================================================
// Combined WeaverBlock + Footnote Tests
// =============================================================================

#[test]
fn test_weaver_block_with_footnote() {
    let input = "{.aside}\nAside with a footnote[^aside].\n\n[^aside]: Footnote in aside context.";
    let output = render_markdown(input);
    insta::assert_snapshot!(output);
}
