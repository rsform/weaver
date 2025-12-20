//! HTML writer for markdown editor with visible formatting characters.
//!
//! Based on ClientWriter from weaver-renderer, but modified to preserve
//! formatting characters (**, *, #, etc) wrapped in styled spans.
//!
//! Uses Parser::into_offset_iter() to track gaps between events, which
//! represent consumed formatting characters.
pub mod embed;
pub mod segmented;
pub mod syntax;
pub mod tags;

use crate::components::editor::writer::segmented::SegmentedWriter;
pub use embed::{EditorImageResolver, EmbedContentProvider, ImageResolver};
pub use syntax::{SyntaxSpanInfo, SyntaxType};

#[allow(unused_imports)]
use super::offset_map::{OffsetMapping, RenderResult};
use loro::LoroText;
use markdown_weaver::{Alignment, CowStr, Event, WeaverAttributes};
use markdown_weaver_escape::{StrWrite, escape_html, escape_html_body_text_with_char_count};
use std::collections::HashMap;
use std::fmt;
use std::ops::Range;
use weaver_common::EntryIndex;

/// Result of rendering with the EditorWriter.
#[derive(Debug, Clone)]
pub struct WriterResult {
    /// HTML segments, one per paragraph (parallel to paragraph_ranges)
    pub html_segments: Vec<String>,

    /// Offset mappings from source to DOM positions, grouped by paragraph
    /// Each inner Vec corresponds to a paragraph in html_segments
    pub offset_maps_by_paragraph: Vec<Vec<OffsetMapping>>,

    /// Paragraph boundaries in source: (byte_range, char_range)
    /// These are extracted during rendering by tracking Tag::Paragraph events
    pub paragraph_ranges: Vec<(Range<usize>, Range<usize>)>,

    /// Syntax spans that can be conditionally hidden, grouped by paragraph
    pub syntax_spans_by_paragraph: Vec<Vec<SyntaxSpanInfo>>,

    /// Refs (wikilinks, AT embeds) collected during this render pass, grouped by paragraph
    pub collected_refs_by_paragraph: Vec<Vec<weaver_common::ExtractedRef>>,
}

/// Tracks the type of wrapper element emitted for WeaverBlock prefix
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WrapperElement {
    Aside,
    Div,
}

#[derive(Debug, Clone, Copy)]
pub enum TableState {
    Head,
    Body,
}

/// HTML writer that preserves markdown formatting characters.
///
/// This writer processes offset-iter events to detect gaps (consumed formatting)
/// and emits them as styled spans for visibility in the editor.
///
/// Output is segmented by paragraph boundaries - each paragraph's HTML goes into
/// a separate String in the output segments Vec.
pub struct EditorWriter<'a, I: Iterator<Item = (Event<'a>, Range<usize>)>, E = (), R = ()> {
    source: &'a str,
    source_text: &'a LoroText,
    events: I,
    writer: SegmentedWriter,
    last_byte_offset: usize,
    last_char_offset: usize,

    end_newline: bool,
    in_non_writing_block: bool,

    table_state: TableState,
    table_alignments: Vec<Alignment>,
    table_cell_index: usize,

    numbers: HashMap<String, usize>,

    embed_provider: Option<E>,
    image_resolver: Option<R>,
    entry_index: Option<&'a EntryIndex>,

    code_buffer: Option<(Option<String>, String)>, // (lang, content)
    code_buffer_byte_range: Option<Range<usize>>,  // byte range of buffered code content
    code_buffer_char_range: Option<Range<usize>>,  // char range of buffered code content
    code_block_char_start: Option<usize>,          // char offset where code block started
    code_block_opening_span_idx: Option<usize>,    // index of opening fence syntax span
    pending_blockquote_range: Option<Range<usize>>, // range for emitting > inside next paragraph

    // Table rendering mode
    render_tables_as_markdown: bool,
    table_start_offset: Option<usize>, // track start of table for markdown rendering

    // Offset mapping tracking - current paragraph
    offset_maps: Vec<OffsetMapping>,
    node_id_prefix: Option<String>, // paragraph ID prefix for stable node IDs
    auto_increment_prefix: Option<usize>, // if set, auto-increment prefix per paragraph from this value
    static_prefix_override: Option<(usize, String)>, // (index, prefix) - override auto-increment at this index
    current_paragraph_index: usize, // which paragraph we're currently building (0-indexed)
    next_node_id: usize,
    current_node_id: Option<String>, // node ID for current text container
    current_node_char_offset: usize, // UTF-16 offset within current node
    current_node_child_count: usize, // number of child elements/text nodes in current container

    // Incremental UTF-16 offset tracking (replaces rope.chars_to_wchars)
    // Maps char_offset -> utf16_offset at checkpoints we've traversed.
    // Can be reused for future lookups or passed to subsequent writers.
    utf16_checkpoints: Vec<(usize, usize)>, // (char_offset, utf16_offset)

    // Paragraph boundary tracking for incremental rendering
    paragraph_ranges: Vec<(Range<usize>, Range<usize>)>, // (byte_range, char_range)
    current_paragraph_start: Option<(usize, usize)>,     // (byte_offset, char_offset)
    list_depth: usize, // Track nesting depth to avoid paragraph boundary override inside lists

    // Syntax span tracking for conditional visibility - current paragraph
    syntax_spans: Vec<SyntaxSpanInfo>,
    next_syn_id: usize,
    /// Stack of pending inline formats: (syn_id of opening span, char start of region)
    /// Used to set formatted_range when closing paired inline markers
    pending_inline_formats: Vec<(String, usize)>,

    /// Collected refs (wikilinks, AT embeds, AT links) for current paragraph
    ref_collector: weaver_common::RefCollector,

    // Per-paragraph accumulated results (completed paragraphs)
    offset_maps_by_para: Vec<Vec<OffsetMapping>>,
    syntax_spans_by_para: Vec<Vec<SyntaxSpanInfo>>,
    refs_by_para: Vec<Vec<weaver_common::ExtractedRef>>,

    // WeaverBlock prefix system
    /// Pending WeaverBlock attrs to apply to the next block element
    pending_block_attrs: Option<WeaverAttributes<'static>>,
    /// Type of wrapper element currently open (needs closing on block end)
    active_wrapper: Option<WrapperElement>,
    /// Buffer for WeaverBlock text content (to parse for attrs)
    weaver_block_buffer: String,
    /// Start char offset of current WeaverBlock (for syntax span)
    weaver_block_char_start: Option<usize>,

