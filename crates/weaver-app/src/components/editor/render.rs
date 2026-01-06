//! Markdown rendering for the editor.
//!
//! Phase 2: Paragraph-level incremental rendering with formatting characters visible.
//!
//! This module provides a thin wrapper around the core rendering logic,
//! adapting it to LoroText and adding app-specific features like timing.

use super::paragraph::ParagraphRender;
use super::writer::embed::EditorImageResolver;
use loro::LoroText;
use weaver_common::{EntryIndex, ResolvedContent};
use weaver_editor_core::{EditInfo, SmolStr, TextBuffer};

// Re-export core types.
pub use weaver_editor_core::RenderCache;

/// Adapter to make LoroText implement TextBuffer-like interface for core.
/// temporary until weaver-editor-crdt crate complete
struct LoroTextAdapter<'a>(&'a LoroText);

impl TextBuffer for LoroTextAdapter<'_> {
    fn len_chars(&self) -> usize {
        self.0.len_unicode()
    }

    fn len_bytes(&self) -> usize {
        self.0.len_utf8()
    }

    fn slice(&self, range: std::ops::Range<usize>) -> Option<SmolStr> {
        self.0
            .slice(range.start, range.end)
            .ok()
            .map(|s| SmolStr::new(&s))
    }

    fn char_at(&self, offset: usize) -> Option<char> {
        self.0
            .slice(offset, offset + 1)
            .ok()
            .and_then(|s| s.chars().next())
    }

    fn char_to_byte(&self, char_offset: usize) -> usize {
        // LoroText doesn't expose this directly, so we compute it.
        let slice = self.0.slice(0, char_offset).unwrap_or_default();
        slice.len()
    }

    fn byte_to_char(&self, byte_offset: usize) -> usize {
        // LoroText doesn't expose this directly, so we compute it.
        let full = self.0.to_string();
        full[..byte_offset.min(full.len())].chars().count()
    }

    fn insert(&mut self, _offset: usize, _text: &str) {
        // Read-only adapter - this should never be called during rendering.
        panic!("LoroTextAdapter is read-only");
    }

    fn delete(&mut self, _range: std::ops::Range<usize>) {
        // Read-only adapter - this should never be called during rendering.
        panic!("LoroTextAdapter is read-only");
    }

    fn to_string(&self) -> String {
        self.0.to_string()
    }
}

/// Render markdown with incremental caching.
///
/// Uses cached paragraph renders when possible, only re-rendering changed paragraphs.
/// This is a thin wrapper around the core rendering logic that adds timing.
///
/// # Parameters
/// - `text`: The LoroText to render
/// - `cache`: Optional previous render cache
/// - `cursor_offset`: Current cursor position
/// - `edit`: Edit info for stable ID assignment
/// - `image_resolver`: Optional image URL resolver
/// - `entry_index`: Optional index for wikilink validation
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

    // Create adapter for LoroText to use with core rendering.
    let adapter = LoroTextAdapter(text);

    // Call the core rendering function.
    let result = weaver_editor_core::render_paragraphs_incremental(
        &adapter,
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
