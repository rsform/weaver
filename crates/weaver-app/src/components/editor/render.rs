//! Markdown rendering for the editor.
//!
//! Phase 2: Paragraph-level incremental rendering with formatting characters visible.
//!
//! Uses EditorWriter which tracks gaps in offset_iter to preserve formatting characters.

use super::offset_map::{OffsetMapping, RenderResult};
use super::paragraph::{ParagraphRender, hash_source, rope_slice_to_string};
use super::writer::EditorWriter;
use jumprope::JumpRopeBuf;
use markdown_weaver::Parser;

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
/// # Deprecated: Use `render_paragraphs()` for incremental rendering
pub fn render_markdown_simple(source: &str) -> RenderResult {
    let source_rope = JumpRopeBuf::from(source);
    let parser = Parser::new_ext(source, weaver_renderer::default_md_options()).into_offset_iter();
    let mut output = String::new();

    match EditorWriter::<_, _, ()>::new(source, &source_rope, parser, &mut output).run() {
        Ok(result) => RenderResult {
            html: output,
            offset_map: result.offset_maps,
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

/// Render markdown in paragraph chunks for incremental DOM updates.
///
/// First renders the whole document to discover paragraph boundaries via
/// markdown events (Tag::Paragraph), then re-renders each paragraph separately.
/// This allows updating only changed paragraphs in the DOM, preserving cursor
/// position naturally.
///
/// # Returns
///
/// A vector of `ParagraphRender` structs, each containing:
/// - Source byte and char ranges
/// - Rendered HTML (without wrapper div)
/// - Offset mappings for that paragraph
/// - Source hash for change detection
///
/// # Phase 2 Benefits
/// - Only re-render changed paragraphs
/// - Browser preserves cursor in unchanged paragraphs naturally
/// - Faster for large documents
/// - No manual cursor restoration needed for most edits
pub fn render_paragraphs(rope: &JumpRopeBuf) -> Vec<ParagraphRender> {
    let source = rope.to_string();

    // Handle empty rope - return single empty paragraph for cursor positioning
    if source.is_empty() {
        let empty_node_id = "n0".to_string();
        let empty_html = format!(r#"<span id="{}">{}</span>"#, empty_node_id, '\u{200B}');

        return vec![ParagraphRender {
            byte_range: 0..0,
            char_range: 0..0,
            html: empty_html,
            offset_map: vec![],
            source_hash: 0,
        }];
    }

    // First pass: render whole document to get paragraph boundaries
    // TODO: CACHE THIS!
    let parser = Parser::new_ext(&source, weaver_renderer::default_md_options()).into_offset_iter();
    let mut scratch_output = String::new();

    let paragraph_ranges =
        match EditorWriter::<_, _, ()>::new(&source, rope, parser, &mut scratch_output).run() {
            Ok(result) => result.paragraph_ranges,
            Err(_) => return Vec::new(),
        };

    // Second pass: render each paragraph separately
    let mut paragraphs = Vec::with_capacity(paragraph_ranges.len());
    let mut node_id_offset = 0; // Track total nodes used so far for unique IDs

    for (idx, (byte_range, char_range)) in paragraph_ranges.iter().enumerate() {
        // Extract paragraph source
        let para_source = rope_slice_to_string(rope, char_range.clone());
        let source_hash = hash_source(&para_source);

        // Render this paragraph with unique node IDs
        let para_rope = JumpRopeBuf::from(para_source.as_str());
        let parser =
            Parser::new_ext(&para_source, weaver_renderer::default_md_options()).into_offset_iter();
        let mut output = String::new();

        let mut offset_map = match EditorWriter::<_, _, ()>::new_with_node_offset(
            &para_source,
            &para_rope,
            parser,
            &mut output,
            node_id_offset,
        )
        .run()
        {
            Ok(result) => {
                // Update node ID offset for next paragraph
                // Count how many unique node IDs were used in this paragraph
                let max_node_id = result
                    .offset_maps
                    .iter()
                    .filter_map(|m| {
                        m.node_id
                            .strip_prefix("n")
                            .and_then(|s| s.parse::<usize>().ok())
                    })
                    .max()
                    .unwrap_or(node_id_offset);
                node_id_offset = max_node_id + 1;

                result.offset_maps
            }
            Err(_) => Vec::new(),
        };

        // Adjust offset map to be relative to document, not paragraph
        // Each mapping's ranges need to be shifted by paragraph start
        let para_char_start = char_range.start;
        let para_byte_start = byte_range.start;

        for mapping in &mut offset_map {
            mapping.byte_range.start += para_byte_start;
            mapping.byte_range.end += para_byte_start;
            mapping.char_range.start += para_char_start;
            mapping.char_range.end += para_char_start;
        }

        paragraphs.push(ParagraphRender {
            byte_range: byte_range.clone(),
            char_range: char_range.clone(),
            html: output,
            offset_map,
            source_hash,
        });
    }

    // Insert gap paragraphs for whitespace between blocks
    // This gives the cursor somewhere to land when positioned in newlines
    let mut paragraphs_with_gaps = Vec::with_capacity(paragraphs.len() * 2);
    let mut prev_end_char = 0usize;
    let mut prev_end_byte = 0usize;

    for para in paragraphs {
        // Check for gap before this paragraph
        if para.char_range.start > prev_end_char {
            let gap_start_char = prev_end_char;
            let gap_end_char = para.char_range.start;
            let gap_start_byte = prev_end_byte;
            let gap_end_byte = para.byte_range.start;

            let gap_node_id = format!("n{}", node_id_offset);
            node_id_offset += 1;
            let gap_html = format!(r#"<span id="{}">{}</span>"#, gap_node_id, '\u{200B}');

            paragraphs_with_gaps.push(ParagraphRender {
                byte_range: gap_start_byte..gap_end_byte,
                char_range: gap_start_char..gap_end_char,
                html: gap_html,
                offset_map: vec![OffsetMapping {
                    byte_range: gap_start_byte..gap_end_byte,
                    char_range: gap_start_char..gap_end_char,
                    node_id: gap_node_id,
                    char_offset_in_node: 0,
                    child_index: None,
                    utf16_len: 1, // zero-width space represents the gap
                }],
                source_hash: hash_source(&rope_slice_to_string(rope, gap_start_char..gap_end_char)),
            });
        }

        prev_end_char = para.char_range.end;
        prev_end_byte = para.byte_range.end;
        paragraphs_with_gaps.push(para);
    }

    // Check if rope ends with trailing newlines (empty paragraph at end)
    // If so, add an empty paragraph div for cursor positioning
    let source = rope.to_string();
    let has_trailing_newlines = source.ends_with("\n\n") || source.ends_with("\n");

    if has_trailing_newlines {
        let doc_end_char = rope.len_chars();
        let doc_end_byte = rope.len_bytes();

        // Only add if there's actually a gap at the end
        if doc_end_char > prev_end_char {
            let empty_node_id = format!("n{}", node_id_offset);
            let empty_html = format!(r#"<span id="{}">{}</span>"#, empty_node_id, '\u{200B}');

            paragraphs_with_gaps.push(ParagraphRender {
                byte_range: prev_end_byte..doc_end_byte,
                char_range: prev_end_char..doc_end_char,
                html: empty_html,
                offset_map: vec![OffsetMapping {
                    byte_range: prev_end_byte..doc_end_byte,
                    char_range: prev_end_char..doc_end_char,
                    node_id: empty_node_id,
                    char_offset_in_node: 0,
                    child_index: None,
                    utf16_len: 1, // zero-width space is 1 UTF-16 code unit
                }],
                source_hash: 0, // always render this paragraph
            });
        }
    }

    paragraphs_with_gaps
}
