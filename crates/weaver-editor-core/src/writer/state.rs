//! State structures for EditorWriter, grouped by concern.

use std::collections::HashMap;
use std::ops::Range;

use markdown_weaver::Alignment;
use smol_str::{SmolStr, ToSmolStr, format_smolstr};

use crate::offset_map::OffsetMapping;
use crate::syntax::{SyntaxSpanInfo, SyntaxType};

/// Table rendering state.
#[derive(Debug, Clone, Default)]
pub struct TableContext {
    pub state: TableState,
    pub alignments: Vec<Alignment>,
    pub cell_index: usize,
    pub render_as_markdown: bool,
    pub start_offset: Option<usize>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TableState {
    #[default]
    Head,
    Body,
}

/// Code block buffering state.
#[derive(Debug, Clone, Default)]
pub struct CodeBlockContext {
    /// (language, content) being buffered
    pub buffer: Option<(Option<SmolStr>, String)>,
    /// Byte range of buffered content
    pub byte_range: Option<Range<usize>>,
    /// Char range of buffered content
    pub char_range: Option<Range<usize>>,
    /// Char offset where code block started
    pub block_start: Option<usize>,
    /// Index of opening fence syntax span
    pub opening_span_idx: Option<usize>,
}

impl CodeBlockContext {
    pub fn is_active(&self) -> bool {
        self.buffer.is_some()
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }
}

/// Node ID generation for DOM element IDs.
#[derive(Debug, Clone)]
pub struct NodeIdGenerator {
    /// Paragraph ID prefix (e.g., "p-0")
    pub prefix: Option<SmolStr>,
    /// Auto-increment base for paragraph prefixes
    pub auto_increment_base: Option<usize>,
    /// Override for specific paragraph index
    pub static_override: Option<(usize, SmolStr)>,
    /// Current paragraph index (0-indexed)
    pub current_paragraph: usize,
    /// Next node ID counter within paragraph
    pub next_node_id: usize,
    /// Next syntax span ID counter
    pub next_syn_id: usize,
}

impl Default for NodeIdGenerator {
    fn default() -> Self {
        Self {
            prefix: None,
            auto_increment_base: None,
            static_override: None,
            current_paragraph: 0,
            next_node_id: 0,
            next_syn_id: 0,
        }
    }
}

impl NodeIdGenerator {
    /// Get the current paragraph prefix.
    pub fn current_prefix(&self) -> SmolStr {
        if let Some((idx, ref prefix)) = self.static_override {
            if idx == self.current_paragraph {
                return prefix.clone();
            }
        }
        if let Some(base) = self.auto_increment_base {
            return format_smolstr!("p-{}", base + self.current_paragraph);
        }
        self.prefix.clone().unwrap_or_else(|| "p-0".to_smolstr())
    }

    /// Generate a node ID (e.g., "p-0-n3")
    pub fn next_node(&mut self) -> SmolStr {
        let id = if let Some(ref prefix) = self.prefix {
            format_smolstr!("{}-n{}", prefix, self.next_node_id)
        } else {
            format_smolstr!("n{}", self.next_node_id)
        };
        self.next_node_id += 1;
        SmolStr::new(id)
    }

    /// Generate a syntax span ID (e.g., "s5")
    pub fn next_syn(&mut self) -> SmolStr {
        let id = format_smolstr!("s{}", self.next_syn_id);
        self.next_syn_id += 1;
        SmolStr::new(id)
    }

    /// Advance to next paragraph.
    pub fn next_paragraph(&mut self) {
        self.current_paragraph += 1;
        self.next_node_id = 0;

        // Update prefix for next paragraph
        if let Some((override_idx, ref override_prefix)) = self.static_override {
            if self.current_paragraph == override_idx {
                self.prefix = Some(override_prefix.clone());
            } else if let Some(base) = self.auto_increment_base {
                self.prefix = Some(format_smolstr!("p-{}", base + self.current_paragraph));
            }
        } else if let Some(base) = self.auto_increment_base {
            self.prefix = Some(format_smolstr!("p-{}", base + self.current_paragraph));
        }
    }
}

/// Current DOM node tracking for offset mapping.
#[derive(Debug, Clone, Default)]
pub struct CurrentNodeState {
    /// Node ID for current text container
    pub id: Option<SmolStr>,
    /// UTF-16 offset within current node
    pub char_offset: usize,
    /// Number of child elements in current container
    pub child_count: usize,
}

impl CurrentNodeState {
    pub fn begin(&mut self, id: SmolStr) {
        self.id = Some(id);
        self.char_offset = 0;
        self.child_count = 0;
    }

    pub fn end(&mut self) {
        self.id = None;
        self.char_offset = 0;
        self.child_count = 0;
    }
}

/// Paragraph boundary tracking.
#[derive(Debug, Clone, Default)]
pub struct ParagraphTracker {
    /// Completed paragraph ranges: (byte_range, char_range)
    pub ranges: Vec<(Range<usize>, Range<usize>)>,
    /// Start of current paragraph: (byte_offset, char_offset)
    pub current_start: Option<(usize, usize)>,
    /// List nesting depth (suppress paragraph boundaries inside lists)
    pub list_depth: usize,
    /// In footnote definition (suppress inner paragraph boundaries)
    pub in_footnote_def: bool,
}

impl ParagraphTracker {
    pub fn start_paragraph(&mut self, byte_offset: usize, char_offset: usize) {
        self.current_start = Some((byte_offset, char_offset));
    }