    // Footnote syntax linking (ref ↔ definition visibility)
    /// Maps footnote name → (syntax_span_index, char_start) for linking ref and def
    footnote_ref_spans: HashMap<String, (usize, usize)>,
    /// Current footnote definition being processed (name, syntax_span_index, char_start)
    current_footnote_def: Option<(String, usize, usize)>,

    _phantom: std::marker::PhantomData<&'a ()>,
}

impl<'a, I: Iterator<Item = (Event<'a>, Range<usize>)>, E: EmbedContentProvider, R: ImageResolver>
    EditorWriter<'a, I, E, R>
{
    pub fn new(source: &'a str, source_text: &'a LoroText, events: I) -> Self {
        Self::new_with_all_offsets(source, source_text, events, 0, 0, 0, 0)
    }

    pub fn new_with_all_offsets(
        source: &'a str,
        source_text: &'a LoroText,
        events: I,
        node_id_offset: usize,
        syn_id_offset: usize,
        char_offset_base: usize,
        byte_offset_base: usize,
    ) -> Self {
        Self {
            source,
            source_text,
            events,
            writer: SegmentedWriter::new(),
            last_byte_offset: byte_offset_base,
            last_char_offset: char_offset_base,
            end_newline: true,
            in_non_writing_block: false,
            table_state: TableState::Head,
            table_alignments: vec![],
            table_cell_index: 0,
            numbers: HashMap::new(),
            embed_provider: None,
            image_resolver: None,
            entry_index: None,
            code_buffer: None,
            code_buffer_byte_range: None,
            code_buffer_char_range: None,
            code_block_char_start: None,
            code_block_opening_span_idx: None,
            pending_blockquote_range: None,
            render_tables_as_markdown: true,
            table_start_offset: None,
            offset_maps: Vec::new(),
            node_id_prefix: None,
            auto_increment_prefix: None,
            static_prefix_override: None,
            current_paragraph_index: 0,
            next_node_id: node_id_offset,
            current_node_id: None,
            current_node_char_offset: 0,
            current_node_child_count: 0,
            utf16_checkpoints: vec![(0, 0)],
            paragraph_ranges: Vec::new(),
            current_paragraph_start: None,
            list_depth: 0,
            syntax_spans: Vec::new(),
            next_syn_id: syn_id_offset,
            pending_inline_formats: Vec::new(),
            ref_collector: weaver_common::RefCollector::new(),
            offset_maps_by_para: Vec::new(),
            syntax_spans_by_para: Vec::new(),
            refs_by_para: Vec::new(),
            pending_block_attrs: None,
            active_wrapper: None,
            weaver_block_buffer: String::new(),
            weaver_block_char_start: None,
            footnote_ref_spans: HashMap::new(),
            current_footnote_def: None,
            _phantom: std::marker::PhantomData,
        }
    }

    /// Add an embed content provider
    pub fn with_embed_provider(mut self, provider: E) -> EditorWriter<'a, I, E, R> {
        self.embed_provider = Some(provider);
        self
    }

    /// Add an image resolver for mapping markdown image URLs to CDN URLs
    pub fn with_image_resolver<R2: ImageResolver>(
        self,
        resolver: R2,
    ) -> EditorWriter<'a, I, E, R2> {
        EditorWriter {
            source: self.source,
            source_text: self.source_text,
            events: self.events,
            writer: self.writer,
            last_byte_offset: self.last_byte_offset,
            last_char_offset: self.last_char_offset,
            end_newline: self.end_newline,
            in_non_writing_block: self.in_non_writing_block,
            table_state: self.table_state,
            table_alignments: self.table_alignments,
            table_cell_index: self.table_cell_index,
            numbers: self.numbers,
            embed_provider: self.embed_provider,
            image_resolver: Some(resolver),
            entry_index: self.entry_index,
            code_buffer: self.code_buffer,
            code_buffer_byte_range: self.code_buffer_byte_range,
            code_buffer_char_range: self.code_buffer_char_range,
            code_block_char_start: self.code_block_char_start,
            code_block_opening_span_idx: self.code_block_opening_span_idx,
            pending_blockquote_range: self.pending_blockquote_range,
            render_tables_as_markdown: self.render_tables_as_markdown,
            table_start_offset: self.table_start_offset,
            offset_maps: self.offset_maps,
            node_id_prefix: self.node_id_prefix,
            auto_increment_prefix: self.auto_increment_prefix,
            static_prefix_override: self.static_prefix_override,
            current_paragraph_index: self.current_paragraph_index,
            next_node_id: self.next_node_id,
            current_node_id: self.current_node_id,
            current_node_char_offset: self.current_node_char_offset,
            current_node_child_count: self.current_node_child_count,
            utf16_checkpoints: self.utf16_checkpoints,
            paragraph_ranges: self.paragraph_ranges,
            current_paragraph_start: self.current_paragraph_start,
            list_depth: self.list_depth,
            syntax_spans: self.syntax_spans,
            next_syn_id: self.next_syn_id,
            pending_inline_formats: self.pending_inline_formats,
            ref_collector: self.ref_collector,
            offset_maps_by_para: self.offset_maps_by_para,
            syntax_spans_by_para: self.syntax_spans_by_para,
            refs_by_para: self.refs_by_para,
            pending_block_attrs: self.pending_block_attrs,
            active_wrapper: self.active_wrapper,
            weaver_block_buffer: self.weaver_block_buffer,
            weaver_block_char_start: self.weaver_block_char_start,
            footnote_ref_spans: self.footnote_ref_spans,
            current_footnote_def: self.current_footnote_def,
            _phantom: std::marker::PhantomData,
        }
    }

    /// Add an entry index for wikilink resolution feedback
    pub fn with_entry_index(mut self, index: &'a EntryIndex) -> Self {
        self.entry_index = Some(index);
        self
    }

    /// Set a prefix for node IDs (typically the paragraph ID).
    /// This makes node IDs paragraph-scoped and stable across re-renders.
    /// Use this for single-paragraph renders where the paragraph ID is known.
    pub fn with_node_id_prefix(mut self, prefix: &str) -> Self {
        self.node_id_prefix = Some(prefix.to_string());
        self.next_node_id = 0; // Reset counter since each paragraph is independent
        self
    }

    /// Enable auto-incrementing paragraph prefixes for multi-paragraph renders.
    /// Each paragraph gets prefix "p-{N}" where N starts at `start_id` and increments.
    /// Node IDs reset to 0 for each paragraph, giving "p-{N}-n0", "p-{N}-n1", etc.
    pub fn with_auto_incrementing_prefix(mut self, start_id: usize) -> Self {
        self.auto_increment_prefix = Some(start_id);
        self.node_id_prefix = Some(format!("p-{}", start_id));
        self.next_node_id = 0;
        self
    }

    /// Get the next paragraph ID that would be assigned (for tracking allocations).
    #[allow(dead_code)]
    pub fn next_paragraph_id(&self) -> Option<usize> {
        self.auto_increment_prefix
    }

    /// Override the auto-incrementing prefix for a specific paragraph index.
    /// Use this when you need a specific paragraph (e.g., cursor paragraph) to have
    /// a stable prefix for DOM/offset_map compatibility.
    pub fn with_static_prefix_at_index(mut self, index: usize, prefix: &str) -> Self {
        self.static_prefix_override = Some((index, prefix.to_string()));
        // If this is for paragraph 0, apply it immediately
        if index == 0 {
            self.node_id_prefix = Some(prefix.to_string());
            self.next_node_id = 0;
        }
        self
    }

    /// Finalize the current paragraph: move accumulated items to per-para vectors,
    /// start a new output segment for the next paragraph.
    fn finalize_paragraph(&mut self, byte_range: Range<usize>, char_range: Range<usize>) {
        // Record paragraph boundary
        self.paragraph_ranges.push((byte_range, char_range));

        // Move current paragraph's data to per-para vectors
        self.offset_maps_by_para
            .push(std::mem::take(&mut self.offset_maps));
        self.syntax_spans_by_para
            .push(std::mem::take(&mut self.syntax_spans));
        self.refs_by_para
            .push(std::mem::take(&mut self.ref_collector.refs));

        // Advance to next paragraph
        self.current_paragraph_index += 1;

        // Determine prefix for next paragraph
        if let Some((override_idx, ref override_prefix)) = self.static_prefix_override {
            if self.current_paragraph_index == override_idx {
                // Use the static override for this paragraph
                self.node_id_prefix = Some(override_prefix.clone());
                self.next_node_id = 0;
            } else if let Some(ref mut current_id) = self.auto_increment_prefix {
                // Use auto-increment (skip the override index to avoid collision)
                *current_id += 1;
                self.node_id_prefix = Some(format!("p-{}", *current_id));
                self.next_node_id = 0;
            }
        } else if let Some(ref mut current_id) = self.auto_increment_prefix {
            // Normal auto-increment
            *current_id += 1;
            self.node_id_prefix = Some(format!("p-{}", *current_id));
            self.next_node_id = 0;
        }

        // Start new output segment for next paragraph
        self.writer.new_segment();
    }

    #[inline]
    fn write_newline(&mut self) -> fmt::Result {
        self.end_newline = true;
        self.writer.write_str("\n")
    }

    #[inline]
    fn write(&mut self, s: &str) -> fmt::Result {
        if !s.is_empty() {
            self.end_newline = s.ends_with('\n');
        }
        self.writer.write_str(s)
    }

    /// Generate a unique syntax span ID
    fn gen_syn_id(&mut self) -> String {
        let id = format!("s{}", self.next_syn_id);
        self.next_syn_id += 1;
        id
    }

    /// Finalize a paired inline format (Strong, Emphasis, Strikethrough).
    /// Pops the pending format info and sets formatted_range on both opening and closing spans.
    fn finalize_paired_inline_format(&mut self) {
        if let Some((opening_syn_id, format_start)) = self.pending_inline_formats.pop() {
            let format_end = self.last_char_offset;
            let formatted_range = format_start..format_end;

            // Update the opening span's formatted_range
            if let Some(opening_span) = self
                .syntax_spans
                .iter_mut()
                .find(|s| s.syn_id == opening_syn_id)
            {
                opening_span.formatted_range = Some(formatted_range.clone());
            } else {
                tracing::warn!(
                    "[FINALIZE_PAIRED] Could not find opening span {}",
                    opening_syn_id
                );
            }

            // Update the closing span's formatted_range (the most recent one)
            // The closing syntax was just emitted by emit_gap_before, so it's the last span
            if let Some(closing_span) = self.syntax_spans.last_mut() {
                // Only update if it's an inline span (closing syntax should be inline)
                if closing_span.syntax_type == SyntaxType::Inline {
                    closing_span.formatted_range = Some(formatted_range);
                }
            }
        }
    }

    /// Generate a unique node ID.
    /// If a prefix is set (paragraph ID), produces `{prefix}-n{counter}`.
    /// Otherwise produces `n{counter}` for backwards compatibility.
    fn gen_node_id(&mut self) -> String {
        let id = if let Some(ref prefix) = self.node_id_prefix {
            format!("{}-n{}", prefix, self.next_node_id)
        } else {
            format!("n{}", self.next_node_id)
        };
        self.next_node_id += 1;
        id
    }

    /// Start tracking a new text container node
    fn begin_node(&mut self, node_id: String) {
        self.current_node_id = Some(node_id);
        self.current_node_char_offset = 0;
        self.current_node_child_count = 0;
    }

    /// Stop tracking current node
    fn end_node(&mut self) {
        self.current_node_id = None;
        self.current_node_char_offset = 0;
        self.current_node_child_count = 0;
    }

    /// Compute UTF-16 length for a text slice with fast path for ASCII.
    #[inline]
    fn utf16_len_for_slice(text: &str) -> usize {
        let byte_len = text.len();
        let char_len = text.chars().count();

        // Fast path: if byte_len == char_len, all ASCII, so utf16_len == char_len
        if byte_len == char_len {
            char_len
        } else {
            // Slow path: has multi-byte chars, need to count UTF-16 code units
            text.encode_utf16().count()
        }
    }

    /// Record an offset mapping for the given byte and char ranges.
    ///
    /// Builds up utf16_checkpoints incrementally for efficient lookups.
    fn record_mapping(&mut self, byte_range: Range<usize>, char_range: Range<usize>) {
        if let Some(ref node_id) = self.current_node_id {
            // Get UTF-16 length using fast path
            let text_slice = &self.source[byte_range.clone()];
            let utf16_len = Self::utf16_len_for_slice(text_slice);

            // Record checkpoint at end of this range for future lookups
            let last_checkpoint = self.utf16_checkpoints.last().copied().unwrap_or((0, 0));
            let new_utf16_offset = last_checkpoint.1 + utf16_len;

            // Only add checkpoint if we've advanced
            if char_range.end > last_checkpoint.0 {
                self.utf16_checkpoints
                    .push((char_range.end, new_utf16_offset));
            }

            let mapping = OffsetMapping {
                byte_range: byte_range.clone(),
                char_range: char_range.clone(),
                node_id: node_id.clone(),
                char_offset_in_node: self.current_node_char_offset,
                child_index: None, // text-based position
                utf16_len,
            };
            self.offset_maps.push(mapping);
            self.current_node_char_offset += utf16_len;
        } else {
            tracing::debug!("[RECORD_MAPPING] SKIPPED - current_node_id is None!");
        }
    }

    /// Process markdown events and write HTML.
    ///
    /// Returns offset mappings and paragraph boundaries. The HTML is written
    /// to the writer passed in the constructor.
    pub fn run(mut self) -> Result<WriterResult, fmt::Error> {
        while let Some((event, range)) = self.events.next() {
            tracing::trace!(
                target: "weaver::writer",
                event = ?event,
                byte_range = ?range,
                "processing event"
            );

            // For End events, emit any trailing content within the event's range
            // BEFORE calling end_tag (which calls end_node and clears current_node_id)
            //
            // EXCEPTION: For inline formatting tags (Strong, Emphasis, Strikethrough),
            // the closing syntax must be emitted AFTER the closing HTML tag, not before.
            // Otherwise the closing `**` span ends up INSIDE the <strong> element.
            // These tags handle their own closing syntax in end_tag().
            // Image and Embed handle ALL their syntax in the Start event, so exclude them too.
            use markdown_weaver::TagEnd;
            let is_self_handled_end = matches!(
                &event,
                Event::End(
                    TagEnd::Strong
                        | TagEnd::Emphasis
                        | TagEnd::Strikethrough
                        | TagEnd::Image
                        | TagEnd::Embed
                )
            );

            if matches!(&event, Event::End(_)) && !is_self_handled_end {
                // Emit gap from last_byte_offset to range.end
                self.emit_gap_before(range.end)?;
            } else if !matches!(&event, Event::End(_)) {
                // For other events, emit any gap before range.start
                // (emit_syntax handles char offset tracking)
                self.emit_gap_before(range.start)?;
            }
            // For inline format End events, gap is emitted inside end_tag() AFTER the closing HTML

            // Store last_byte before processing
            let last_byte_before = self.last_byte_offset;

            // Process the event (passing range for tag syntax)
            self.process_event(event, range.clone())?;

            // Update tracking - but don't override if start_tag manually updated it
            // (for inline formatting tags that emit opening syntax)
            if self.last_byte_offset == last_byte_before {
                // Event didn't update offset, so we update it
                self.last_byte_offset = range.end;
            }
            // else: Event updated offset (e.g. start_tag emitted opening syntax), keep that value
        }

        // Emit any trailing syntax
        self.emit_gap_before(self.source.len())?;

        // Handle unmapped trailing content (stripped by parser)
        // This includes trailing spaces that markdown ignores
        let doc_byte_len = self.source.len();
        let doc_char_len = self.source_text.len_unicode();

        if self.last_byte_offset < doc_byte_len || self.last_char_offset < doc_char_len {
            // Emit the trailing content as visible syntax
            if self.last_byte_offset < doc_byte_len {
                let trailing = &self.source[self.last_byte_offset..];
                if !trailing.is_empty() {
                    let char_start = self.last_char_offset;
                    let trailing_char_len = trailing.chars().count();

                    let char_end = char_start + trailing_char_len;
                    let syn_id = self.gen_syn_id();

                    write!(
                        &mut self.writer,
                        "<span class=\"md-placeholder\" data-syn-id=\"{}\" data-char-start=\"{}\" data-char-end=\"{}\">",
                        syn_id, char_start, char_end
                    )?;
                    escape_html(&mut self.writer, trailing)?;
                    self.write("</span>")?;

                    // Record syntax span info
                    // self.syntax_spans.push(SyntaxSpanInfo {
                    //     syn_id,
                    //     char_range: char_start..char_end,
                    //     syntax_type: SyntaxType::Inline,
                    //     formatted_range: None,
                    // });

                    // Record mapping if we have a node
                    if let Some(ref node_id) = self.current_node_id {
                        let mapping = OffsetMapping {
                            byte_range: self.last_byte_offset..doc_byte_len,
                            char_range: char_start..char_end,
                            node_id: node_id.clone(),
                            char_offset_in_node: self.current_node_char_offset,
                            child_index: None,
                            utf16_len: trailing_char_len, // visible
                        };
                        self.offset_maps.push(mapping);
                        self.current_node_char_offset += trailing_char_len;
                    }

                    self.last_char_offset = char_start + trailing_char_len;
                }
            }
        }

        // Add any remaining accumulated data for the last paragraph
        // (content that wasn't followed by a paragraph boundary)
        if !self.offset_maps.is_empty()
            || !self.syntax_spans.is_empty()
            || !self.ref_collector.refs.is_empty()
        {
            self.offset_maps_by_para.push(self.offset_maps);
            self.syntax_spans_by_para.push(self.syntax_spans);
            self.refs_by_para.push(self.ref_collector.refs);
        }

        // Get HTML segments from writer
        let html_segments = self.writer.into_segments();

        Ok(WriterResult {
            html_segments,
            offset_maps_by_paragraph: self.offset_maps_by_para,
            paragraph_ranges: self.paragraph_ranges,
            syntax_spans_by_paragraph: self.syntax_spans_by_para,
            collected_refs_by_paragraph: self.refs_by_para,
        })
    }

    // Consume raw text events until end tag, for alt attributes
    #[allow(dead_code)]
    fn raw_text(&mut self) -> Result<(), fmt::Error> {
        use Event::*;
        let mut nest = 0;
        while let Some((event, _range)) = self.events.next() {
            match event {
                Start(_) => nest += 1,
                End(_) => {
                    if nest == 0 {
                        break;
                    }
                    nest -= 1;
                }
                Html(_) => {}
                InlineHtml(text) | Code(text) | Text(text) => {
                    // Don't use escape_html_body_text here.
                    // The output of this function is used in the `alt` attribute.
                    escape_html(&mut self.writer, &text)?;
                    self.end_newline = text.ends_with('\n');
                }
                InlineMath(text) => {
                    self.write("$")?;
                    escape_html(&mut self.writer, &text)?;
                    self.write("$")?;
                }
                DisplayMath(text) => {
                    self.write("$$")?;
                    escape_html(&mut self.writer, &text)?;
                    self.write("$$")?;
                }
                SoftBreak | HardBreak | Rule => {
                    self.write(" ")?;
                }
                FootnoteReference(name) => {
                    let len = self.numbers.len() + 1;
                    let number = *self.numbers.entry(name.into_string()).or_insert(len);
                    write!(&mut self.writer, "[{}]", number)?;
                }
                TaskListMarker(true) => self.write("[x]")?,
                TaskListMarker(false) => self.write("[ ]")?,
                WeaverBlock(_) => {}
            }
        }
        Ok(())
    }

    /// Consume events until End tag without writing anything.
    /// Used when we've already extracted content from source and just need to advance the iterator.
    fn consume_until_end(&mut self) {
        use Event::*;
        let mut nest = 0;
        while let Some((event, _range)) = self.events.next() {
            match event {
                Start(_) => nest += 1,
                End(_) => {
                    if nest == 0 {
                        break;
                    }
                    nest -= 1;
                }
                _ => {}
            }
        }
    }

    /// Parse WeaverBlock text content into attributes.
    /// Format: comma-separated, colon for key:value, otherwise class.
    /// Example: ".aside, width: 300px" -> classes: ["aside"], attrs: [("width", "300px")]
    fn parse_weaver_attrs(text: &str) -> WeaverAttributes<'static> {
        let mut classes = Vec::new();
        let mut attrs = Vec::new();

        for part in text.split(',') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }

            if let Some((key, value)) = part.split_once(':') {
                let key = key.trim();
                let value = value.trim();
                if !key.is_empty() && !value.is_empty() {
                    attrs.push((
                        CowStr::from(key.to_string()),
                        CowStr::from(value.to_string()),
                    ));
                }
            } else {
                // No colon - treat as class, strip leading dot if present
                let class = part.strip_prefix('.').unwrap_or(part);
                if !class.is_empty() {
                    classes.push(CowStr::from(class.to_string()));
                }
            }
        }

        WeaverAttributes { classes, attrs }
    }

    /// Emit wrapper element start based on pending block attrs.
    /// Returns true if a wrapper was emitted.
    fn emit_wrapper_start(&mut self) -> Result<bool, fmt::Error> {
        if let Some(attrs) = self.pending_block_attrs.take() {
            let is_aside = attrs.classes.iter().any(|c| c.as_ref() == "aside");

            if !self.end_newline {
                self.write("\n")?;
            }

            if is_aside {
                self.write("<aside")?;
                self.active_wrapper = Some(WrapperElement::Aside);
            } else {
                self.write("<div")?;
                self.active_wrapper = Some(WrapperElement::Div);
            }

            // Write classes (excluding "aside" if using <aside> element)
            let classes: Vec<_> = if is_aside {
                attrs
                    .classes
                    .iter()
                    .filter(|c| c.as_ref() != "aside")
                    .collect()
            } else {
                attrs.classes.iter().collect()
            };

            if !classes.is_empty() {
                self.write(" class=\"")?;
                for (i, class) in classes.iter().enumerate() {
                    if i > 0 {
                        self.write(" ")?;
                    }
                    escape_html(&mut self.writer, class)?;
                }
                self.write("\"")?;
            }

            // Write other attrs
            for (attr, value) in &attrs.attrs {
                self.write(" ")?;
                escape_html(&mut self.writer, attr)?;
                self.write("=\"")?;
                escape_html(&mut self.writer, value)?;
                self.write("\"")?;
            }

            self.write(">\n")?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Close active wrapper element if one is open
    fn close_wrapper(&mut self) -> Result<(), fmt::Error> {
        if let Some(wrapper) = self.active_wrapper.take() {
            match wrapper {
                WrapperElement::Aside => self.write("</aside>\n")?,
                WrapperElement::Div => self.write("</div>\n")?,
            }
        }
        Ok(())
    }

    fn process_event(&mut self, event: Event<'_>, range: Range<usize>) -> Result<(), fmt::Error> {
        use Event::*;

        match event {
            Start(tag) => self.start_tag(tag, range)?,
            End(tag) => self.end_tag(tag, range)?,
            Text(text) => {
                // If buffering code, append to buffer instead of writing
                if let Some((_, ref mut buffer)) = self.code_buffer {
                    buffer.push_str(&text);

                    // Track byte and char ranges for code block content
                    let text_char_len = text.chars().count();
                    let text_byte_len = text.len();
                    if let Some(ref mut code_byte_range) = self.code_buffer_byte_range {
                        // Extend existing ranges
                        code_byte_range.end = range.end;
                        if let Some(ref mut code_char_range) = self.code_buffer_char_range {
                            code_char_range.end = self.last_char_offset + text_char_len;
                        }
                    } else {
                        // First text in code block - start tracking
                        self.code_buffer_byte_range = Some(range.clone());
                        self.code_buffer_char_range =
                            Some(self.last_char_offset..self.last_char_offset + text_char_len);
                    }
                    // Update offsets so paragraph boundary is correct
                    self.last_char_offset += text_char_len;
                    self.last_byte_offset += text_byte_len;
                } else if !self.in_non_writing_block {
                    // Escape HTML and count chars in one pass
                    let char_start = self.last_char_offset;
                    let text_char_len =
                        escape_html_body_text_with_char_count(&mut self.writer, &text)?;
                    let char_end = char_start + text_char_len;

                    // Text becomes a text node child of the current container
                    if text_char_len > 0 {
                        self.current_node_child_count += 1;
                    }

                    // Record offset mapping
                    self.record_mapping(range.clone(), char_start..char_end);

                    // Update char offset tracking
                    self.last_char_offset = char_end;
                    self.end_newline = text.ends_with('\n');
                }
            }
            Code(text) => {
                let format_start = self.last_char_offset;
                let raw_text = &self.source[range.clone()];

                // Track opening span index so we can set formatted_range later
                let opening_span_idx = if raw_text.starts_with('`') {
                    let syn_id = self.gen_syn_id();
                    let char_start = self.last_char_offset;
                    let backtick_char_end = char_start + 1;
                    write!(
                        &mut self.writer,
                        "<span class=\"md-syntax-inline\" data-syn-id=\"{}\" data-char-start=\"{}\" data-char-end=\"{}\" spellcheck=\"false\">`</span>",
                        syn_id, char_start, backtick_char_end
                    )?;
                    self.syntax_spans.push(SyntaxSpanInfo {
                        syn_id,
                        char_range: char_start..backtick_char_end,
                        syntax_type: SyntaxType::Inline,
                        formatted_range: None, // Set after we know the full range
                    });
                    self.last_char_offset += 1;
                    Some(self.syntax_spans.len() - 1)
                } else {
                    None
                };

                self.write("<code>")?;

                // Track offset mapping for code content
                let content_char_start = self.last_char_offset;
                let text_char_len = escape_html_body_text_with_char_count(&mut self.writer, &text)?;
                let content_char_end = content_char_start + text_char_len;

                // Record offset mapping (code content is visible)
                self.record_mapping(range.clone(), content_char_start..content_char_end);
                self.last_char_offset = content_char_end;

                self.write("</code>")?;

                // Emit closing backtick and track it
                if raw_text.ends_with('`') {
                    let syn_id = self.gen_syn_id();
                    let backtick_char_start = self.last_char_offset;
                    let backtick_char_end = backtick_char_start + 1;
                    write!(
                        &mut self.writer,
                        "<span class=\"md-syntax-inline\" data-syn-id=\"{}\" data-char-start=\"{}\" data-char-end=\"{}\" spellcheck=\"false\">`</span>",
                        syn_id, backtick_char_start, backtick_char_end
                    )?;

                    // Now we know the full formatted range
                    let formatted_range = format_start..backtick_char_end;

                    self.syntax_spans.push(SyntaxSpanInfo {
                        syn_id,
                        char_range: backtick_char_start..backtick_char_end,
                        syntax_type: SyntaxType::Inline,
                        formatted_range: Some(formatted_range.clone()),
                    });

                    // Update opening span with formatted_range
                    if let Some(idx) = opening_span_idx {
                        self.syntax_spans[idx].formatted_range = Some(formatted_range);
                    }

                    self.last_char_offset += 1;
                }
            }
            InlineMath(text) => {
                // Math rendering follows embed pattern: syntax spans hide when cursor outside,
                // rendered content always visible
                let raw_text = &self.source[range.clone()];
                let syn_id = self.gen_syn_id();
                let opening_char_start = self.last_char_offset;

                // Calculate char positions
                let text_char_len = text.chars().count();
                let opening_char_end = opening_char_start + 1; // "$"
                let content_char_start = opening_char_end;
                let content_char_end = content_char_start + text_char_len;
                let closing_char_start = content_char_end;
                let closing_char_end = closing_char_start + 1; // "$"
                let formatted_range = opening_char_start..closing_char_end;

                // 1. Emit opening $ syntax span
                if raw_text.starts_with('$') {
                    write!(
                        &mut self.writer,
                        "<span class=\"md-syntax-inline\" data-syn-id=\"{}\" data-char-start=\"{}\" data-char-end=\"{}\" spellcheck=\"false\">$</span>",
                        syn_id, opening_char_start, opening_char_end
                    )?;
                    self.syntax_spans.push(SyntaxSpanInfo {
                        syn_id: syn_id.clone(),
                        char_range: opening_char_start..opening_char_end,
                        syntax_type: SyntaxType::Inline,
                        formatted_range: Some(formatted_range.clone()),
                    });
                    self.record_mapping(
                        range.start..range.start + 1,
                        opening_char_start..opening_char_end,
                    );
                }

                // 2. Emit raw LaTeX content (hidden with syntax when cursor outside)
                write!(
                    &mut self.writer,
                    "<span class=\"math-source\" data-syn-id=\"{}\" data-char-start=\"{}\" data-char-end=\"{}\" spellcheck=\"false\">",
                    syn_id, content_char_start, content_char_end
                )?;
                escape_html(&mut self.writer, &text)?;
                self.write("</span>")?;
                self.syntax_spans.push(SyntaxSpanInfo {
                    syn_id: syn_id.clone(),
                    char_range: content_char_start..content_char_end,
                    syntax_type: SyntaxType::Inline,
                    formatted_range: Some(formatted_range.clone()),
                });
                self.record_mapping(
                    range.start + 1..range.end - 1,
                    content_char_start..content_char_end,
                );

                // 3. Emit closing $ syntax span
                if raw_text.ends_with('$') {
                    write!(
                        &mut self.writer,
                        "<span class=\"md-syntax-inline\" data-syn-id=\"{}\" data-char-start=\"{}\" data-char-end=\"{}\" spellcheck=\"false\">$</span>",
                        syn_id, closing_char_start, closing_char_end
                    )?;
                    self.syntax_spans.push(SyntaxSpanInfo {
                        syn_id: syn_id.clone(),
                        char_range: closing_char_start..closing_char_end,
                        syntax_type: SyntaxType::Inline,
                        formatted_range: Some(formatted_range.clone()),
                    });
                    self.record_mapping(
                        range.end - 1..range.end,
                        closing_char_start..closing_char_end,
                    );
                }

                // 4. Emit rendered MathML (always visible, not tied to syn_id)
                // Include data-char-target so clicking moves cursor into the math region
                // contenteditable="false" so DOM walker skips this for offset counting
                match weaver_renderer::math::render_math(&text, false) {
                    weaver_renderer::math::MathResult::Success(mathml) => {
                        write!(
                            &mut self.writer,
                            "<span class=\"math math-inline math-rendered math-clickable\" contenteditable=\"false\" data-char-target=\"{}\">{}</span>",
                            content_char_start, mathml
                        )?;
                    }
                    weaver_renderer::math::MathResult::Error { html, .. } => {
                        // Show error indicator (also always visible)
                        self.write(&html)?;
                    }
                }

                self.last_char_offset = closing_char_end;
            }
            DisplayMath(text) => {
                // Math rendering follows embed pattern: syntax spans hide when cursor outside,
                // rendered content always visible
                let raw_text = &self.source[range.clone()];
                let syn_id = self.gen_syn_id();
                let opening_char_start = self.last_char_offset;

                // Calculate char positions
                let text_char_len = text.chars().count();
                let opening_char_end = opening_char_start + 2; // "$$"
                let content_char_start = opening_char_end;
                let content_char_end = content_char_start + text_char_len;
                let closing_char_start = content_char_end;
                let closing_char_end = closing_char_start + 2; // "$$"
                let formatted_range = opening_char_start..closing_char_end;

                // 1. Emit opening $$ syntax span
                // Use Block syntax type so visibility is based on "cursor in same paragraph"
                if raw_text.starts_with("$$") {
                    write!(
                        &mut self.writer,
                        "<span class=\"md-syntax-block\" data-syn-id=\"{}\" data-char-start=\"{}\" data-char-end=\"{}\" spellcheck=\"false\">$$</span>",
                        syn_id, opening_char_start, opening_char_end
                    )?;
                    self.syntax_spans.push(SyntaxSpanInfo {
                        syn_id: syn_id.clone(),
                        char_range: opening_char_start..opening_char_end,
                        syntax_type: SyntaxType::Block,
                        formatted_range: Some(formatted_range.clone()),
                    });
                    self.record_mapping(
                        range.start..range.start + 2,
                        opening_char_start..opening_char_end,
                    );
                }

                // 2. Emit raw LaTeX content (hidden with syntax when cursor outside)
                write!(
                    &mut self.writer,
                    "<span class=\"math-source\" data-syn-id=\"{}\" data-char-start=\"{}\" data-char-end=\"{}\" spellcheck=\"false\">",
                    syn_id, content_char_start, content_char_end
                )?;
                escape_html(&mut self.writer, &text)?;
                self.write("</span>")?;
                self.syntax_spans.push(SyntaxSpanInfo {
                    syn_id: syn_id.clone(),
                    char_range: content_char_start..content_char_end,
                    syntax_type: SyntaxType::Block,
                    formatted_range: Some(formatted_range.clone()),
                });
                self.record_mapping(
                    range.start + 2..range.end - 2,
                    content_char_start..content_char_end,
                );

                // 3. Emit closing $$ syntax span
                if raw_text.ends_with("$$") {
                    write!(
                        &mut self.writer,
                        "<span class=\"md-syntax-block\" data-syn-id=\"{}\" data-char-start=\"{}\" data-char-end=\"{}\" spellcheck=\"false\">$$</span>",
                        syn_id, closing_char_start, closing_char_end
                    )?;
                    self.syntax_spans.push(SyntaxSpanInfo {
                        syn_id: syn_id.clone(),
                        char_range: closing_char_start..closing_char_end,
                        syntax_type: SyntaxType::Block,
                        formatted_range: Some(formatted_range.clone()),
                    });
                    self.record_mapping(
                        range.end - 2..range.end,
                        closing_char_start..closing_char_end,
                    );
                }

                // 4. Emit rendered MathML (always visible, not tied to syn_id)
                // Include data-char-target so clicking moves cursor into the math region
                // contenteditable="false" so DOM walker skips this for offset counting
                match weaver_renderer::math::render_math(&text, true) {
                    weaver_renderer::math::MathResult::Success(mathml) => {
                        write!(
                            &mut self.writer,
                            "<span class=\"math math-display math-rendered math-clickable\" contenteditable=\"false\" data-char-target=\"{}\">{}</span>",
                            content_char_start, mathml
                        )?;
                    }
                    weaver_renderer::math::MathResult::Error { html, .. } => {
                        // Show error indicator (also always visible)
                        self.write(&html)?;
                    }
                }

                self.last_char_offset = closing_char_end;
            }
            Html(html) => {
                // Track offset mapping for raw HTML
                let char_start = self.last_char_offset;
                let html_char_len = html.chars().count();
                let char_end = char_start + html_char_len;

                self.write(&html)?;

                // Record mapping for inline HTML
                self.record_mapping(range.clone(), char_start..char_end);
                self.last_char_offset = char_end;
            }
            InlineHtml(html) => {
                // Track offset mapping for raw HTML
                let char_start = self.last_char_offset;
                let html_char_len = html.chars().count();
                let char_end = char_start + html_char_len;
                self.write(r#"<span class="html-embed html-embed-inline">"#)?;
                self.write(&html)?;
                self.write("</span>")?;
                // Record mapping for inline HTML
                self.record_mapping(range.clone(), char_start..char_end);
                self.last_char_offset = char_end;
            }
            SoftBreak => {
                // Emit <br> for visual line break, plus a space for cursor positioning.
                // This space maps to the \n so the cursor can land here when navigating.
                let char_start = self.last_char_offset;

                // Emit <br>
                self.write("<br />")?;
                self.current_node_child_count += 1;

                // Emit space for cursor positioning - this gives the browser somewhere
                // to place the cursor when navigating to this line
                self.write(" ")?;
                self.current_node_child_count += 1;

                // Map the space to the newline position - cursor landing here means
                // we're at the end of the line (after the \n)
                if let Some(ref node_id) = self.current_node_id {
                    let mapping = OffsetMapping {
                        byte_range: range.clone(),
                        char_range: char_start..char_start + 1,
                        node_id: node_id.clone(),
                        char_offset_in_node: self.current_node_char_offset,
                        child_index: None,
                        utf16_len: 1, // the space we emitted
                    };
                    self.offset_maps.push(mapping);
                    self.current_node_char_offset += 1;
                }

                self.last_char_offset = char_start + 1; // +1 for the \n
            }
            HardBreak => {
                // Emit the two spaces as visible (dimmed) text, then <br>
                let gap = &self.source[range.clone()];
                if gap.ends_with('\n') {
                    let spaces = &gap[..gap.len() - 1]; // everything except the \n
                    let char_start = self.last_char_offset;
                    let spaces_char_len = spaces.chars().count();
                    let char_end = char_start + spaces_char_len;

                    // Emit and map the visible spaces
                    let syn_id = self.gen_syn_id();
                    write!(
                        &mut self.writer,
                        "<span class=\"md-placeholder\" data-syn-id=\"{}\" data-char-start=\"{}\" data-char-end=\"{}\">",
                        syn_id, char_start, char_end
                    )?;
                    escape_html(&mut self.writer, spaces)?;
                    self.write("</span>")?;

                    // Count this span as a child
                    self.current_node_child_count += 1;

                    self.record_mapping(
                        range.start..range.start + spaces.len(),
                        char_start..char_end,
                    );

                    // Now the actual line break <br>
                    self.write("<br />")?;

                    // Count the <br> as a child
                    self.current_node_child_count += 1;

                    // After <br>, emit plain zero-width space for cursor positioning
                    self.write(" ")?;

                    // Count the zero-width space text node as a child
                    self.current_node_child_count += 1;

                    // Map the newline position to the zero-width space text node
                    if let Some(ref node_id) = self.current_node_id {
                        let newline_char_offset = char_start + spaces_char_len;
                        let mapping = OffsetMapping {
                            byte_range: range.start + spaces.len()..range.end,
                            char_range: newline_char_offset..newline_char_offset + 1,
                            node_id: node_id.clone(),
                            char_offset_in_node: self.current_node_char_offset,
                            child_index: None, // text node - TreeWalker will find it
                            utf16_len: 1,      // zero-width space is 1 UTF-16 unit
                        };
                        self.offset_maps.push(mapping);

                        // Increment char offset - TreeWalker will encounter this text node
                        self.current_node_char_offset += 1;
                    }

                    // DO NOT increment last_char_offset - zero-width space is not in source
                    // The \n itself IS in source, so we already accounted for it
                    self.last_char_offset = char_start + spaces_char_len + 1; // +1 for \n
                } else {
                    // Fallback: just <br>
                    self.write("<br />")?;
                }
            }
            Rule => {
                if !self.end_newline {
                    self.write("\n")?;
                }

                // Emit syntax span before the rendered element
                if range.start < range.end {
                    let raw_text = &self.source[range];
                    let trimmed = raw_text.trim();
                    if !trimmed.is_empty() {
                        let syn_id = self.gen_syn_id();
                        let char_start = self.last_char_offset;
                        let char_len = trimmed.chars().count();
                        let char_end = char_start + char_len;

                        write!(
                            &mut self.writer,
                            "<span class=\"md-syntax-block\" data-syn-id=\"{}\" data-char-start=\"{}\" data-char-end=\"{}\" spellcheck=\"false\">",
                            syn_id, char_start, char_end
                        )?;
                        escape_html(&mut self.writer, trimmed)?;
                        self.write("</span>\n")?;

                        self.syntax_spans.push(SyntaxSpanInfo {
                            syn_id,
                            char_range: char_start..char_end,
                            syntax_type: SyntaxType::Block,
                            formatted_range: None,
                        });
                    }
                }

                // Wrap <hr /> in toggle-block for future cursor-based toggling
                self.write("<div class=\"toggle-block\"><hr /></div>\n")?;
            }
            FootnoteReference(name) => {
                // Get/create footnote number
                let len = self.numbers.len() + 1;
                let number = *self.numbers.entry(name.to_string()).or_insert(len);

                // Emit the [^name] syntax as a hideable syntax span
                let raw_text = &self.source[range.clone()];
                let char_start = self.last_char_offset;
                let syntax_char_len = raw_text.chars().count();
                let char_end = char_start + syntax_char_len;
                let syn_id = self.gen_syn_id();

                write!(
                    &mut self.writer,
                    "<span class=\"md-syntax-inline\" data-syn-id=\"{}\" data-char-start=\"{}\" data-char-end=\"{}\" spellcheck=\"false\">",
                    syn_id, char_start, char_end
                )?;
                escape_html(&mut self.writer, raw_text)?;
                self.write("</span>")?;

                // Track this span for linking with the footnote definition later
                let span_index = self.syntax_spans.len();
                self.syntax_spans.push(SyntaxSpanInfo {
                    syn_id,
                    char_range: char_start..char_end,
                    syntax_type: SyntaxType::Inline,
                    formatted_range: None, // Set when we see the definition
                });
                self.footnote_ref_spans
                    .insert(name.to_string(), (span_index, char_start));

                // Record offset mapping for the syntax span content
                self.record_mapping(range.clone(), char_start..char_end);

                // Count as child
                self.current_node_child_count += 1;

                // Emit the visible footnote reference (superscript number)
                write!(
                    &mut self.writer,
                    "<sup class=\"footnote-reference\"><a href=\"#fn-{}\">{}</a></sup>",
                    name, number
                )?;

                // Update tracking
                self.last_char_offset = char_end;
                self.last_byte_offset = range.end;
            }
            TaskListMarker(checked) => {
                // Emit the [ ] or [x] syntax
                if range.start < range.end {
                    let raw_text = &self.source[range];
                    if let Some(bracket_pos) = raw_text.find('[') {
                        let end_pos = raw_text.find(']').map(|p| p + 1).unwrap_or(bracket_pos + 3);
                        let syntax = &raw_text[bracket_pos..end_pos.min(raw_text.len())];

                        let syn_id = self.gen_syn_id();
                        let char_start = self.last_char_offset;
                        let syntax_char_len = syntax.chars().count();
                        let char_end = char_start + syntax_char_len;

                        write!(
                            &mut self.writer,
                            "<span class=\"md-syntax-inline\" data-syn-id=\"{}\" data-char-start=\"{}\" data-char-end=\"{}\" spellcheck=\"false\">",
                            syn_id, char_start, char_end
                        )?;
                        escape_html(&mut self.writer, syntax)?;
                        self.write("</span> ")?;

                        self.syntax_spans.push(SyntaxSpanInfo {
                            syn_id,
                            char_range: char_start..char_end,
                            syntax_type: SyntaxType::Inline,
                            formatted_range: None,
                        });
                    }
                }

                if checked {
                    self.write("<input disabled=\"\" type=\"checkbox\" checked=\"\"/>\n")?;
                } else {
                    self.write("<input disabled=\"\" type=\"checkbox\"/>\n")?;
                }
            }
            WeaverBlock(text) => {
                // Buffer WeaverBlock content for parsing on End
                self.weaver_block_buffer.push_str(&text);
            }
        }
        Ok(())
    }
}
