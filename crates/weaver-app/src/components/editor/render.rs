//! Markdown rendering for the editor.
//!
//! Phase 2: Paragraph-level incremental rendering with formatting characters visible.
//!
//! Uses EditorWriter which tracks gaps in offset_iter to preserve formatting characters.

use super::document::EditInfo;
use super::offset_map::{OffsetMapping, RenderResult};
use super::paragraph::{ParagraphRender, hash_source, text_slice_to_string};
use super::writer::{EditorImageResolver, EditorWriter, ImageResolver, SyntaxSpanInfo};
use loro::LoroText;
use markdown_weaver::Parser;
use std::ops::Range;
use weaver_common::{EntryIndex, ResolvedContent};

/// Cache for incremental paragraph rendering.
/// Stores previously rendered paragraphs to avoid re-rendering unchanged content.
#[derive(Clone, Debug, Default)]
pub struct RenderCache {
    /// Cached paragraph renders (content paragraphs only, gaps computed fresh)
    pub paragraphs: Vec<CachedParagraph>,
    /// Next available node ID for fresh renders
    pub next_node_id: usize,
    /// Next available syntax span ID for fresh renders
    pub next_syn_id: usize,
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
    /// Syntax spans for conditional visibility
    pub syntax_spans: Vec<SyntaxSpanInfo>,
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

/// Apply a signed delta to a usize, saturating at 0 on underflow.
fn apply_delta(val: usize, delta: isize) -> usize {
    if delta >= 0 {
        val.saturating_add(delta as usize)
    } else {
        val.saturating_sub((-delta) as usize)
    }
}

/// Adjust a cached paragraph's positions after an earlier edit.
fn adjust_paragraph_positions(
    cached: &CachedParagraph,
    char_delta: isize,
    byte_delta: isize,
) -> ParagraphRender {
    let mut adjusted_map = cached.offset_map.clone();
    for mapping in &mut adjusted_map {
        mapping.char_range.start = apply_delta(mapping.char_range.start, char_delta);
        mapping.char_range.end = apply_delta(mapping.char_range.end, char_delta);
        mapping.byte_range.start = apply_delta(mapping.byte_range.start, byte_delta);
        mapping.byte_range.end = apply_delta(mapping.byte_range.end, byte_delta);
    }

    let mut adjusted_syntax = cached.syntax_spans.clone();
    for span in &mut adjusted_syntax {
        span.adjust_positions(char_delta);
    }

    ParagraphRender {
        byte_range: apply_delta(cached.byte_range.start, byte_delta)
            ..apply_delta(cached.byte_range.end, byte_delta),
        char_range: apply_delta(cached.char_range.start, char_delta)
            ..apply_delta(cached.char_range.end, char_delta),
        html: cached.html.clone(),
        offset_map: adjusted_map,
        syntax_spans: adjusted_syntax,
        source_hash: cached.source_hash,
    }
}

/// Render markdown with incremental caching.
///
/// Uses cached paragraph renders when possible, only re-rendering changed paragraphs.
/// For "safe" edits (no boundary changes), skips boundary rediscovery entirely.
///
/// # Parameters
/// - `entry_index`: Optional index for wikilink validation (adds link-valid/link-broken classes)
/// - `resolved_content`: Pre-resolved embed content for sync rendering
///
/// # Returns
/// (paragraphs, cache, collected_refs) - collected_refs contains wikilinks and AT embeds found during render
pub fn render_paragraphs_incremental(
    text: &LoroText,
    cache: Option<&RenderCache>,
    edit: Option<&EditInfo>,
    image_resolver: Option<&EditorImageResolver>,
    entry_index: Option<&EntryIndex>,
    resolved_content: &ResolvedContent,
) -> (
    Vec<ParagraphRender>,
    RenderCache,
    Vec<weaver_common::ExtractedRef>,
) {
    let source = text.to_string();

    // Handle empty document
    if source.is_empty() {
        let empty_node_id = "n0".to_string();
        let empty_html = format!(r#"<span id="{}">{}</span>"#, empty_node_id, '\u{200B}');

        let para = ParagraphRender {
            byte_range: 0..0,
            char_range: 0..0,
            html: empty_html.clone(),
            offset_map: vec![],
            syntax_spans: vec![],
            source_hash: 0,
        };

        let new_cache = RenderCache {
            paragraphs: vec![CachedParagraph {
                source_hash: 0,
                byte_range: 0..0,
                char_range: 0..0,
                html: empty_html,
                offset_map: vec![],
                syntax_spans: vec![],
            }],
            next_node_id: 1,
            next_syn_id: 0,
        };

        return (vec![para], new_cache, vec![]);
    }

    // Determine if we can use fast path (skip boundary discovery)
    // Need cache and non-boundary-affecting edit info (for edit position)
    let current_len = text.len_unicode();
    let use_fast_path = cache.is_some() && edit.is_some() && !is_boundary_affecting(edit.unwrap());

    tracing::debug!(
        target: "weaver::render",
        use_fast_path,
        has_cache = cache.is_some(),
        has_edit = edit.is_some(),
        boundary_affecting = edit.map(is_boundary_affecting),
        current_len,
        "render path decision"
    );

    // Get paragraph boundaries
    let paragraph_ranges = if use_fast_path {
        // Fast path: adjust cached boundaries based on actual length change
        let cache = cache.unwrap();
        let edit = edit.unwrap();

        // Find which paragraph the edit falls into
        let edit_pos = edit.edit_char_pos;

        // Compute delta from actual length difference, not edit info
        // This handles stale edits gracefully (delta = 0 if lengths match)
        let cached_len = cache
            .paragraphs
            .last()
            .map(|p| p.char_range.end)
            .unwrap_or(0);
        let char_delta = current_len as isize - cached_len as isize;

        // Adjust each cached paragraph's range
        cache
            .paragraphs
            .iter()
            .map(|p| {
                if p.char_range.end < edit_pos {
                    // Before edit - no change (edit is strictly after this paragraph)
                    (p.byte_range.clone(), p.char_range.clone())
                } else if p.char_range.start > edit_pos {
                    // After edit - shift by delta (edit is strictly before this paragraph)
                    // Calculate byte delta (approximation: assume 1 byte per char for ASCII)
                    // This is imprecise but boundaries are rediscovered on slow path anyway
                    let byte_delta = char_delta; // TODO: proper byte calculation
                    (
                        apply_delta(p.byte_range.start, byte_delta)
                            ..apply_delta(p.byte_range.end, byte_delta),
                        apply_delta(p.char_range.start, char_delta)
                            ..apply_delta(p.char_range.end, char_delta),
                    )
                } else {
                    // Edit is at or within this paragraph - expand its end
                    (
                        p.byte_range.start..apply_delta(p.byte_range.end, char_delta),
                        p.char_range.start..apply_delta(p.char_range.end, char_delta),
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
            text,
            parser,
            &mut scratch_output,
        )
        .run()
        {
            Ok(result) => result.paragraph_ranges,
            Err(_) => return (Vec::new(), RenderCache::default(), vec![]),
        }
    };

    // Log discovered paragraphs
    for (i, (byte_range, char_range)) in paragraph_ranges.iter().enumerate() {
        let preview: String = text_slice_to_string(text, char_range.clone())
            .chars()
            .take(30)
            .collect();
        tracing::trace!(
            target: "weaver::render",
            para_idx = i,
            char_range = ?char_range,
            byte_range = ?byte_range,
            preview = %preview,
            "paragraph boundary"
        );
    }

    // Render paragraphs, reusing cache where possible
    let mut paragraphs = Vec::with_capacity(paragraph_ranges.len());
    let mut new_cached = Vec::with_capacity(paragraph_ranges.len());
    let mut all_refs: Vec<weaver_common::ExtractedRef> = Vec::new();
    let mut node_id_offset = cache.map(|c| c.next_node_id).unwrap_or(0);
    let mut syn_id_offset = cache.map(|c| c.next_syn_id).unwrap_or(0);

    for (idx, (byte_range, char_range)) in paragraph_ranges.iter().enumerate() {
        let para_source = text_slice_to_string(text, char_range.clone());
        let source_hash = hash_source(&para_source);

        // Check if we have a cached render with matching hash
        let cached_match =
            cache.and_then(|c| c.paragraphs.iter().find(|p| p.source_hash == source_hash));

        let (html, offset_map, syntax_spans) = if let Some(cached) = cached_match {
            // Reuse cached HTML, offset map, and syntax spans (adjusted for position)
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

            let mut adjusted_syntax = cached.syntax_spans.clone();
            for span in &mut adjusted_syntax {
                span.adjust_positions(char_delta);
            }

            (cached.html.clone(), adjusted_map, adjusted_syntax)
        } else {
            // Fresh render needed - create detached LoroDoc for this paragraph
            let para_doc = loro::LoroDoc::new();
            let para_text = para_doc.get_text("content");
            let _ = para_text.insert(0, &para_source);

            let parser = Parser::new_ext(&para_source, weaver_renderer::default_md_options())
                .into_offset_iter();
            let mut output = String::new();

            // Use provided resolver or empty default
            let resolver = image_resolver.cloned().unwrap_or_default();

            // Build writer with optional entry index for wikilink validation
            // Pass paragraph's document-level offsets so all embedded char/byte positions are absolute
            let mut writer =
                EditorWriter::<_, _, &ResolvedContent, &EditorImageResolver>::new_with_all_offsets(
                    &para_source,
                    &para_text,
                    parser,
                    &mut output,
                    node_id_offset,
                    syn_id_offset,
                    char_range.start,
                    byte_range.start,
                )
                .with_image_resolver(&resolver)
                .with_embed_provider(resolved_content);

            if let Some(idx) = entry_index {
                writer = writer.with_entry_index(idx);
            }

            let (mut offset_map, mut syntax_spans) = match writer.run() {
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

                    // Update syn ID offset
                    let max_syn_id = result
                        .syntax_spans
                        .iter()
                        .filter_map(|s| {
                            s.syn_id
                                .strip_prefix("s")
                                .and_then(|id| id.parse::<usize>().ok())
                        })
                        .max()
                        .unwrap_or(syn_id_offset.saturating_sub(1));
                    syn_id_offset = max_syn_id + 1;

                    // Collect refs from this paragraph
                    all_refs.extend(result.collected_refs);

                    (result.offset_maps, result.syntax_spans)
                }
                Err(_) => (Vec::new(), Vec::new()),
            };

            // Offsets are already document-absolute since we pass char_range.start/byte_range.start
            // to the writer constructor
            (output, offset_map, syntax_spans)
        };

        // Store in cache
        new_cached.push(CachedParagraph {
            source_hash,
            byte_range: byte_range.clone(),
            char_range: char_range.clone(),
            html: html.clone(),
            offset_map: offset_map.clone(),
            syntax_spans: syntax_spans.clone(),
        });

        paragraphs.push(ParagraphRender {
            byte_range: byte_range.clone(),
            char_range: char_range.clone(),
            html,
            offset_map,
            syntax_spans,
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
            // Visible gap element covers EXTRA whitespace beyond minimum break
            let gap_start_char = prev_end_char + MIN_PARAGRAPH_BREAK_INCR;
            let gap_end_char = para.char_range.start;
            let gap_start_byte = prev_end_byte + MIN_PARAGRAPH_BREAK_INCR;
            let gap_end_byte = para.byte_range.start;

            // Position-based ID: deterministic, stable across cache states
            let gap_node_id = format!("gap-{}-{}", gap_start_char, gap_end_char);
            let gap_html = format!(r#"<span id="{}">{}</span>"#, gap_node_id, '\u{200B}');

            // Gap paragraph covers ALL whitespace (like trailing gaps do)
            // so cursor anywhere in the inter-paragraph zone triggers restoration
            paragraphs_with_gaps.push(ParagraphRender {
                byte_range: prev_end_byte..gap_end_byte,
                char_range: prev_end_char..gap_end_char,
                html: gap_html,
                offset_map: vec![OffsetMapping {
                    byte_range: prev_end_byte..gap_end_byte,
                    char_range: prev_end_char..gap_end_char,
                    node_id: gap_node_id,
                    char_offset_in_node: 0,
                    child_index: None,
                    utf16_len: 1,
                }],
                syntax_spans: vec![],
                source_hash: hash_source(&text_slice_to_string(text, gap_start_char..gap_end_char)),
            });
        }

        prev_end_char = para.char_range.end;
        prev_end_byte = para.byte_range.end;
        paragraphs_with_gaps.push(para);
    }

    // Add trailing gap if needed
    let has_trailing_newlines = source.ends_with("\n\n") || source.ends_with("\n");
    if has_trailing_newlines {
        let doc_end_char = text.len_unicode();
        let doc_end_byte = text.len_utf8();

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
                syntax_spans: vec![],
                source_hash: 0,
            });
        }
    }

    let new_cache = RenderCache {
        paragraphs: new_cached,
        next_node_id: node_id_offset,
        next_syn_id: syn_id_offset,
    };

    (paragraphs_with_gaps, new_cache, all_refs)
}
