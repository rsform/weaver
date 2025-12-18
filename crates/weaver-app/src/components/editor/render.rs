//! Markdown rendering for the editor.
//!
//! Phase 2: Paragraph-level incremental rendering with formatting characters visible.
//!
//! Uses EditorWriter which tracks gaps in offset_iter to preserve formatting characters.

use super::document::EditInfo;
use super::offset_map::{OffsetMapping, RenderResult};
use super::paragraph::{ParagraphRender, hash_source, make_paragraph_id, text_slice_to_string};
use super::writer::{EditorImageResolver, EditorWriter, ImageResolver, SyntaxSpanInfo};
use loro::LoroText;
use markdown_weaver::Parser;
use std::collections::HashMap;
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
    /// Next available paragraph ID (monotonic counter)
    pub next_para_id: usize,
}

/// A cached paragraph render that can be reused if source hasn't changed.
#[derive(Clone, Debug)]
pub struct CachedParagraph {
    /// Stable monotonic ID for DOM element identity
    pub id: String,
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
    /// Collected refs (wikilinks, AT embeds) from this paragraph
    pub collected_refs: Vec<weaver_common::ExtractedRef>,
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

/// Insert gap paragraphs for extra whitespace between blocks.
fn add_gap_paragraphs(
    paragraphs: Vec<ParagraphRender>,
    text: &LoroText,
    source: &str,
) -> Vec<ParagraphRender> {
    const MIN_PARAGRAPH_BREAK_INCR: usize = 2; // \n\n

    let mut paragraphs_with_gaps = Vec::with_capacity(paragraphs.len() * 2);
    let mut prev_end_char = 0usize;
    let mut prev_end_byte = 0usize;

    for para in paragraphs {
        let gap_size = para.char_range.start.saturating_sub(prev_end_char);
        if gap_size > MIN_PARAGRAPH_BREAK_INCR {
            let gap_start_char = prev_end_char + MIN_PARAGRAPH_BREAK_INCR;
            let gap_end_char = para.char_range.start;
            let gap_start_byte = prev_end_byte + MIN_PARAGRAPH_BREAK_INCR;
            let gap_end_byte = para.byte_range.start;

            let gap_node_id = format!("gap-{}-{}", gap_start_char, gap_end_char);
            let gap_html = format!(r#"<span id="{}">{}</span>"#, gap_node_id, '\u{200B}');

            paragraphs_with_gaps.push(ParagraphRender {
                id: gap_node_id.clone(),
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
            let trailing_node_id = format!("gap-{}-{}", prev_end_char, doc_end_char);
            let trailing_html = format!(r#"<span id="{}">{}</span>"#, trailing_node_id, '\u{200B}');

            paragraphs_with_gaps.push(ParagraphRender {
                id: trailing_node_id.clone(),
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

    paragraphs_with_gaps
}

/// Render markdown with incremental caching.
///
/// Uses cached paragraph renders when possible, only re-rendering changed paragraphs.
///
/// # Parameters
/// - `cursor_offset`: Current cursor position (for finding which NEW paragraph is the cursor para)
/// - `edit`: Edit info for stable ID assignment. Uses `edit_char_pos` to find which OLD cached
///   paragraph to reuse the ID from (since cursor may have moved after the edit).
/// - `entry_index`: Optional index for wikilink validation (adds link-valid/link-broken classes)
/// - `resolved_content`: Pre-resolved embed content for sync rendering
///
/// # Returns
/// (paragraphs, cache, collected_refs) - collected_refs contains wikilinks and AT embeds found during render
pub fn render_paragraphs_incremental(
    text: &LoroText,
    cache: Option<&RenderCache>,
    cursor_offset: usize,
    edit: Option<&EditInfo>,
    image_resolver: Option<&EditorImageResolver>,
    entry_index: Option<&EntryIndex>,
    resolved_content: &ResolvedContent,
) -> (
    Vec<ParagraphRender>,
    RenderCache,
    Vec<weaver_common::ExtractedRef>,
) {
    let fn_start = crate::perf::now();
    let source = text.to_string();

    // Handle empty document
    if source.is_empty() {
        let empty_node_id = "n0".to_string();
        let empty_html = format!(r#"<span id="{}">{}</span>"#, empty_node_id, '\u{200B}');
        let para_id = make_paragraph_id(0);

        let para = ParagraphRender {
            id: para_id.clone(),
            byte_range: 0..0,
            char_range: 0..0,
            html: empty_html.clone(),
            offset_map: vec![],
            syntax_spans: vec![],
            source_hash: 0,
        };

        let new_cache = RenderCache {
            paragraphs: vec![CachedParagraph {
                id: para_id,
                source_hash: 0,
                byte_range: 0..0,
                char_range: 0..0,
                html: empty_html,
                offset_map: vec![],
                syntax_spans: vec![],
                collected_refs: vec![],
            }],
            next_node_id: 1,
            next_syn_id: 0,
            next_para_id: 1,
        };

        return (vec![para], new_cache, vec![]);
    }

    // Determine if we can use fast path (skip boundary discovery)
    // Need cache and non-boundary-affecting edit info (for edit position)
    let current_len = text.len_unicode();
    let current_byte_len = text.len_utf8();

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
        let (cached_len, cached_byte_len) = cache
            .paragraphs
            .last()
            .map(|p| (p.char_range.end, p.byte_range.end))
            .unwrap_or((0, 0));
        let char_delta = current_len as isize - cached_len as isize;
        let byte_delta = current_byte_len as isize - cached_byte_len as isize;

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
                    (
                        apply_delta(p.byte_range.start, byte_delta)
                            ..apply_delta(p.byte_range.end, byte_delta),
                        apply_delta(p.char_range.start, char_delta)
                            ..apply_delta(p.char_range.end, char_delta),
                    )
                } else {
                    // Edit is at or within this paragraph - expand its end
                    (
                        p.byte_range.start..apply_delta(p.byte_range.end, byte_delta),
                        p.char_range.start..apply_delta(p.char_range.end, char_delta),
                    )
                }
            })
            .collect::<Vec<_>>()
    } else {
        vec![] // Will be populated by slow path below
    };

    // Validate fast path results - if any ranges are invalid, use slow path
    let use_fast_path = if !paragraph_ranges.is_empty() {
        let all_valid = paragraph_ranges
            .iter()
            .all(|(_, char_range)| char_range.start <= char_range.end);
        if !all_valid {
            tracing::debug!(
                target: "weaver::render",
                "fast path produced invalid ranges, falling back to slow path"
            );
            false
        } else {
            true
        }
    } else {
        false
    };

    // ============ FAST PATH ============
    // Reuse cached paragraphs with offset adjustment, only re-render cursor paragraph
    if use_fast_path {
        let fast_start = crate::perf::now();
        let cache = cache.unwrap();
        let edit = edit.unwrap();
        let edit_pos = edit.edit_char_pos;

        // Compute deltas
        let (cached_len, cached_byte_len) = cache
            .paragraphs
            .last()
            .map(|p| (p.char_range.end, p.byte_range.end))
            .unwrap_or((0, 0));
        let char_delta = current_len as isize - cached_len as isize;
        let byte_delta = current_byte_len as isize - cached_byte_len as isize;

        // Find cursor paragraph index
        let cursor_para_idx = cache
            .paragraphs
            .iter()
            .position(|p| p.char_range.start <= edit_pos && edit_pos <= p.char_range.end);

        let mut paragraphs = Vec::with_capacity(cache.paragraphs.len());
        let mut new_cached = Vec::with_capacity(cache.paragraphs.len());
        let mut all_refs: Vec<weaver_common::ExtractedRef> = Vec::new();

        for (idx, cached_para) in cache.paragraphs.iter().enumerate() {
            let is_cursor_para = Some(idx) == cursor_para_idx;

            // Adjust ranges based on position relative to edit
            let (byte_range, char_range) = if cached_para.char_range.end < edit_pos {
                // Before edit - no change
                (cached_para.byte_range.clone(), cached_para.char_range.clone())
            } else if cached_para.char_range.start > edit_pos {
                // After edit - shift by delta
                (
                    apply_delta(cached_para.byte_range.start, byte_delta)
                        ..apply_delta(cached_para.byte_range.end, byte_delta),
                    apply_delta(cached_para.char_range.start, char_delta)
                        ..apply_delta(cached_para.char_range.end, char_delta),
                )
            } else {
                // Contains edit - expand end
                (
                    cached_para.byte_range.start..apply_delta(cached_para.byte_range.end, byte_delta),
                    cached_para.char_range.start..apply_delta(cached_para.char_range.end, char_delta),
                )
            };

            let para_source = text_slice_to_string(text, char_range.clone());
            let source_hash = hash_source(&para_source);

            if is_cursor_para {
                // Re-render cursor paragraph for fresh syntax detection
                let resolver = image_resolver.cloned().unwrap_or_default();
                let parser = Parser::new_ext(&para_source, weaver_renderer::default_md_options())
                    .into_offset_iter();

                let para_doc = loro::LoroDoc::new();
                let para_text = para_doc.get_text("content");
                let _ = para_text.insert(0, &para_source);

                let mut writer = EditorWriter::<_, &ResolvedContent, &EditorImageResolver>::new(
                    &para_source,
                    &para_text,
                    parser,
                )
                .with_image_resolver(&resolver)
                .with_embed_provider(resolved_content);

                if let Some(idx) = entry_index {
                    writer = writer.with_entry_index(idx);
                }

                let (html, offset_map, syntax_spans, para_refs) = match writer.run() {
                    Ok(result) => {
                        // Adjust offsets to be document-absolute
                        let mut offset_map = result.offset_maps_by_paragraph.into_iter().next().unwrap_or_default();
                        for m in &mut offset_map {
                            m.char_range.start += char_range.start;
                            m.char_range.end += char_range.start;
                            m.byte_range.start += byte_range.start;
                            m.byte_range.end += byte_range.start;
                        }
                        let mut syntax_spans = result.syntax_spans_by_paragraph.into_iter().next().unwrap_or_default();
                        for s in &mut syntax_spans {
                            s.adjust_positions(char_range.start as isize);
                        }
                        let para_refs = result.collected_refs_by_paragraph.into_iter().next().unwrap_or_default();
                        let html = result.html_segments.into_iter().next().unwrap_or_default();
                        (html, offset_map, syntax_spans, para_refs)
                    }
                    Err(_) => (String::new(), Vec::new(), Vec::new(), Vec::new()),
                };

                all_refs.extend(para_refs.clone());

                new_cached.push(CachedParagraph {
                    id: cached_para.id.clone(),
                    source_hash,
                    byte_range: byte_range.clone(),
                    char_range: char_range.clone(),
                    html: html.clone(),
                    offset_map: offset_map.clone(),
                    syntax_spans: syntax_spans.clone(),
                    collected_refs: para_refs.clone(),
                });

                paragraphs.push(ParagraphRender {
                    id: cached_para.id.clone(),
                    byte_range,
                    char_range,
                    html,
                    offset_map,
                    syntax_spans,
                    source_hash,
                });
            } else {
                // Reuse cached with adjusted offsets
                let mut offset_map = cached_para.offset_map.clone();
                let mut syntax_spans = cached_para.syntax_spans.clone();

                if cached_para.char_range.start > edit_pos {
                    // After edit - adjust offsets
                    for m in &mut offset_map {
                        m.char_range.start = apply_delta(m.char_range.start, char_delta);
                        m.char_range.end = apply_delta(m.char_range.end, char_delta);
                        m.byte_range.start = apply_delta(m.byte_range.start, byte_delta);
                        m.byte_range.end = apply_delta(m.byte_range.end, byte_delta);
                    }
                    for s in &mut syntax_spans {
                        s.adjust_positions(char_delta);
                    }
                }

                all_refs.extend(cached_para.collected_refs.clone());

                new_cached.push(CachedParagraph {
                    id: cached_para.id.clone(),
                    source_hash,
                    byte_range: byte_range.clone(),
                    char_range: char_range.clone(),
                    html: cached_para.html.clone(),
                    offset_map: offset_map.clone(),
                    syntax_spans: syntax_spans.clone(),
                    collected_refs: cached_para.collected_refs.clone(),
                });

                paragraphs.push(ParagraphRender {
                    id: cached_para.id.clone(),
                    byte_range,
                    char_range,
                    html: cached_para.html.clone(),
                    offset_map,
                    syntax_spans,
                    source_hash,
                });
            }
        }

        // Add gaps (reuse gap logic from below)
        let paragraphs_with_gaps = add_gap_paragraphs(paragraphs, text, &source);

        let new_cache = RenderCache {
            paragraphs: new_cached,
            next_node_id: 0,
            next_syn_id: 0,
            next_para_id: cache.next_para_id,
        };

        let fast_ms = crate::perf::now() - fast_start;
        tracing::debug!(
            fast_ms,
            paragraphs = paragraphs_with_gaps.len(),
            cursor_para_idx,
            "fast path render timing"
        );

        return (paragraphs_with_gaps, new_cache, all_refs);
    }

    // ============ SLOW PATH ============
    // Full render when boundaries might have changed
    let render_start = crate::perf::now();
    let parser =
        Parser::new_ext(&source, weaver_renderer::default_md_options()).into_offset_iter();

    // Use provided resolver or empty default
    let resolver = image_resolver.cloned().unwrap_or_default();

    // Build writer with all resolvers
    let mut writer = EditorWriter::<_, &ResolvedContent, &EditorImageResolver>::new(
        &source,
        text,
        parser,
    )
    .with_image_resolver(&resolver)
    .with_embed_provider(resolved_content);

    if let Some(idx) = entry_index {
        writer = writer.with_entry_index(idx);
    }

    let writer_result = match writer.run() {
        Ok(result) => result,
        Err(_) => return (Vec::new(), RenderCache::default(), vec![]),
    };

    let render_ms = crate::perf::now() - render_start;

    let paragraph_ranges = writer_result.paragraph_ranges.clone();

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

    // Build paragraphs from full render segments
    let build_start = crate::perf::now();
    let mut paragraphs = Vec::with_capacity(paragraph_ranges.len());
    let mut new_cached = Vec::with_capacity(paragraph_ranges.len());
    let mut all_refs: Vec<weaver_common::ExtractedRef> = Vec::new();
    let mut next_para_id = cache.map(|c| c.next_para_id).unwrap_or(0);

    // Find which paragraph contains cursor (for stable ID assignment)
    let cursor_para_idx = paragraph_ranges.iter().position(|(_, char_range)| {
        char_range.start <= cursor_offset && cursor_offset <= char_range.end
    });

    tracing::debug!(
        cursor_offset,
        ?cursor_para_idx,
        edit_char_pos = ?edit.map(|e| e.edit_char_pos),
        "ID assignment: cursor and edit info"
    );

    // Build hash->cached_para lookup for non-cursor matching
    let cached_by_hash: HashMap<u64, &CachedParagraph> = cache
        .map(|c| c.paragraphs.iter().map(|p| (p.source_hash, p)).collect())
        .unwrap_or_default();

    for (idx, (byte_range, char_range)) in paragraph_ranges.iter().enumerate() {
        let para_source = text_slice_to_string(text, char_range.clone());
        let source_hash = hash_source(&para_source);
        let is_cursor_para = Some(idx) == cursor_para_idx;

        // ID assignment: cursor paragraph matches by edit position, others match by hash
        let para_id = if is_cursor_para {
            let edit_in_this_para = edit
                .map(|e| char_range.start <= e.edit_char_pos && e.edit_char_pos <= char_range.end)
                .unwrap_or(false);
            let lookup_pos = if edit_in_this_para {
                edit.map(|e| e.edit_char_pos).unwrap_or(cursor_offset)
            } else {
                cursor_offset
            };
            let found_cached = cache.and_then(|c| {
                c.paragraphs
                    .iter()
                    .find(|p| p.char_range.start <= lookup_pos && lookup_pos <= p.char_range.end)
            });

            if let Some(cached) = found_cached {
                tracing::debug!(
                    lookup_pos,
                    edit_in_this_para,
                    cursor_offset,
                    cached_id = %cached.id,
                    cached_range = ?cached.char_range,
                    "cursor para: reusing cached ID"
                );
                cached.id.clone()
            } else {
                let id = make_paragraph_id(next_para_id);
                next_para_id += 1;
                id
            }
        } else {
            // Non-cursor: match by content hash
            cached_by_hash
                .get(&source_hash)
                .map(|p| p.id.clone())
                .unwrap_or_else(|| {
                    let id = make_paragraph_id(next_para_id);
                    next_para_id += 1;
                    id
                })
        };

        // Get data from full render segments
        let html = writer_result.html_segments.get(idx).cloned().unwrap_or_default();
        let offset_map = writer_result.offset_maps_by_paragraph.get(idx).cloned().unwrap_or_default();
        let syntax_spans = writer_result.syntax_spans_by_paragraph.get(idx).cloned().unwrap_or_default();
        let para_refs = writer_result.collected_refs_by_paragraph.get(idx).cloned().unwrap_or_default();

        all_refs.extend(para_refs.clone());

        // Store in cache
        new_cached.push(CachedParagraph {
            id: para_id.clone(),
            source_hash,
            byte_range: byte_range.clone(),
            char_range: char_range.clone(),
            html: html.clone(),
            offset_map: offset_map.clone(),
            syntax_spans: syntax_spans.clone(),
            collected_refs: para_refs.clone(),
        });

        paragraphs.push(ParagraphRender {
            id: para_id,
            byte_range: byte_range.clone(),
            char_range: char_range.clone(),
            html,
            offset_map,
            syntax_spans,
            source_hash,
        });
    }

    let build_ms = crate::perf::now() - build_start;
    tracing::debug!(
        render_ms,
        build_ms,
        paragraphs = paragraph_ranges.len(),
        "single-pass render timing"
    );

    let paragraphs_with_gaps = add_gap_paragraphs(paragraphs, text, &source);

    let new_cache = RenderCache {
        paragraphs: new_cached,
        next_node_id: 0, // Not used in single-pass mode
        next_syn_id: 0,  // Not used in single-pass mode
        next_para_id,
    };

    let total_ms = crate::perf::now() - fn_start;
    tracing::debug!(
        total_ms,
        render_ms,
        build_ms,
        paragraphs = paragraphs_with_gaps.len(),
        "render_paragraphs_incremental timing"
    );

    (paragraphs_with_gaps, new_cache, all_refs)
}
