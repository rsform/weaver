use crate::NotebookContext;

use super::*;
use std::path::PathBuf;
use weaver_common::jacquard::client::{
    AtpSession, MemorySessionStore,
    credential_session::{CredentialSession, SessionKey},
};

/// Type alias for the session used in tests
type TestSession = CredentialSession<
    MemorySessionStore<SessionKey, AtpSession>,
    weaver_common::jacquard::identity::JacquardResolver,
>;

/// Helper: Create test context without network capabilities
fn test_context() -> StaticSiteContext<TestSession> {
    let root = PathBuf::from("/tmp/test");
    let destination = PathBuf::from("/tmp/output");
    let mut ctx = StaticSiteContext::new(root, destination, None);
    ctx.client = None; // Explicitly disable network
    ctx
}

/// Helper: Render markdown to HTML using test context
async fn render_markdown(input: &str) -> String {
    let context = test_context();
    export_page(input, context).await.unwrap()
}

#[tokio::test]
async fn test_smoke() {
    let output = render_markdown("Hello world").await;
    assert!(output.contains("Hello world"));
}

#[tokio::test]
async fn test_paragraph_rendering() {
    let input = "This is a paragraph.\n\nThis is another paragraph.";
    let output = render_markdown(input).await;
    insta::assert_snapshot!(output);
}

#[tokio::test]
async fn test_heading_rendering() {
    let input = "# Heading 1\n\n## Heading 2\n\n### Heading 3";
    let output = render_markdown(input).await;
    insta::assert_snapshot!(output);
}

#[tokio::test]
async fn test_list_rendering() {
    let input = "- Item 1\n- Item 2\n  - Nested\n\n1. Ordered 1\n2. Ordered 2";
    let output = render_markdown(input).await;
    insta::assert_snapshot!(output);
}

#[tokio::test]
async fn test_code_block_rendering() {
    let input = "```rust\nfn main() {\n    println!(\"Hello\");\n}\n```";
    let output = render_markdown(input).await;
    insta::assert_snapshot!(output);
}

#[tokio::test]
async fn test_table_rendering() {
    let input = "| Left | Center | Right |\n|:-----|:------:|------:|\n| A | B | C |";
    let output = render_markdown(input).await;
    insta::assert_snapshot!(output);
}

#[tokio::test]
async fn test_blockquote_rendering() {
    let input = "> This is a quote\n>\n> With multiple lines";
    let output = render_markdown(input).await;
    insta::assert_snapshot!(output);
}

#[tokio::test]
async fn test_math_rendering() {
    let input = "Inline $x^2$ and display:\n\n$$\ny = mx + b\n$$";
    let output = render_markdown(input).await;
    insta::assert_snapshot!(output);
}

#[tokio::test]
async fn test_wikilink_resolution() {
    let vault_contents = vec![
        PathBuf::from("notes/First Note.md"),
        PathBuf::from("notes/Second Note.md"),
    ];

    let mut context = test_context();
    context.dir_contents = Some(vault_contents.into());

    let input = "[[First Note]] and [[Second Note]]";
    let output = export_page(input, context).await.unwrap();
    println!("{output}");
    assert!(output.contains("./First%20Note.html"));
    assert!(output.contains("./Second%20Note.html"));
}

#[tokio::test]
async fn test_broken_wikilink() {
    let vault_contents = vec![PathBuf::from("notes/Exists.md")];

    let mut context = test_context();
    context.dir_contents = Some(vault_contents.into());

    let input = "[[Does Not Exist]]";
    let output = export_page(input, context).await.unwrap();

    // Broken wikilinks become links (they just don't point anywhere valid)
    // This is acceptable - static site will show 404 on click
    assert!(output.contains("<a href="));
    assert!(output.contains("Does Not Exist</a>") || output.contains("Does%20Not%20Exist"));
}

#[tokio::test]
async fn test_wikilink_with_section() {
    let vault_contents = vec![PathBuf::from("Note.md")];

    let mut context = test_context();
    context.dir_contents = Some(vault_contents.into());

    let input = "[[Note#Section]]";
    let output = export_page(input, context).await.unwrap();
    println!("{output}");
    assert!(output.contains("Note#Section"));
}

#[tokio::test]
async fn test_link_flattening_enabled() {
    let mut context = test_context();
    context.options = StaticSiteOptions::FLATTEN_STRUCTURE;

    let input = "[Link](path/to/nested/file.md)";
    let output = export_page(input, context).await.unwrap();
    println!("{output}");
    // Should flatten to single parent directory
    assert!(output.contains("./entry/file.html"));
}

#[tokio::test]
async fn test_link_flattening_disabled() {
    let mut context = test_context();
    context.options = StaticSiteOptions::empty();

    let input = "[Link](path/to/nested/file.md)";
    let output = export_page(input, context).await.unwrap();
    println!("{output}");
    // Should preserve original path
    assert!(output.contains("path/to/nested/file.html"));
}

#[tokio::test]
async fn test_frontmatter_parsing() {
    let input = "---\ntitle: Test Page\nauthor: Test Author\n---\n\nContent here";
    let context = test_context();
    let output = export_page(input, context.clone()).await.unwrap();

    // Frontmatter should be parsed but not rendered
    assert!(!output.contains("title: Test Page"));
    assert!(output.contains("Content here"));

    // Verify frontmatter was captured
    let frontmatter = context.frontmatter();
    let yaml = frontmatter.contents();
    let yaml_guard = yaml.read().unwrap();
    assert!(yaml_guard.len() > 0);
}

