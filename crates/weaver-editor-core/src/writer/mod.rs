//! EditorWriter - HTML generation for markdown with visible formatting.
//!
//! Refactored to use grouped state structs for clarity.
//! Generic over TextBuffer - works with ropey (local) or can be adapted for Loro (collab).

mod embed;
mod events;
mod state;
mod syntax;
mod tags;

pub use embed::EditorImageResolver;
pub use state::*;

use std::collections::HashMap;
use std::fmt::{self, Write as FmtWrite};
use std::ops::Range;

use markdown_weaver::Event;
use smol_str::SmolStr;

use crate::offset_map::OffsetMapping;
use crate::render::{EmbedContentProvider, ImageResolver, WikilinkValidator};
use crate::syntax::SyntaxSpanInfo;

/// Result of rendering with EditorWriter.
#[derive(Debug, Clone, Default)]
pub struct WriterResult {
    /// HTML segments, one per paragraph
    pub html_segments: Vec<String>,
    /// Offset mappings per paragraph
    pub offset_maps_by_paragraph: Vec<Vec<OffsetMapping>>,
    /// Paragraph boundaries: (byte_range, char_range)
    pub paragraph_ranges: Vec<(Range<usize>, Range<usize>)>,
    /// Syntax spans per paragraph
    pub syntax_spans_by_paragraph: Vec<Vec<SyntaxSpanInfo>>,
    /// Collected refs per paragraph
    pub collected_refs_by_paragraph: Vec<Vec<weaver_common::ExtractedRef>>,
}

/// Segmented HTML output writer.
#[derive(Debug, Clone, Default)]
pub struct SegmentedWriter {
    segments: Vec<String>,
    current: String,
}

impl SegmentedWriter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn write_str(&mut self, s: &str) -> fmt::Result {
        self.current.push_str(s);
        Ok(())
    }

    pub fn new_segment(&mut self) {
        if !self.current.is_empty() {
            self.segments.push(std::mem::take(&mut self.current));
        }
    }

    pub fn into_segments(mut self) -> Vec<String> {
        self.new_segment();
        self.segments
    }

    pub fn current_len(&self) -> usize {
        self.current.len()
    }
}

impl FmtWrite for SegmentedWriter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.current.push_str(s);
        Ok(())
    }
}

impl markdown_weaver_escape::StrWrite for SegmentedWriter {
    type Error = fmt::Error;

    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.current.push_str(s);
        Ok(())
    }

    fn write_fmt(&mut self, args: fmt::Arguments<'_>) -> fmt::Result {
        std::fmt::Write::write_fmt(&mut self.current, args)
    }
}

