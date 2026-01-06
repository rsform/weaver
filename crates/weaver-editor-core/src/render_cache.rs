//! Render caching and incremental paragraph rendering.
//!
//! This module provides infrastructure for incremental markdown rendering,
//! caching paragraph renders to avoid re-rendering unchanged content.

use std::ops::Range;

use smol_str::SmolStr;

use crate::offset_map::OffsetMapping;
use crate::paragraph::{ParagraphRender, hash_source, make_paragraph_id};
use crate::syntax::SyntaxSpanInfo;
use crate::text::TextBuffer;
use crate::types::EditInfo;
use crate::writer::EditorWriter;
use crate::{EditorRope, EmbedContentProvider, ImageResolver};

use markdown_weaver::Parser;
use weaver_common::ExtractedRef;

/// Cache for incremental paragraph rendering.
/// Stores previously rendered paragraphs to avoid re-rendering unchanged content.
#[derive(Clone, Debug, Default)]
pub struct RenderCache {
    /// Cached paragraph renders (content paragraphs only, gaps computed fresh).
    pub paragraphs: Vec<CachedParagraph>,
    /// Next available node ID for fresh renders.
    pub next_node_id: usize,
    /// Next available syntax span ID for fresh renders.
    pub next_syn_id: usize,
    /// Next available paragraph ID (monotonic counter).
    pub next_para_id: usize,
}

/// A cached paragraph render that can be reused if source hasn't changed.
#[derive(Clone, Debug)]
pub struct CachedParagraph {
    /// Stable monotonic ID for DOM element identity.
    pub id: SmolStr,
    /// Hash of paragraph source text for change detection.
    pub source_hash: u64,
    /// Byte range in source document.
    pub byte_range: Range<usize>,
    /// Char range in source document.
    pub char_range: Range<usize>,
    /// Rendered HTML.
    pub html: String,
    /// Offset mappings for cursor positioning.
    pub offset_map: Vec<OffsetMapping>,
    /// Syntax spans for conditional visibility.
    pub syntax_spans: Vec<SyntaxSpanInfo>,
    /// Collected refs (wikilinks, AT embeds) from this paragraph.
    pub collected_refs: Vec<ExtractedRef>,
}

