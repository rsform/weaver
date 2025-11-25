//! Markdown rendering for the editor.
//!
//! Phase 2: Paragraph-level incremental rendering with formatting characters visible.
//!
//! Uses EditorWriter which tracks gaps in offset_iter to preserve formatting characters.

use super::document::EditInfo;
use super::offset_map::{OffsetMapping, RenderResult};
use super::paragraph::{ParagraphRender, hash_source, rope_slice_to_string};
use super::writer::EditorWriter;
use jumprope::JumpRopeBuf;
use markdown_weaver::Parser;
use std::ops::Range;

/// Cache for incremental paragraph rendering.
/// Stores previously rendered paragraphs to avoid re-rendering unchanged content.
#[derive(Clone, Debug, Default)]
pub struct RenderCache {
    /// Cached paragraph renders (content paragraphs only, gaps computed fresh)
    pub paragraphs: Vec<CachedParagraph>,
    /// Next available node ID for fresh renders
    pub next_node_id: usize,
}

/// A cached paragraph render that can be reused if source hasn't changed.
#[derive(Clone, Debug)]
pub struct CachedParagraph {
    /// Hash of paragraph source text for change detection
    pub source_hash: u64,
    /// Byte range in source document
    pub byte_range: Range<usize>,
    /// Char range in source document
    pub char_range: Range<usize>,
    /// Rendered HTML
    pub html: String,
    /// Offset mappings for cursor positioning
    pub offset_map: Vec<OffsetMapping>,
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