/// HTML writer that preserves markdown formatting characters.
///
/// Generic over:
/// - `T`: Text buffer for efficient offset conversions
/// - `I`: Iterator of markdown events with byte ranges
/// - `E`: Embed content provider (optional)
/// - `R`: Image resolver (optional)
/// - `W`: Wikilink validator (optional)
pub struct EditorWriter<'a, T, I, E = (), R = (), W = ()>
where
    T: crate::TextBuffer,
    I: Iterator<Item = (Event<'a>, Range<usize>)>,
{
    // === Input ===
    source: &'a str,
    text_buffer: &'a T,
    events: I,

    // === Output ===
    writer: SegmentedWriter,

    // === Position tracking ===
    last_byte_offset: usize,
    last_char_offset: usize,

    // === Rendering flags ===
    end_newline: bool,
    in_non_writing_block: bool,

    // === Grouped state ===
    pub(crate) table: TableContext,
    pub(crate) code_block: CodeBlockContext,
    pub(crate) node_ids: NodeIdGenerator,
    pub(crate) current_node: CurrentNodeState,
    pub(crate) paragraphs: ParagraphTracker,
    pub(crate) current_para: ParagraphBuildState,
    pub(crate) weaver_block: WeaverBlockContext,
    pub(crate) footnotes: FootnoteContext,
    pub(crate) utf16: Utf16Tracker,

    // === Per-paragraph results ===
    offset_maps_by_para: Vec<Vec<OffsetMapping>>,
    syntax_spans_by_para: Vec<Vec<SyntaxSpanInfo>>,
    refs_by_para: Vec<Vec<weaver_common::ExtractedRef>>,

    // === External resolvers ===
    embed_provider: Option<E>,
    image_resolver: Option<R>,
    wikilink_validator: Option<W>,
    entry_index: Option<&'a weaver_common::EntryIndex>,

    // === Misc ===
    numbers: HashMap<SmolStr, usize>,
    pending_blockquote_range: Option<Range<usize>>,
    ref_collector: weaver_common::RefCollector,
}

impl<'a, T, I, E, R, W> EditorWriter<'a, T, I, E, R, W>
where
    T: crate::TextBuffer,
    I: Iterator<Item = (Event<'a>, Range<usize>)>,
{
    /// Create a new EditorWriter.
    ///
    /// `source` is the markdown source text (should match text_buffer content).
    /// `text_buffer` provides efficient offset conversions.
    /// `events` is the markdown parser event iterator.
    pub fn new(source: &'a str, text_buffer: &'a T, events: I) -> Self {
        Self {
            source,
            text_buffer,
            events,
            writer: SegmentedWriter::new(),
            last_byte_offset: 0,
            last_char_offset: 0,
            end_newline: true,
            in_non_writing_block: false,
            table: TableContext::default(),
            code_block: CodeBlockContext::default(),
            node_ids: NodeIdGenerator::default(),
            current_node: CurrentNodeState::default(),
            paragraphs: ParagraphTracker::default(),
            current_para: ParagraphBuildState::default(),
            weaver_block: WeaverBlockContext::default(),
            footnotes: FootnoteContext::default(),
            utf16: Utf16Tracker::new(),
            offset_maps_by_para: Vec::new(),
            syntax_spans_by_para: Vec::new(),
            refs_by_para: Vec::new(),
            embed_provider: None,
            image_resolver: None,
            wikilink_validator: None,
            entry_index: None,
            numbers: HashMap::new(),
            pending_blockquote_range: None,
            ref_collector: weaver_common::RefCollector::new(),
        }
    }

    /// Set a static node ID prefix for all paragraphs.
    pub fn with_node_id_prefix(mut self, prefix: &str) -> Self {
        self.node_ids.prefix = Some(SmolStr::new(prefix));
        self.node_ids.next_node_id = 0;
        self
    }

    /// Use auto-incrementing paragraph prefixes starting from `base`.
    pub fn with_auto_incrementing_prefix(mut self, base: usize) -> Self {
        use smol_str::format_smolstr;
        self.node_ids.auto_increment_base = Some(base);
        self.node_ids.prefix = Some(format_smolstr!("p-{}", base));
        self.node_ids.next_node_id = 0;
        self
    }

    /// Override prefix for a specific paragraph index.
    pub fn with_static_prefix_at_index(mut self, index: usize, prefix: &str) -> Self {
        self.node_ids.static_override = Some((index, SmolStr::new(prefix)));
        if index == 0 {
            self.node_ids.prefix = Some(SmolStr::new(prefix));
            self.node_ids.next_node_id = 0;
        }
        self
    }

    /// Set initial offsets (for rendering a subset of the document).
    pub fn with_offsets(
        mut self,
        byte_offset: usize,
        char_offset: usize,
        node_id_offset: usize,
        syn_id_offset: usize,
    ) -> Self {
        self.last_byte_offset = byte_offset;
        self.last_char_offset = char_offset;
        self.node_ids.next_node_id = node_id_offset;
        self.node_ids.next_syn_id = syn_id_offset;
        self
    }

    /// Set embed content provider.
    pub fn with_embed_provider<E2: EmbedContentProvider>(
        self,
        provider: E2,
    ) -> EditorWriter<'a, T, I, E2, R, W> {
        EditorWriter {
            source: self.source,
            text_buffer: self.text_buffer,
            events: self.events,
            writer: self.writer,
            last_byte_offset: self.last_byte_offset,
            last_char_offset: self.last_char_offset,
            end_newline: self.end_newline,
            in_non_writing_block: self.in_non_writing_block,
            table: self.table,
            code_block: self.code_block,
            node_ids: self.node_ids,
            current_node: self.current_node,
            paragraphs: self.paragraphs,
            current_para: self.current_para,
            weaver_block: self.weaver_block,
            footnotes: self.footnotes,
            utf16: self.utf16,
            offset_maps_by_para: self.offset_maps_by_para,
            syntax_spans_by_para: self.syntax_spans_by_para,
            refs_by_para: self.refs_by_para,
            embed_provider: Some(provider),
            image_resolver: self.image_resolver,
            wikilink_validator: self.wikilink_validator,
            entry_index: self.entry_index,
            numbers: self.numbers,
            pending_blockquote_range: self.pending_blockquote_range,
            ref_collector: self.ref_collector,
        }
    }

    /// Set image resolver.
    pub fn with_image_resolver<R2: ImageResolver>(
        self,
        resolver: R2,
    ) -> EditorWriter<'a, T, I, E, R2, W> {
        EditorWriter {
            source: self.source,
            text_buffer: self.text_buffer,
            events: self.events,
            writer: self.writer,
            last_byte_offset: self.last_byte_offset,
            last_char_offset: self.last_char_offset,
            end_newline: self.end_newline,
            in_non_writing_block: self.in_non_writing_block,
            table: self.table,
            code_block: self.code_block,
            node_ids: self.node_ids,
            current_node: self.current_node,
            paragraphs: self.paragraphs,
            current_para: self.current_para,
            weaver_block: self.weaver_block,
            footnotes: self.footnotes,
            utf16: self.utf16,
            offset_maps_by_para: self.offset_maps_by_para,
            syntax_spans_by_para: self.syntax_spans_by_para,
            refs_by_para: self.refs_by_para,
            embed_provider: self.embed_provider,
            image_resolver: Some(resolver),
            wikilink_validator: self.wikilink_validator,
            entry_index: self.entry_index,
            numbers: self.numbers,
            pending_blockquote_range: self.pending_blockquote_range,
            ref_collector: self.ref_collector,
        }
    }

    /// Set wikilink validator.
    pub fn with_wikilink_validator<W2: WikilinkValidator>(
        self,
        validator: W2,
    ) -> EditorWriter<'a, T, I, E, R, W2> {
        EditorWriter {
            source: self.source,
            text_buffer: self.text_buffer,
            events: self.events,
            writer: self.writer,
            last_byte_offset: self.last_byte_offset,
            last_char_offset: self.last_char_offset,
            end_newline: self.end_newline,
            in_non_writing_block: self.in_non_writing_block,
            table: self.table,
            code_block: self.code_block,
            node_ids: self.node_ids,
            current_node: self.current_node,
            paragraphs: self.paragraphs,
            current_para: self.current_para,
            weaver_block: self.weaver_block,
            footnotes: self.footnotes,
            utf16: self.utf16,
            offset_maps_by_para: self.offset_maps_by_para,
            syntax_spans_by_para: self.syntax_spans_by_para,
            refs_by_para: self.refs_by_para,
            embed_provider: self.embed_provider,
            image_resolver: self.image_resolver,
            wikilink_validator: Some(validator),
            entry_index: self.entry_index,
            numbers: self.numbers,
            pending_blockquote_range: self.pending_blockquote_range,
            ref_collector: self.ref_collector,
        }
    }

    /// Set entry index for wikilink resolution.
    pub fn with_entry_index(mut self, index: &'a weaver_common::EntryIndex) -> Self {
        self.entry_index = Some(index);
        self
    }
}