    pub fn end_paragraph(
        &mut self,
        byte_offset: usize,
        char_offset: usize,
    ) -> Option<(Range<usize>, Range<usize>)> {
        if let Some((start_byte, start_char)) = self.current_start.take() {
            let ranges = (start_byte..byte_offset, start_char..char_offset);
            self.ranges.push(ranges.clone());
            Some(ranges)
        } else {
            None
        }
    }

    pub fn in_list(&self) -> bool {
        self.list_depth > 0
    }

    pub fn should_track_boundaries(&self) -> bool {
        self.list_depth == 0 && !self.in_footnote_def
    }
}

/// Current paragraph build state (offset maps, syntax spans, refs).
#[derive(Debug, Clone, Default)]
pub struct ParagraphBuildState {
    /// Offset mappings for current paragraph
    pub offset_maps: Vec<OffsetMapping>,
    /// Syntax spans for current paragraph
    pub syntax_spans: Vec<SyntaxSpanInfo>,
    /// Collected refs for current paragraph
    pub collected_refs: Vec<weaver_common::ExtractedRef>,
    /// Stack of pending inline formats: (syn_id, char_start)
    pub pending_inline_formats: Vec<(SmolStr, usize)>,
}

impl ParagraphBuildState {
    pub fn take_all(
        &mut self,
    ) -> (
        Vec<OffsetMapping>,
        Vec<SyntaxSpanInfo>,
        Vec<weaver_common::ExtractedRef>,
    ) {
        (
            std::mem::take(&mut self.offset_maps),
            std::mem::take(&mut self.syntax_spans),
            std::mem::take(&mut self.collected_refs),
        )
    }

    /// Finalize a paired inline format (Strong, Emphasis, Strikethrough).
    pub fn finalize_paired_format(&mut self, last_char_offset: usize) {
        if let Some((opening_syn_id, format_start)) = self.pending_inline_formats.pop() {
            let formatted_range = format_start..last_char_offset;

            // Update opening span
            if let Some(span) = self
                .syntax_spans
                .iter_mut()
                .find(|s| s.syn_id == opening_syn_id)
            {
                span.formatted_range = Some(formatted_range.clone());
            }

            // Update closing span (most recent)
            if let Some(closing) = self.syntax_spans.last_mut() {
                if closing.syntax_type == SyntaxType::Inline {
                    closing.formatted_range = Some(formatted_range);
                }
            }
        }
    }
}

/// WeaverBlock prefix system state.
#[derive(Debug, Clone, Default)]
pub struct WeaverBlockContext {
    /// Pending attrs to apply to next block element
    pub pending_attrs: Option<markdown_weaver::WeaverAttributes<'static>>,
    /// Type of wrapper element currently open
    pub active_wrapper: Option<WrapperElement>,
    /// Buffer for WeaverBlock text content
    pub buffer: String,
    /// Start char offset of current WeaverBlock
    pub char_start: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WrapperElement {
    Aside,
    Div,
}

/// Footnote reference/definition linking state.
#[derive(Debug, Clone, Default)]
pub struct FootnoteContext {
    /// Maps footnote name -> (syntax_span_index, char_start)
    pub ref_spans: HashMap<String, (usize, usize)>,
    /// Current footnote def being processed: (name, span_idx, char_start)
    pub current_def: Option<(String, usize, usize)>,
}

/// UTF-16 offset checkpoints for incremental tracking.
#[derive(Debug, Clone, Default)]
pub struct Utf16Tracker {
    /// Checkpoints: (char_offset, utf16_offset)
    pub checkpoints: Vec<(usize, usize)>,
}

impl Utf16Tracker {
    pub fn new() -> Self {
        Self {
            checkpoints: vec![(0, 0)],
        }
    }

    /// Add a checkpoint.
    pub fn checkpoint(&mut self, char_offset: usize, utf16_offset: usize) {
        if self.checkpoints.last().map(|(c, _)| *c) != Some(char_offset) {
            self.checkpoints.push((char_offset, utf16_offset));
        }
    }

    /// Get the last checkpoint.
    pub fn last(&self) -> (usize, usize) {
        self.checkpoints.last().copied().unwrap_or((0, 0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_node_id_generator() {
        let mut generator = NodeIdGenerator::default();
        generator.prefix = Some("p-0".to_smolstr());

        assert_eq!(generator.next_node().as_str(), "p-0-n0");
        assert_eq!(generator.next_node().as_str(), "p-0-n1");
        assert_eq!(generator.next_syn().as_str(), "s0");
        assert_eq!(generator.next_syn().as_str(), "s1");
    }

    #[test]
    fn test_node_id_generator_auto_increment() {
        let mut generator = NodeIdGenerator::default();
        generator.auto_increment_base = Some(0);
        generator.prefix = Some("p-0".to_smolstr());

        assert_eq!(generator.next_node().as_str(), "p-0-n0");
        generator.next_paragraph();
        assert_eq!(generator.prefix, Some("p-1".to_smolstr()));
        assert_eq!(generator.next_node().as_str(), "p-1-n0");
    }

    #[test]
    fn test_paragraph_tracker() {
        let mut tracker = ParagraphTracker::default();

        tracker.start_paragraph(0, 0);
        let ranges = tracker.end_paragraph(10, 10);
        assert_eq!(ranges, Some((0..10, 0..10)));

        tracker.list_depth = 1;
        assert!(tracker.in_list());
        assert!(!tracker.should_track_boundaries());
    }

    #[test]
    fn test_code_block_context() {
        let mut ctx = CodeBlockContext::default();
        assert!(!ctx.is_active());

        ctx.buffer = Some((Some("rust".to_smolstr()), String::new()));
        assert!(ctx.is_active());

        ctx.clear();
        assert!(!ctx.is_active());
    }
}
