//! Markdown rendering for the editor.
//!
//! Phase 2: Paragraph-level incremental rendering with formatting characters visible.
//!
//! This module provides a thin wrapper around the core rendering logic,
//! adding app-specific features like timing instrumentation.

use super::paragraph::ParagraphRender;
use super::writer::embed::EditorImageResolver;
use weaver_common::{EntryIndex, ResolvedContent};
use weaver_editor_core::{EditInfo, TextBuffer};

// Re-export core types.
pub use weaver_editor_core::RenderCache;

/// Render markdown with incremental caching.
///
/// Uses cached paragraph renders when possible, only re-rendering changed paragraphs.
/// This is a thin wrapper around the core rendering logic that adds timing.
///
/// # Parameters
/// - `text`: Any TextBuffer implementation (LoroTextBuffer, EditorRope, etc.)
/// - `cache`: Optional previous render cache
/// - `cursor_offset`: Current cursor position
/// - `edit`: Edit info for stable ID assignment
/// - `image_resolver`: Optional image URL resolver
/// - `entry_index`: Optional index for wikilink validation
/// - `resolved_content`: Pre-resolved embed content for sync rendering
///
/// # Returns
/// (paragraphs, cache, collected_refs) - collected_refs contains wikilinks and AT embeds found during render
pub fn render_paragraphs_incremental<T: TextBuffer>(
    text: &T,
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

    let result = weaver_editor_core::render_paragraphs_incremental(
        text,
        cache,
        cursor_offset,
        edit,
        image_resolver,
        entry_index,
        resolved_content,
    );

    let total_ms = crate::perf::now() - fn_start;
    tracing::debug!(
        total_ms,
        paragraphs = result.paragraphs.len(),
        "render_paragraphs_incremental timing"
    );

    (result.paragraphs, result.cache, result.collected_refs)
}