// Core helper methods
impl<'a, T, I, E, R, W> EditorWriter<'a, T, I, E, R, W>
where
    T: crate::TextBuffer,
    I: Iterator<Item = (Event<'a>, Range<usize>)>,
{
    /// Write a string to the output.
    #[inline]
    pub(crate) fn write(&mut self, s: &str) -> fmt::Result {
        if !s.is_empty() {
            self.end_newline = s.ends_with('\n');
        }
        self.writer.write_str(s)
    }

    /// Write a newline.
    #[inline]
    pub(crate) fn write_newline(&mut self) -> fmt::Result {
        self.end_newline = true;
        self.writer.write_str("\n")
    }

    /// Generate a unique node ID.
    pub(crate) fn gen_node_id(&mut self) -> SmolStr {
        self.node_ids.next_node()
    }

    /// Generate a unique syntax span ID.
    pub(crate) fn gen_syn_id(&mut self) -> SmolStr {
        self.node_ids.next_syn()
    }

    /// Start tracking a new text container node.
    pub(crate) fn begin_node(&mut self, node_id: SmolStr) {
        self.current_node.begin(node_id);
    }

    /// Stop tracking current node.
    pub(crate) fn end_node(&mut self) {
        self.current_node.end();
    }

    /// Compute UTF-16 length for a text slice (fast path for ASCII).
    #[inline]
    pub(crate) fn utf16_len_for_slice(text: &str) -> usize {
        let byte_len = text.len();
        let char_len = text.chars().count();

        if byte_len == char_len {
            char_len
        } else {
            text.encode_utf16().count()
        }
    }

    /// Record an offset mapping.
    pub(crate) fn record_mapping(&mut self, byte_range: Range<usize>, char_range: Range<usize>) {
        if let Some(ref node_id) = self.current_node.id {
            let text_slice = &self.source[byte_range.clone()];
            let utf16_len = Self::utf16_len_for_slice(text_slice);

            // Record UTF-16 checkpoint
            let last = self.utf16.last();
            let new_utf16 = last.1 + utf16_len;
            if char_range.end > last.0 {
                self.utf16.checkpoint(char_range.end, new_utf16);
            }

            let mapping = OffsetMapping {
                byte_range,
                char_range: char_range.clone(),
                node_id: node_id.clone(),
                char_offset_in_node: self.current_node.char_offset,
                child_index: None,
                utf16_len,
            };
            self.current_para.offset_maps.push(mapping);
            self.current_node.char_offset += utf16_len;
        }
    }

    /// Finalize the current paragraph.
    pub(crate) fn finalize_paragraph(
        &mut self,
        byte_range: Range<usize>,
        char_range: Range<usize>,
    ) {
        self.paragraphs.ranges.push((byte_range, char_range));

        let (maps, spans, refs) = self.current_para.take_all();
        self.offset_maps_by_para.push(maps);
        self.syntax_spans_by_para.push(spans);
        self.refs_by_para.push(refs);

        self.node_ids.next_paragraph();
        self.writer.new_segment();
    }

    /// Consume events until End tag without writing.
    pub(crate) fn consume_until_end(&mut self) {
        let mut nest = 0;
        while let Some((event, _)) = self.events.next() {
            match event {
                Event::Start(_) => nest += 1,
                Event::End(_) => {
                    if nest == 0 {
                        break;
                    }
                    nest -= 1;
                }
                _ => {}
            }
        }
    }
}
