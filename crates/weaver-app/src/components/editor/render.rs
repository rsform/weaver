//! Markdown rendering for the editor.
//!
//! Phase 2: Full-document rendering with formatting characters visible as styled spans.
//! Future: Incremental paragraph rendering and contextual formatting visibility.
//!
//! Uses EditorWriter which tracks gaps in offset_iter to preserve formatting characters.

use markdown_weaver::Parser;
use super::offset_map::RenderResult;
use super::writer::EditorWriter;

/// Render markdown to HTML with visible formatting characters and offset mappings.
///
/// This function performs a full re-render of the document on every change.
/// Formatting characters (**, *, #, etc) are wrapped in styled spans for visibility.
///
/// Uses EditorWriter which processes offset_iter events to detect consumed
/// formatting characters and emit them as `<span class="md-syntax-*">` elements.
///
/// Returns both the rendered HTML and offset mappings for cursor restoration.
///
/// # Phase 2 features
/// - Formatting characters visible (wrapped in .md-syntax-inline and .md-syntax-block)
/// - Offset map generation for cursor restoration
/// - Full document re-render (fast enough for current needs)
///
/// # Future improvements
/// - Paragraph-level incremental rendering
/// - Contextual formatting hiding based on cursor position
pub fn render_markdown_simple(source: &str) -> RenderResult {
    use jumprope::JumpRopeBuf;

    let source_rope = JumpRopeBuf::from(source);
    let parser = Parser::new_ext(source, weaver_renderer::default_md_options())
        .into_offset_iter();
    let mut output = String::new();

    match EditorWriter::<_, _, ()>::new(source, &source_rope, parser, &mut output).run() {
        Ok(offset_map) => RenderResult {
            html: output,
            offset_map,
        },
        Err(_) => {
            // Fallback to empty result on error
            RenderResult {
                html: String::new(),
                offset_map: Vec::new(),
            }
        }
    }
}