/// Check if an edit affects paragraph boundaries.
///
/// Edits that don't contain newlines and aren't in the block-syntax zone
/// are considered "safe" and can skip boundary rediscovery.
pub fn is_boundary_affecting(edit: &EditInfo) -> bool {
    // Newlines always affect boundaries (paragraph splits/joins).
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
pub fn apply_delta(val: usize, delta: isize) -> usize {
    if delta >= 0 {
        val.saturating_add(delta as usize)
    } else {
        val.saturating_sub((-delta) as usize)
    }
}

/// Result of incremental paragraph rendering.
pub struct IncrementalRenderResult {
    /// Rendered paragraphs.
    pub paragraphs: Vec<ParagraphRender>,
    /// Updated cache for next render.
    pub cache: RenderCache,
    /// Collected refs (wikilinks, AT embeds) found during render.
    pub collected_refs: Vec<ExtractedRef>,
}

/// Render markdown with incremental caching.
///
/// Uses cached paragraph renders when possible, only re-rendering changed paragraphs.
/// Generic over any `TextBuffer` implementation.
///
/// # Parameters
/// - `text`: The text buffer to render
/// - `cache`: Optional previous render cache
/// - `cursor_offset`: Current cursor position (for finding which NEW paragraph is the cursor para)
/// - `edit`: Edit info for stable ID assignment
/// - `image_resolver`: Optional image URL resolver
/// - `entry_index`: Optional index for wikilink validation
/// - `embed_provider`: Provider for embed content
///
/// # Returns
/// `IncrementalRenderResult` containing paragraphs, updated cache, and collected refs.
pub fn render_paragraphs_incremental<T, I, E>(
    text: &T,
    cache: Option<&RenderCache>,
    cursor_offset: usize,
    edit: Option<&EditInfo>,
    image_resolver: Option<&I>,
    entry_index: Option<&weaver_common::EntryIndex>,
    embed_provider: &E,
) -> IncrementalRenderResult
where
    T: TextBuffer,
    I: ImageResolver + Clone + Default,
    E: EmbedContentProvider,
{
    let source = text.to_string();

    // Log source entering renderer to detect ZWC/space issues.
    if tracing::enabled!(target: "weaver::render", tracing::Level::TRACE) {
        tracing::trace!(
            target: "weaver::render",
            source_len = source.len(),
            source_chars = source.chars().count(),
            source_content = %source.escape_debug(),
            "render_paragraphs: source entering renderer"
        );
    }

    // Handle empty document.
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

        return IncrementalRenderResult {
            paragraphs: vec![para],
            cache: new_cache,
            collected_refs: vec![],
        };
    }

    // Determine if we can use fast path (skip boundary discovery).
    let current_len = text.len_chars();
    let current_byte_len = text.len_bytes();

    // If we have cache but no edit, just return cached data (no re-render needed).
    // This happens on cursor position changes, clicks, etc.
    if let (Some(c), None) = (cache, edit) {
        let cached_len = c.paragraphs.last().map(|p| p.char_range.end).unwrap_or(0);
        if cached_len == current_len {
            tracing::trace!(
                target: "weaver::render",
                "no edit, returning cached paragraphs"
            );
            let paragraphs: Vec<ParagraphRender> = c
                .paragraphs
                .iter()
                .map(|p| ParagraphRender {
                    id: p.id.clone(),
                    byte_range: p.byte_range.clone(),
                    char_range: p.char_range.clone(),
                    html: p.html.clone(),
                    offset_map: p.offset_map.clone(),
                    syntax_spans: p.syntax_spans.clone(),
                    source_hash: p.source_hash,
                })
                .collect();
            return IncrementalRenderResult {
                paragraphs,
                cache: c.clone(),
                collected_refs: c
                    .paragraphs
                    .iter()
                    .flat_map(|p| p.collected_refs.clone())
                    .collect(),
            };
        }
    }

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

    // Get paragraph boundaries.
    let paragraph_ranges = if use_fast_path {
        // Fast path: adjust cached boundaries based on actual length change.
        let cache = cache.unwrap();
        let edit = edit.unwrap();

        let edit_pos = edit.edit_char_pos;

        let (cached_len, cached_byte_len) = cache
            .paragraphs
            .last()
            .map(|p| (p.char_range.end, p.byte_range.end))
            .unwrap_or((0, 0));
        let char_delta = current_len as isize - cached_len as isize;
        let byte_delta = current_byte_len as isize - cached_byte_len as isize;

        cache
            .paragraphs
            .iter()
            .map(|p| {
                if p.char_range.end < edit_pos {
                    (p.byte_range.clone(), p.char_range.clone())
                } else if p.char_range.start > edit_pos {
                    (
                        apply_delta(p.byte_range.start, byte_delta)
                            ..apply_delta(p.byte_range.end, byte_delta),
                        apply_delta(p.char_range.start, char_delta)
                            ..apply_delta(p.char_range.end, char_delta),
                    )
                } else {
                    (
                        p.byte_range.start..apply_delta(p.byte_range.end, byte_delta),
                        p.char_range.start..apply_delta(p.char_range.end, char_delta),
                    )
                }
            })
            .collect::<Vec<_>>()
    } else {
        vec![]
    };

    // Validate fast path results.
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
    if use_fast_path {
        let cache = cache.unwrap();
        let edit = edit.unwrap();
        let edit_pos = edit.edit_char_pos;

        let (cached_len, cached_byte_len) = cache
            .paragraphs
            .last()
            .map(|p| (p.char_range.end, p.byte_range.end))
            .unwrap_or((0, 0));
        let char_delta = current_len as isize - cached_len as isize;
        let byte_delta = current_byte_len as isize - cached_byte_len as isize;

        let cursor_para_idx = cache
            .paragraphs
            .iter()
            .position(|p| p.char_range.start <= edit_pos && edit_pos <= p.char_range.end);

        let mut paragraphs = Vec::with_capacity(cache.paragraphs.len());
        let mut new_cached = Vec::with_capacity(cache.paragraphs.len());
        let mut all_refs: Vec<ExtractedRef> = Vec::new();

        for (idx, cached_para) in cache.paragraphs.iter().enumerate() {
            let is_cursor_para = Some(idx) == cursor_para_idx;

            let (byte_range, char_range) = if cached_para.char_range.end < edit_pos {
                (
                    cached_para.byte_range.clone(),
                    cached_para.char_range.clone(),
                )
            } else if cached_para.char_range.start > edit_pos {
                (
                    apply_delta(cached_para.byte_range.start, byte_delta)
                        ..apply_delta(cached_para.byte_range.end, byte_delta),
                    apply_delta(cached_para.char_range.start, char_delta)
                        ..apply_delta(cached_para.char_range.end, char_delta),
                )
            } else {
                (
                    cached_para.byte_range.start
                        ..apply_delta(cached_para.byte_range.end, byte_delta),
                    cached_para.char_range.start
                        ..apply_delta(cached_para.char_range.end, char_delta),
                )
            };

            let para_source = text
                .slice(char_range.clone())
                .map(|s| s.to_string())
                .unwrap_or_default();
            let source_hash = hash_source(&para_source);

            if is_cursor_para {
                // Re-render cursor paragraph for fresh syntax detection.
                let resolver = image_resolver.cloned().unwrap_or_default();
                let parser = Parser::new_ext(&para_source, weaver_renderer::default_md_options())
                    .into_offset_iter();

                let para_rope = EditorRope::from(para_source.as_str());

                let mut writer = EditorWriter::<_, _, &E, &I, ()>::new(
                    &para_source,
                    &para_rope,
                    parser,
                )
                .with_node_id_prefix(&cached_para.id)
                .with_image_resolver(&resolver)
                .with_embed_provider(embed_provider);

                if let Some(idx) = entry_index {
                    writer = writer.with_entry_index(idx);
                }

                let (html, offset_map, syntax_spans, para_refs) = match writer.run() {
                    Ok(result) => {
                        let mut offset_map = result
                            .offset_maps_by_paragraph
                            .into_iter()
                            .next()
                            .unwrap_or_default();
                        for m in &mut offset_map {
                            m.char_range.start += char_range.start;
                            m.char_range.end += char_range.start;
                            m.byte_range.start += byte_range.start;
                            m.byte_range.end += byte_range.start;
                        }
                        let mut syntax_spans = result
                            .syntax_spans_by_paragraph
                            .into_iter()
                            .next()
                            .unwrap_or_default();
                        for s in &mut syntax_spans {
                            s.adjust_positions(char_range.start as isize);
                        }
                        let para_refs = result
                            .collected_refs_by_paragraph
                            .into_iter()
                            .next()
                            .unwrap_or_default();
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
                // Reuse cached with adjusted offsets.
                let mut offset_map = cached_para.offset_map.clone();
                let mut syntax_spans = cached_para.syntax_spans.clone();

                if cached_para.char_range.start > edit_pos {
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

        let new_cache = RenderCache {
            paragraphs: new_cached,
            next_node_id: 0,
            next_syn_id: 0,
            next_para_id: cache.next_para_id,
        };

        return IncrementalRenderResult {
            paragraphs,
            cache: new_cache,
            collected_refs: all_refs,
        };
    }

    // ============ SLOW PATH ============
    // Partial render: reuse cached paragraphs before edit, parse from affected to end.

    let (reused_paragraphs, parse_start_byte, parse_start_char) =
        if let (Some(c), Some(e)) = (cache, edit) {
            let edit_pos = e.edit_char_pos;
            let affected_idx = c
                .paragraphs
                .iter()
                .position(|p| p.char_range.end >= edit_pos);

            if let Some(mut idx) = affected_idx {
                const BOUNDARY_SLOP: usize = 3;
                let para_start = c.paragraphs[idx].char_range.start;
                if idx > 0 && edit_pos < para_start + BOUNDARY_SLOP {
                    idx -= 1;
                }

                if idx > 0 {
                    let reused: Vec<_> = c.paragraphs[..idx].to_vec();
                    let last_reused = &c.paragraphs[idx - 1];
                    tracing::trace!(
                        reused_count = idx,
                        parse_start_byte = last_reused.byte_range.end,
                        parse_start_char = last_reused.char_range.end,
                        "slow path: partial parse from affected paragraph"
                    );
                    (
                        reused,
                        last_reused.byte_range.end,
                        last_reused.char_range.end,
                    )
                } else {
                    (Vec::new(), 0, 0)
                }
            } else {
                if let Some(last) = c.paragraphs.last() {
                    let reused = c.paragraphs.clone();
                    (reused, last.byte_range.end, last.char_range.end)
                } else {
                    (Vec::new(), 0, 0)
                }
            }
        } else {
            (Vec::new(), 0, 0)
        };

    let parse_slice = &source[parse_start_byte..];
    let parser =
        Parser::new_ext(parse_slice, weaver_renderer::default_md_options()).into_offset_iter();

    let resolver = image_resolver.cloned().unwrap_or_default();
    let slice_rope = EditorRope::from(parse_slice);

    let reused_count = reused_paragraphs.len();
    let parsed_para_id_start = if reused_count == 0 {
        0
    } else {
        cache.map(|c| c.next_para_id).unwrap_or(0)
    };

    tracing::trace!(
        parsed_para_id_start,
        reused_count,
        "slow path: paragraph ID allocation"
    );

    let cursor_para_override: Option<(usize, SmolStr)> = cache.and_then(|c| {
        let cached_cursor_idx = c.paragraphs.iter().position(|p| {
            p.char_range.start <= cursor_offset && cursor_offset <= p.char_range.end
        })?;

        if cached_cursor_idx < reused_count {
            return None;
        }

        let cached_para = &c.paragraphs[cached_cursor_idx];
        let parsed_index = cached_cursor_idx - reused_count;

        tracing::trace!(
            cached_cursor_idx,
            reused_count,
            parsed_index,
            cached_id = %cached_para.id,
            "slow path: cursor paragraph override"
        );

        Some((parsed_index, cached_para.id.clone()))
    });

    let mut writer = EditorWriter::<_, _, &E, &I, ()>::new(parse_slice, &slice_rope, parser)
        .with_auto_incrementing_prefix(parsed_para_id_start)
        .with_image_resolver(&resolver)
        .with_embed_provider(embed_provider);

    if let Some((idx, ref prefix)) = cursor_para_override {
        writer = writer.with_static_prefix_at_index(idx, prefix);
    }

    if let Some(idx) = entry_index {
        writer = writer.with_entry_index(idx);
    }

    let writer_result = match writer.run() {
        Ok(result) => result,
        Err(_) => {
            return IncrementalRenderResult {
                paragraphs: Vec::new(),
                cache: RenderCache::default(),
                collected_refs: vec![],
            }
        }
    };

    let parsed_para_count = writer_result.paragraph_ranges.len();

    let parsed_paragraph_ranges: Vec<_> = writer_result
        .paragraph_ranges
        .iter()
        .map(|(byte_range, char_range)| {
            (
                (byte_range.start + parse_start_byte)..(byte_range.end + parse_start_byte),
                (char_range.start + parse_start_char)..(char_range.end + parse_start_char),
            )
        })
        .collect();

    let paragraph_ranges: Vec<_> = reused_paragraphs
        .iter()
        .map(|p| (p.byte_range.clone(), p.char_range.clone()))
        .chain(parsed_paragraph_ranges.clone())
        .collect();

    if tracing::enabled!(tracing::Level::TRACE) {
        for (i, (byte_range, char_range)) in paragraph_ranges.iter().enumerate() {
            let preview: String = text
                .slice(char_range.clone())
                .map(|s| s.chars().take(30).collect())
                .unwrap_or_default();
            tracing::trace!(
                target: "weaver::render",
                para_idx = i,
                char_range = ?char_range,
                byte_range = ?byte_range,
                preview = %preview,
                "paragraph boundary"
            );
        }
    }

    let mut paragraphs = Vec::with_capacity(paragraph_ranges.len());
    let mut new_cached = Vec::with_capacity(paragraph_ranges.len());
    let mut all_refs: Vec<ExtractedRef> = Vec::new();
    let next_para_id = parsed_para_id_start + parsed_para_count;
    let reused_count = reused_paragraphs.len();

    let cursor_para_idx = paragraph_ranges.iter().position(|(_, char_range)| {
        char_range.start <= cursor_offset && cursor_offset <= char_range.end
    });

    tracing::trace!(
        cursor_offset,
        ?cursor_para_idx,
        edit_char_pos = ?edit.map(|e| e.edit_char_pos),
        reused_count,
        parsed_count = parsed_paragraph_ranges.len(),
        "ID assignment: cursor and edit info"
    );

    for (idx, (byte_range, char_range)) in paragraph_ranges.iter().enumerate() {
        let para_source = text
            .slice(char_range.clone())
            .map(|s| s.to_string())
            .unwrap_or_default();
        let source_hash = hash_source(&para_source);
        let is_cursor_para = Some(idx) == cursor_para_idx;

        let is_reused = idx < reused_count;

        let para_id = if is_reused {
            reused_paragraphs[idx].id.clone()
        } else {
            let parsed_idx = idx - reused_count;

            let id = if let Some((override_idx, ref override_prefix)) = cursor_para_override {
                if parsed_idx == override_idx {
                    override_prefix.clone()
                } else {
                    make_paragraph_id(parsed_para_id_start + parsed_idx)
                }
            } else {
                make_paragraph_id(parsed_para_id_start + parsed_idx)
            };

            if idx < 3 || is_cursor_para {
                tracing::trace!(
                    idx,
                    parsed_idx,
                    is_cursor_para,
                    para_id = %id,
                    "slow path: assigned paragraph ID"
                );
            }

            id
        };

        let (html, offset_map, syntax_spans, para_refs) = if is_reused {
            let reused = &reused_paragraphs[idx];
            (
                reused.html.clone(),
                reused.offset_map.clone(),
                reused.syntax_spans.clone(),
                reused.collected_refs.clone(),
            )
        } else {
            let parsed_idx = idx - reused_count;
            let html = writer_result
                .html_segments
                .get(parsed_idx)
                .cloned()
                .unwrap_or_default();

            let mut offset_map = writer_result
                .offset_maps_by_paragraph
                .get(parsed_idx)
                .cloned()
                .unwrap_or_default();
            for m in &mut offset_map {
                m.char_range.start += parse_start_char;
                m.char_range.end += parse_start_char;
                m.byte_range.start += parse_start_byte;
                m.byte_range.end += parse_start_byte;
            }

            let mut syntax_spans = writer_result
                .syntax_spans_by_paragraph
                .get(parsed_idx)
                .cloned()
                .unwrap_or_default();
            for s in &mut syntax_spans {
                s.adjust_positions(parse_start_char as isize);
            }

            let para_refs = writer_result
                .collected_refs_by_paragraph
                .get(parsed_idx)
                .cloned()
                .unwrap_or_default();
            (html, offset_map, syntax_spans, para_refs)
        };

        all_refs.extend(para_refs.clone());

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

    let new_cache = RenderCache {
        paragraphs: new_cached,
        next_node_id: 0,
        next_syn_id: 0,
        next_para_id,
    };

    IncrementalRenderResult {
        paragraphs,
        cache: new_cache,
        collected_refs: all_refs,
    }
}