    for (_idx, (byte_range, char_range)) in paragraph_ranges.iter().enumerate() {
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

    // Insert gap paragraphs for EXTRA whitespace between blocks.
    // Standard paragraph break is 2 newlines (\n\n) - no gap needed for that.
    // Gaps are only for whitespace BEYOND the minimum, giving cursor a landing spot.
    // Gap IDs are position-based for stability across renders.
    const MIN_PARAGRAPH_BREAK: usize = 2; // \n\n

    let mut paragraphs_with_gaps = Vec::with_capacity(paragraphs.len() * 2);
    let mut prev_end_char = 0usize;
    let mut prev_end_byte = 0usize;

    for para in paragraphs {
        // Check for gap before this paragraph - only if MORE than minimum break
        let gap_size = para.char_range.start.saturating_sub(prev_end_char);
        if gap_size > MIN_PARAGRAPH_BREAK {
            // Gap covers the EXTRA whitespace beyond the minimum break
            let gap_start_char = prev_end_char + MIN_PARAGRAPH_BREAK;
            let gap_end_char = para.char_range.start;
            let gap_start_byte = prev_end_byte + MIN_PARAGRAPH_BREAK;
            let gap_end_byte = para.byte_range.start;

            // Position-based ID: deterministic, stable across cache states
            let gap_node_id = format!("gap-{}-{}", gap_start_char, gap_end_char);
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
            // Position-based ID for trailing gap
            let trailing_node_id = format!("gap-{}-{}", prev_end_char, doc_end_char);
            let trailing_html = format!(r#"<span id="{}">{}</span>"#, trailing_node_id, '\u{200B}');

            paragraphs_with_gaps.push(ParagraphRender {
                byte_range: prev_end_byte..doc_end_byte,
                char_range: prev_end_char..doc_end_char,
                html: trailing_html,
                offset_map: vec![OffsetMapping {
                    byte_range: prev_end_byte..doc_end_byte,
                    char_range: prev_end_char..doc_end_char,
                    node_id: trailing_node_id,
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

/// Check if an edit affects paragraph boundaries.
///
/// Edits that don't contain newlines and aren't in the block-syntax zone
/// are considered "safe" and can skip boundary rediscovery.
fn is_boundary_affecting(edit: &EditInfo) -> bool {
    // Newlines always affect boundaries (paragraph splits/joins)
    if edit.contains_newline {
        return true;
    }

    // Edits in the block-syntax zone (first ~6 chars of line) could affect
    // headings, lists, blockquotes, code fences, etc.
    if edit.in_block_syntax_zone {
        return true;
    }

    false
}

/// Adjust a cached paragraph's positions after an earlier edit.
fn adjust_paragraph_positions(
    cached: &CachedParagraph,
    char_delta: isize,
    byte_delta: isize,
) -> ParagraphRender {
    let mut adjusted_map = cached.offset_map.clone();
    for mapping in &mut adjusted_map {
        mapping.char_range.start = (mapping.char_range.start as isize + char_delta) as usize;
        mapping.char_range.end = (mapping.char_range.end as isize + char_delta) as usize;
        mapping.byte_range.start = (mapping.byte_range.start as isize + byte_delta) as usize;
        mapping.byte_range.end = (mapping.byte_range.end as isize + byte_delta) as usize;
    }

    ParagraphRender {
        byte_range: (cached.byte_range.start as isize + byte_delta) as usize
            ..(cached.byte_range.end as isize + byte_delta) as usize,
        char_range: (cached.char_range.start as isize + char_delta) as usize
            ..(cached.char_range.end as isize + char_delta) as usize,
        html: cached.html.clone(),
        offset_map: adjusted_map,
        source_hash: cached.source_hash,
    }
}

/// Render markdown with incremental caching.
///
/// Uses cached paragraph renders when possible, only re-rendering changed paragraphs.
/// For "safe" edits (no boundary changes), skips boundary rediscovery entirely.
///
/// # Arguments
/// - `rope`: The document rope to render
/// - `cache`: Previous render cache (if any)
/// - `edit`: Information about the most recent edit (if any)
///
/// # Returns
/// Tuple of (rendered paragraphs, updated cache)
pub fn render_paragraphs_incremental(
    rope: &JumpRopeBuf,
    cache: Option<&RenderCache>,
    edit: Option<&EditInfo>,
) -> (Vec<ParagraphRender>, RenderCache) {
    let source = rope.to_string();

    // Handle empty document
    if source.is_empty() {
        let empty_node_id = "n0".to_string();
        let empty_html = format!(r#"<span id="{}">{}</span>"#, empty_node_id, '\u{200B}');

        let para = ParagraphRender {
            byte_range: 0..0,
            char_range: 0..0,
            html: empty_html.clone(),
            offset_map: vec![],
            source_hash: 0,
        };

        let new_cache = RenderCache {
            paragraphs: vec![CachedParagraph {
                source_hash: 0,
                byte_range: 0..0,
                char_range: 0..0,
                html: empty_html,
                offset_map: vec![],
            }],
            next_node_id: 1,
        };

        return (vec![para], new_cache);
    }

    // Determine if we can use fast path (skip boundary discovery)
    let use_fast_path = cache.is_some() && edit.is_some() && !is_boundary_affecting(edit.unwrap());

    // Get paragraph boundaries
    let paragraph_ranges = if use_fast_path {
        // Fast path: adjust cached boundaries based on edit
        let cache = cache.unwrap();
        let edit = edit.unwrap();

        // Find which paragraph the edit falls into
        let edit_pos = edit.edit_char_pos;
        let char_delta = edit.inserted_len as isize - edit.deleted_len as isize;

        // Adjust each cached paragraph's range
        cache
            .paragraphs
            .iter()
            .map(|p| {
                if p.char_range.end <= edit_pos {
                    // Before edit - no change
                    (p.byte_range.clone(), p.char_range.clone())
                } else if p.char_range.start >= edit_pos {
                    // After edit - shift by delta
                    // Calculate byte delta (approximation: assume 1 byte per char for ASCII)
                    // This is imprecise but boundaries are rediscovered on slow path anyway
                    let byte_delta = char_delta; // TODO: proper byte calculation
                    (
                        (p.byte_range.start as isize + byte_delta) as usize
                            ..(p.byte_range.end as isize + byte_delta) as usize,
                        (p.char_range.start as isize + char_delta) as usize
                            ..(p.char_range.end as isize + char_delta) as usize,
                    )
                } else {
                    // Edit is within this paragraph - expand its end
                    (
                        p.byte_range.start..(p.byte_range.end as isize + char_delta) as usize,
                        p.char_range.start..(p.char_range.end as isize + char_delta) as usize,
                    )
                }
            })
            .collect::<Vec<_>>()
    } else {
        // Slow path: run boundary-only pass to discover paragraph boundaries
        let parser =
            Parser::new_ext(&source, weaver_renderer::default_md_options()).into_offset_iter();
        let mut scratch_output = String::new();

        match EditorWriter::<_, _, ()>::new_boundary_only(
            &source,
            rope,
            parser,
            &mut scratch_output,
        )
        .run()
        {
            Ok(result) => result.paragraph_ranges,
            Err(_) => return (Vec::new(), RenderCache::default()),
        }
    };

    // Render paragraphs, reusing cache where possible
    let mut paragraphs = Vec::with_capacity(paragraph_ranges.len());
    let mut new_cached = Vec::with_capacity(paragraph_ranges.len());
    let mut node_id_offset = cache.map(|c| c.next_node_id).unwrap_or(0);

    for (byte_range, char_range) in paragraph_ranges.iter() {
        let para_source = rope_slice_to_string(rope, char_range.clone());
        let source_hash = hash_source(&para_source);

        // Check if we have a cached render with matching hash
        let cached_match =
            cache.and_then(|c| c.paragraphs.iter().find(|p| p.source_hash == source_hash));

        let (html, offset_map) = if let Some(cached) = cached_match {
            // Reuse cached HTML and offset map (adjusted for position)
            let char_delta = char_range.start as isize - cached.char_range.start as isize;
            let byte_delta = byte_range.start as isize - cached.byte_range.start as isize;

            let mut adjusted_map = cached.offset_map.clone();
            for mapping in &mut adjusted_map {
                mapping.char_range.start =
                    (mapping.char_range.start as isize + char_delta) as usize;
                mapping.char_range.end = (mapping.char_range.end as isize + char_delta) as usize;
                mapping.byte_range.start =
                    (mapping.byte_range.start as isize + byte_delta) as usize;
                mapping.byte_range.end = (mapping.byte_range.end as isize + byte_delta) as usize;
            }

            (cached.html.clone(), adjusted_map)
        } else {
            // Fresh render needed
            let para_rope = JumpRopeBuf::from(para_source.as_str());
            let parser = Parser::new_ext(&para_source, weaver_renderer::default_md_options())
                .into_offset_iter();
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
                    // Update node ID offset
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

            // Adjust offsets to document coordinates
            let para_char_start = char_range.start;
            let para_byte_start = byte_range.start;
            for mapping in &mut offset_map {
                mapping.byte_range.start += para_byte_start;
                mapping.byte_range.end += para_byte_start;
                mapping.char_range.start += para_char_start;
                mapping.char_range.end += para_char_start;
            }

            (output, offset_map)
        };

        // Store in cache
        new_cached.push(CachedParagraph {
            source_hash,
            byte_range: byte_range.clone(),
            char_range: char_range.clone(),
            html: html.clone(),
            offset_map: offset_map.clone(),
        });

        paragraphs.push(ParagraphRender {
            byte_range: byte_range.clone(),
            char_range: char_range.clone(),
            html,
            offset_map,
            source_hash,
        });
    }

    // Insert gap paragraphs for EXTRA whitespace between blocks.
    // Standard paragraph break is 2 newlines (\n\n) - no gap needed for that.
    // Gaps are only for whitespace BEYOND the minimum, giving cursor a landing spot.
    const MIN_PARAGRAPH_BREAK_INCR: usize = 2; // \n\n

    let mut paragraphs_with_gaps = Vec::with_capacity(paragraphs.len() * 2);
    let mut prev_end_char = 0usize;
    let mut prev_end_byte = 0usize;

    for para in paragraphs {
        // Check for gap before this paragraph - only if MORE than minimum break
        let gap_size = para.char_range.start.saturating_sub(prev_end_char);
        if gap_size > MIN_PARAGRAPH_BREAK_INCR {
            // Gap covers the EXTRA whitespace beyond the minimum break
            let gap_start_char = prev_end_char + MIN_PARAGRAPH_BREAK_INCR;
            let gap_end_char = para.char_range.start;
            let gap_start_byte = prev_end_byte + MIN_PARAGRAPH_BREAK_INCR;
            let gap_end_byte = para.byte_range.start;

            // Position-based ID: deterministic, stable across cache states
            let gap_node_id = format!("gap-{}-{}", gap_start_char, gap_end_char);
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
                    utf16_len: 1,
                }],
                source_hash: hash_source(&rope_slice_to_string(rope, gap_start_char..gap_end_char)),
            });
        }

        prev_end_char = para.char_range.end;
        prev_end_byte = para.byte_range.end;
        paragraphs_with_gaps.push(para);
    }

    // Add trailing gap if needed
    let has_trailing_newlines = source.ends_with("\n\n") || source.ends_with("\n");
    if has_trailing_newlines {
        let doc_end_char = rope.len_chars();
        let doc_end_byte = rope.len_bytes();

        if doc_end_char > prev_end_char {
            // Position-based ID for trailing gap
            let trailing_node_id = format!("gap-{}-{}", prev_end_char, doc_end_char);
            let trailing_html = format!(r#"<span id="{}">{}</span>"#, trailing_node_id, '\u{200B}');

            paragraphs_with_gaps.push(ParagraphRender {
                byte_range: prev_end_byte..doc_end_byte,
                char_range: prev_end_char..doc_end_char,
                html: trailing_html,
                offset_map: vec![OffsetMapping {
                    byte_range: prev_end_byte..doc_end_byte,
                    char_range: prev_end_char..doc_end_char,
                    node_id: trailing_node_id,
                    char_offset_in_node: 0,
                    child_index: None,
                    utf16_len: 1,
                }],
                source_hash: 0,
            });
        }
    }

    let new_cache = RenderCache {
        paragraphs: new_cached,
        next_node_id: node_id_offset,
    };

    (paragraphs_with_gaps, new_cache)
}