#[tokio::test]
async fn test_empty_frontmatter() {
    let input = "---\n---\n\nContent";
    let output = render_markdown(input).await;

    assert!(output.contains("Content"));
    assert!(!output.contains("---"));
}

#[tokio::test]
async fn test_empty_input() {
    let output = render_markdown("").await;
    assert_eq!(output, "");
}

#[tokio::test]
async fn test_html_and_special_characters() {
    // Test that markdown correctly handles HTML and special chars per CommonMark spec
    let input =
        "Text with <special> & some text. Valid tags: <em>emphasis</em> and <strong>bold</strong>";
    let output = render_markdown(input).await;

    // & must be escaped for valid HTML
    assert!(output.contains("&amp;"));

    // Inline HTML tags pass through (CommonMark behavior)
    assert!(output.contains("<special>"));
    assert!(output.contains("<em>emphasis</em>"));
    assert!(output.contains("<strong>bold</strong>"));
}

#[tokio::test]
async fn test_unicode_content() {
    let input = "Unicode: 你好 🎉 café";
    let output = render_markdown(input).await;

    assert!(output.contains("你好"));
    assert!(output.contains("🎉"));
    assert!(output.contains("café"));
}

// =============================================================================
// WeaverBlock Prefix Tests
// =============================================================================

#[tokio::test]
async fn test_weaver_block_aside_class() {
    let input = "\n\n{.aside}\nThis paragraph should be in an aside.";
    let output = render_markdown(input).await;
    insta::assert_snapshot!(output);
}

#[tokio::test]
async fn test_weaver_block_custom_class() {
    let input = "\n\n{.highlight}\nThis paragraph has a custom class.";
    let output = render_markdown(input).await;
    insta::assert_snapshot!(output);
}

#[tokio::test]
async fn test_weaver_block_custom_attributes() {
    let input = "\n\n{.foo, width: 300px, data-test: value}\nParagraph with class and attributes.";
    let output = render_markdown(input).await;
    insta::assert_snapshot!(output);
}

#[tokio::test]
async fn test_weaver_block_before_heading() {
    let input = "\n\n{.aside}\n## Heading in aside\n\nParagraph also in aside.";
    let output = render_markdown(input).await;
    insta::assert_snapshot!(output);
}

#[tokio::test]
async fn test_weaver_block_before_blockquote() {
    let input = "\n\n{.aside}\n\n> This blockquote is in an aside.";
    let output = render_markdown(input).await;
    insta::assert_snapshot!(output);
}

#[tokio::test]
async fn test_weaver_block_before_list() {
    let input = "\n\n{.aside}\n\n- Item 1\n- Item 2";
    let output = render_markdown(input).await;
    insta::assert_snapshot!(output);
}

#[tokio::test]
async fn test_weaver_block_before_code_block() {
    let input = "\n\n{.aside}\n\n```rust\nfn main() {}\n```";
    let output = render_markdown(input).await;
    insta::assert_snapshot!(output);
}

#[tokio::test]
async fn test_weaver_block_multiple_classes() {
    let input = "\n\n{.aside, .highlight, .important}\nMultiple classes applied.";
    let output = render_markdown(input).await;
    insta::assert_snapshot!(output);
}

#[tokio::test]
async fn test_weaver_block_no_effect_on_following() {
    let input = "\n\n{.aside}\nFirst paragraph in aside.\n\nSecond paragraph NOT in aside.";
    let output = render_markdown(input).await;
    insta::assert_snapshot!(output);
}

// =============================================================================
// Footnote / Sidenote Tests
// =============================================================================

#[tokio::test]
async fn test_footnote_traditional() {
    let input = "Here is some text[^1].\n[^1]: This is the footnote definition.";
    let output = render_markdown(input).await;
    insta::assert_snapshot!(output);
}

#[tokio::test]
async fn test_footnote_sidenote_inline() {
    // When definition immediately follows reference in the same paragraph flow
    let input = "Here is text[^note]\n[^note]: Sidenote content.";
    let output = render_markdown(input).await;
    insta::assert_snapshot!(output);
}

#[tokio::test]
async fn test_footnote_multiple() {
    let input = "First[^1] and second[^2] footnotes.\n[^1]: First note.\n[^2]: Second note.";
    let output = render_markdown(input).await;
    insta::assert_snapshot!(output);
}

#[tokio::test]
async fn test_footnote_with_inline_formatting() {
    let input = "Text with footnote[^fmt].\n[^fmt]: Note with **bold** and *italic*.";
    let output = render_markdown(input).await;
    insta::assert_snapshot!(output);
}

#[tokio::test]
async fn test_footnote_named() {
    let input = "Reference[^my-note].\n[^my-note]: Named footnote content.";
    let output = render_markdown(input).await;
    insta::assert_snapshot!(output);
}

#[tokio::test]
async fn test_footnote_in_blockquote() {
    let input = "> Quote with footnote[^q].\n[^q]: Footnote for quote.";
    let output = render_markdown(input).await;
    insta::assert_snapshot!(output);
}

// =============================================================================
// Combined WeaverBlock + Footnote Tests
// =============================================================================

#[tokio::test]
async fn test_weaver_block_with_footnote() {
    let input = "{.aside}\nAside with a footnote[^aside].\n\n[^aside]: Footnote in aside context.";
    let output = render_markdown(input).await;
    insta::assert_snapshot!(output);
}
