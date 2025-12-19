//! HTML writer for markdown editor with visible formatting characters.
//!
//! Based on ClientWriter from weaver-renderer, but modified to preserve
//! formatting characters (**, *, #, etc) wrapped in styled spans.
//!
//! Uses Parser::into_offset_iter() to track gaps between events, which
//! represent consumed formatting characters.
#[allow(unused_imports)]
use super::offset_map::{OffsetMapping, RenderResult};
use jacquard::types::{ident::AtIdentifier, string::Rkey};
use loro::LoroText;
use markdown_weaver::{
    Alignment, BlockQuoteKind, CodeBlockKind, CowStr, EmbedType, Event, LinkType, Tag,
    WeaverAttributes,
};
use markdown_weaver_escape::{
    StrWrite, escape_href, escape_html, escape_html_body_text,
    escape_html_body_text_with_char_count,
};
use std::collections::HashMap;
use std::fmt;
use std::ops::Range;
use weaver_common::{EntryIndex, ResolvedContent};

/// Writer that segments output by paragraph boundaries.
///
/// Each paragraph's HTML is written to a separate String in the segments Vec.
/// Call `new_segment()` at paragraph boundaries to start a new segment.
#[derive(Debug, Clone, Default)]
pub struct SegmentedWriter {
    segments: Vec<String>,
}

#[allow(dead_code)]
impl SegmentedWriter {
    pub fn new() -> Self {
        Self {
            segments: vec![String::new()],
        }
    }

    /// Start a new segment for the next paragraph.
    pub fn new_segment(&mut self) {
        self.segments.push(String::new());
    }

    /// Get the completed segments.
    pub fn into_segments(self) -> Vec<String> {
        self.segments
    }

    /// Get current segment count.
    pub fn segment_count(&self) -> usize {
        self.segments.len()
    }
}

impl StrWrite for SegmentedWriter {
    type Error = fmt::Error;

    #[inline]
    fn write_str(&mut self, s: &str) -> Result<(), Self::Error> {
        if let Some(segment) = self.segments.last_mut() {
            segment.push_str(s);
        }
        Ok(())
    }

    #[inline]
    fn write_fmt(&mut self, args: fmt::Arguments) -> Result<(), Self::Error> {
        if let Some(segment) = self.segments.last_mut() {
            fmt::Write::write_fmt(segment, args)?;
        }
        Ok(())
    }
}

impl fmt::Write for SegmentedWriter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        <Self as StrWrite>::write_str(self, s)
    }

    fn write_fmt(&mut self, args: fmt::Arguments<'_>) -> fmt::Result {
        <Self as StrWrite>::write_fmt(self, args)
    }
}

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

/// Classification of markdown syntax characters
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyntaxType {
    /// Inline formatting: **, *, ~~, `, $, [, ], (, )
    Inline,
    /// Block formatting: #, >, -, *, 1., ```, ---
    Block,
}

/// Information about a syntax span for conditional visibility
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyntaxSpanInfo {
    /// Unique identifier for this syntax span (e.g., "s0", "s1")
    pub syn_id: String,
    /// Source char range this syntax covers (just this marker)
    pub char_range: Range<usize>,
    /// Whether this is inline or block-level syntax
    pub syntax_type: SyntaxType,
    /// For paired inline syntax (**, *, etc), the full formatted region
    /// from opening marker through content to closing marker.
    /// When cursor is anywhere in this range, the syntax is visible.
    pub formatted_range: Option<Range<usize>>,
}

impl SyntaxSpanInfo {
    /// Adjust all position fields by a character delta.
    ///
    /// This adjusts both `char_range` and `formatted_range` (if present) together,
    /// ensuring they stay in sync. Use this instead of manually adjusting fields
    /// to avoid forgetting one.
    pub fn adjust_positions(&mut self, char_delta: isize) {
        self.char_range.start = (self.char_range.start as isize + char_delta) as usize;
        self.char_range.end = (self.char_range.end as isize + char_delta) as usize;
        if let Some(ref mut fr) = self.formatted_range {
            fr.start = (fr.start as isize + char_delta) as usize;
            fr.end = (fr.end as isize + char_delta) as usize;
        }
    }
}

/// Classify syntax text as inline or block level
fn classify_syntax(text: &str) -> SyntaxType {
    let trimmed = text.trim_start();

    // Check for block-level markers
    if trimmed.starts_with('#')
        || trimmed.starts_with('>')
        || trimmed.starts_with("```")
        || trimmed.starts_with("---")
        || (trimmed.starts_with('-')
            && trimmed
                .chars()
                .nth(1)
                .map(|c| c.is_whitespace())
                .unwrap_or(false))
        || (trimmed.starts_with('*')
            && trimmed
                .chars()
                .nth(1)
                .map(|c| c.is_whitespace())
                .unwrap_or(false))
        || trimmed
            .chars()
            .next()
            .map(|c| c.is_ascii_digit())
            .unwrap_or(false)
            && trimmed.contains('.')
    {
        SyntaxType::Block
    } else {
        SyntaxType::Inline
    }
}

/// Synchronous callback for injecting embed content
///
/// Takes the embed tag and returns optional HTML content to inject.
pub trait EmbedContentProvider {
    fn get_embed_content(&self, tag: &Tag<'_>) -> Option<String>;
}

impl EmbedContentProvider for () {
    fn get_embed_content(&self, _tag: &Tag<'_>) -> Option<String> {
        None
    }
}

impl EmbedContentProvider for &ResolvedContent {
    fn get_embed_content(&self, tag: &Tag<'_>) -> Option<String> {
        if let Tag::Embed { dest_url, .. } = tag {
            let url = dest_url.as_ref();
            if url.starts_with("at://") {
                if let Ok(at_uri) = jacquard::types::string::AtUri::new(url) {
                    return ResolvedContent::get_embed_content(self, &at_uri)
                        .map(|s| s.to_string());
                }
            }
        }
        None
    }
}

/// Resolves image URLs to CDN URLs based on stored images.
///
/// The markdown may reference images by name (e.g., "photo.jpg" or "/notebook/image.png").
/// This trait maps those names to the actual CDN URL using the blob CID and owner DID.
pub trait ImageResolver {
    /// Resolve an image URL from markdown to a CDN URL.
    ///
    /// Returns `Some(cdn_url)` if the image is found, `None` to use the original URL.
    fn resolve_image_url(&self, url: &str) -> Option<String>;
}

impl ImageResolver for () {
    fn resolve_image_url(&self, _url: &str) -> Option<String> {
        None
    }
}

/// Concrete image resolver that maps image names to URLs.
///
/// Resolved image path type
#[derive(Clone, Debug)]
enum ResolvedImage {
    /// Data URL for immediate preview (still uploading)
    Pending(String),
    /// Draft image: `/image/{ident}/draft/{blob_rkey}/{name}`
    Draft {
        blob_rkey: Rkey<'static>,
        ident: AtIdentifier<'static>,
    },
    /// Published image: `/image/{ident}/{entry_rkey}/{name}`
    Published {
        entry_rkey: Rkey<'static>,
        ident: AtIdentifier<'static>,
    },
}

/// Resolves image paths in the editor.
///
/// Supports three states for images:
/// - Pending: uses data URL for immediate preview while upload is in progress
/// - Draft: uses path format `/image/{did}/draft/{blob_rkey}/{name}`
/// - Published: uses path format `/image/{did}/{entry_rkey}/{name}`
///
/// Image URLs in markdown use the format `/image/{name}`.
#[derive(Clone, Default)]
pub struct EditorImageResolver {
    /// All resolved images: name -> resolved path info
    images: std::collections::HashMap<String, ResolvedImage>,
}

impl EditorImageResolver {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a pending image with a data URL for immediate preview.
    pub fn add_pending(&mut self, name: String, data_url: String) {
        self.images.insert(name, ResolvedImage::Pending(data_url));
    }

    /// Promote a pending image to uploaded (draft) status.
    pub fn promote_to_uploaded(
        &mut self,
        name: &str,
        blob_rkey: Rkey<'static>,
        ident: AtIdentifier<'static>,
    ) {
        self.images
            .insert(name.to_string(), ResolvedImage::Draft { blob_rkey, ident });
    }

    /// Add an already-uploaded draft image.
    pub fn add_uploaded(
        &mut self,
        name: String,
        blob_rkey: Rkey<'static>,
        ident: AtIdentifier<'static>,
    ) {
        self.images
            .insert(name, ResolvedImage::Draft { blob_rkey, ident });
    }

    /// Add a published image.
    pub fn add_published(
        &mut self,
        name: String,
        entry_rkey: Rkey<'static>,
        ident: AtIdentifier<'static>,
    ) {
        self.images
            .insert(name, ResolvedImage::Published { entry_rkey, ident });
    }

    /// Check if an image is pending upload.
    pub fn is_pending(&self, name: &str) -> bool {
        matches!(self.images.get(name), Some(ResolvedImage::Pending(_)))
    }

    /// Build a resolver from editor images and user identifier.
    ///
    /// For draft mode (entry_rkey=None), only images with a `published_blob_uri` are included.
    /// For published mode (entry_rkey=Some), all images are included.
    pub fn from_images<'a>(
        images: impl IntoIterator<Item = &'a super::document::EditorImage>,
        ident: AtIdentifier<'static>,
        entry_rkey: Option<Rkey<'static>>,
    ) -> Self {
        use jacquard::IntoStatic;

        let mut resolver = Self::new();
        for editor_image in images {
            // Get the name from the Image (use alt text as fallback if name is empty)
            let name = editor_image
                .image
                .name
                .as_ref()
                .map(|n| n.to_string())
                .unwrap_or_else(|| editor_image.image.alt.to_string());

            if name.is_empty() {
                continue;
            }

            match &entry_rkey {
                // Published mode: use entry rkey for all images
                Some(rkey) => {
                    resolver.add_published(name, rkey.clone(), ident.clone());
                }
                // Draft mode: use published_blob_uri rkey
                None => {
                    let blob_rkey = match &editor_image.published_blob_uri {
                        Some(uri) => match uri.rkey() {
                            Some(rkey) => rkey.0.clone().into_static(),
                            None => continue,
                        },
                        None => continue,
                    };
                    resolver.add_uploaded(name, blob_rkey, ident.clone());
                }
            }
        }
        resolver
    }
}

impl ImageResolver for EditorImageResolver {
    fn resolve_image_url(&self, url: &str) -> Option<String> {
        // Extract image name from /image/{name} format
        let name = url.strip_prefix("/image/").unwrap_or(url);

        let resolved = self.images.get(name)?;
        match resolved {
            ResolvedImage::Pending(data_url) => Some(data_url.clone()),
            ResolvedImage::Draft { blob_rkey, ident } => {
                Some(format!("/image/{}/draft/{}/{}", ident, blob_rkey, name))
            }
            ResolvedImage::Published { entry_rkey, ident } => {
                Some(format!("/image/{}/{}/{}", ident, entry_rkey, name))
            }
        }
    }
}

impl ImageResolver for &EditorImageResolver {
    fn resolve_image_url(&self, url: &str) -> Option<String> {
        (*self).resolve_image_url(url)
    }
}

/// Tracks the type of wrapper element emitted for WeaverBlock prefix
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WrapperElement {
    Aside,
    Div,
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

#[derive(Debug, Clone, Copy)]
enum TableState {
    Head,
    Body,
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

    /// Emit syntax span for a given range and record offset mapping
    fn emit_syntax(&mut self, range: Range<usize>) -> Result<(), fmt::Error> {
        if range.start < range.end {
            let syntax = &self.source[range.clone()];
            if !syntax.is_empty() {
                let char_start = self.last_char_offset;
                let syntax_char_len = syntax.chars().count();
                let char_end = char_start + syntax_char_len;

                tracing::trace!(
                    target: "weaver::writer",
                    byte_range = ?range,
                    char_range = ?(char_start..char_end),
                    syntax = %syntax.escape_debug(),
                    "emit_syntax"
                );

                // Whitespace-only content (trailing spaces, newlines) should be emitted
                // as plain text, not wrapped in a hideable syntax span
                let is_whitespace_only = syntax.trim().is_empty();

                if is_whitespace_only {
                    // Emit as plain text with tracking span (not hideable)
                    let created_node = if self.current_node_id.is_none() {
                        let node_id = self.gen_node_id();
                        write!(&mut self.writer, "<span id=\"{}\">", node_id)?;
                        self.begin_node(node_id);
                        true
                    } else {
                        false
                    };

                    escape_html(&mut self.writer, syntax)?;

                    // Record offset mapping BEFORE end_node (which clears current_node_id)
                    self.record_mapping(range.clone(), char_start..char_end);
                    self.last_char_offset = char_end;
                    self.last_byte_offset = range.end;

                    if created_node {
                        self.write("</span>")?;
                        self.end_node();
                    }
                } else {
                    // Real syntax - wrap in hideable span
                    let syntax_type = classify_syntax(syntax);
                    let class = match syntax_type {
                        SyntaxType::Inline => "md-syntax-inline",
                        SyntaxType::Block => "md-syntax-block",
                    };

                    // Generate unique ID for this syntax span
                    let syn_id = self.gen_syn_id();

                    // If we're outside any node, create a wrapper span for tracking
                    let created_node = if self.current_node_id.is_none() {
                        let node_id = self.gen_node_id();
                        write!(
                            &mut self.writer,
                            "<span id=\"{}\" class=\"{}\" data-syn-id=\"{}\" data-char-start=\"{}\" data-char-end=\"{}\">",
                            node_id, class, syn_id, char_start, char_end
                        )?;
                        self.begin_node(node_id);
                        true
                    } else {
                        write!(
                            &mut self.writer,
                            "<span class=\"{}\" data-syn-id=\"{}\" data-char-start=\"{}\" data-char-end=\"{}\">",
                            class, syn_id, char_start, char_end
                        )?;
                        false
                    };

                    escape_html(&mut self.writer, syntax)?;
                    self.write("</span>")?;

                    // Record syntax span info for visibility toggling
                    self.syntax_spans.push(SyntaxSpanInfo {
                        syn_id,
                        char_range: char_start..char_end,
                        syntax_type,
                        formatted_range: None,
                    });

                    // Record offset mapping for this syntax
                    self.record_mapping(range.clone(), char_start..char_end);
                    self.last_char_offset = char_end;
                    self.last_byte_offset = range.end;

                    // Close wrapper if we created one
                    if created_node {
                        self.write("</span>")?;
                        self.end_node();
                    }
                }
            }
        }
        Ok(())
    }

    /// Emit syntax span inside current node with full offset tracking.
    ///
    /// Use this for syntax markers that appear inside block elements (headings, lists,
    /// blockquotes, code fences). Unlike `emit_syntax` which is for gaps and creates
    /// wrapper nodes, this assumes we're already inside a tracked node.
    ///
    /// - Writes `<span class="md-syntax-{class}">{syntax}</span>`
    /// - Records offset mapping (for cursor positioning)
    /// - Updates both `last_char_offset` and `last_byte_offset`
    fn emit_inner_syntax(
        &mut self,
        syntax: &str,
        byte_start: usize,
        syntax_type: SyntaxType,
    ) -> Result<(), fmt::Error> {
        if syntax.is_empty() {
            return Ok(());
        }

        let char_start = self.last_char_offset;
        let syntax_char_len = syntax.chars().count();
        let char_end = char_start + syntax_char_len;
        let byte_end = byte_start + syntax.len();

        let class_str = match syntax_type {
            SyntaxType::Inline => "md-syntax-inline",
            SyntaxType::Block => "md-syntax-block",
        };

        // Generate unique ID for this syntax span
        let syn_id = self.gen_syn_id();

        write!(
            &mut self.writer,
            "<span class=\"{}\" data-syn-id=\"{}\" data-char-start=\"{}\" data-char-end=\"{}\">",
            class_str, syn_id, char_start, char_end
        )?;
        escape_html(&mut self.writer, syntax)?;
        self.write("</span>")?;

        // Record syntax span info for visibility toggling
        self.syntax_spans.push(SyntaxSpanInfo {
            syn_id,
            char_range: char_start..char_end,
            syntax_type,
            formatted_range: None,
        });

        // Record offset mapping for cursor positioning
        self.record_mapping(byte_start..byte_end, char_start..char_end);

        self.last_char_offset = char_end;
        self.last_byte_offset = byte_end;

        Ok(())
    }

    /// Emit any gap between last position and next offset
    fn emit_gap_before(&mut self, next_offset: usize) -> Result<(), fmt::Error> {
        // Skip gap emission if we're inside a table being rendered as markdown
        if self.table_start_offset.is_some() && self.render_tables_as_markdown {
            return Ok(());
        }

        // Skip gap emission if we're buffering code block content
        // The code block handler manages its own syntax emission
        if self.code_buffer.is_some() {
            return Ok(());
        }

        if next_offset > self.last_byte_offset {
            self.emit_syntax(self.last_byte_offset..next_offset)?;
        }
        Ok(())
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
                    attrs.push((CowStr::from(key.to_string()), CowStr::from(value.to_string())));
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

    fn start_tag(&mut self, tag: Tag<'_>, range: Range<usize>) -> Result<(), fmt::Error> {
        // Check if this is a block-level tag that should have syntax inside
        let is_block_tag = matches!(tag, Tag::Heading { .. } | Tag::BlockQuote(_));

        // For inline tags, emit syntax before tag
        if !is_block_tag && range.start < range.end {
            let raw_text = &self.source[range.clone()];
            let opening_syntax = match &tag {
                Tag::Strong => {
                    if raw_text.starts_with("**") {
                        Some("**")
                    } else if raw_text.starts_with("__") {
                        Some("__")
                    } else {
                        None
                    }
                }
                Tag::Emphasis => {
                    if raw_text.starts_with("*") {
                        Some("*")
                    } else if raw_text.starts_with("_") {
                        Some("_")
                    } else {
                        None
                    }
                }
                Tag::Strikethrough => {
                    if raw_text.starts_with("~~") {
                        Some("~~")
                    } else {
                        None
                    }
                }
                Tag::Link { link_type, .. } => {
                    if matches!(link_type, LinkType::WikiLink { .. }) {
                        if raw_text.starts_with("[[") {
                            Some("[[")
                        } else {
                            None
                        }
                    } else if raw_text.starts_with('[') {
                        Some("[")
                    } else {
                        None
                    }
                }
                // Note: Tag::Image and Tag::Embed handle their own syntax spans
                // in their respective handlers, so don't emit here
                _ => None,
            };

            if let Some(syntax) = opening_syntax {
                let syntax_type = classify_syntax(syntax);
                let class = match syntax_type {
                    SyntaxType::Inline => "md-syntax-inline",
                    SyntaxType::Block => "md-syntax-block",
                };

                let char_start = self.last_char_offset;
                let syntax_char_len = syntax.chars().count();
                let char_end = char_start + syntax_char_len;
                let syntax_byte_len = syntax.len();

                // Generate unique ID for this syntax span
                let syn_id = self.gen_syn_id();

                write!(
                    &mut self.writer,
                    "<span class=\"{}\" data-syn-id=\"{}\" data-char-start=\"{}\" data-char-end=\"{}\">",
                    class, syn_id, char_start, char_end
                )?;
                escape_html(&mut self.writer, syntax)?;
                self.write("</span>")?;

                // Record syntax span info for visibility toggling
                self.syntax_spans.push(SyntaxSpanInfo {
                    syn_id: syn_id.clone(),
                    char_range: char_start..char_end,
                    syntax_type,
                    formatted_range: None, // Will be updated when closing tag is emitted
                });

                // Record offset mapping for cursor positioning
                // This is critical - without it, current_node_char_offset is wrong
                // and all subsequent cursor positions are shifted
                let byte_start = range.start;
                let byte_end = range.start + syntax_byte_len;
                self.record_mapping(byte_start..byte_end, char_start..char_end);

                // For paired inline syntax, track opening span for formatted_range
                if matches!(
                    tag,
                    Tag::Strong | Tag::Emphasis | Tag::Strikethrough | Tag::Link { .. }
                ) {
                    self.pending_inline_formats.push((syn_id, char_start));
                }

                // Update tracking - we've consumed this opening syntax
                self.last_char_offset = char_end;
                self.last_byte_offset = range.start + syntax_byte_len;
            }
        }

        // Emit the opening tag
        match tag {
            // HTML blocks get their own paragraph to try and corral them better
            Tag::HtmlBlock => {
                // Record paragraph start for boundary tracking
                // BUT skip if inside a list - list owns the paragraph boundary
                if self.list_depth == 0 {
                    self.current_paragraph_start =
                        Some((self.last_byte_offset, self.last_char_offset));
                }
                let node_id = self.gen_node_id();

                if self.end_newline {
                    write!(
                        &mut self.writer,
                        r#"<p id="{}" class="html-embed html-embed-block">"#,
                        node_id
                    )?;
                } else {
                    write!(
                        &mut self.writer,
                        r#"\n<p id="{}" class="html-embed html-embed-block">"#,
                        node_id
                    )?;
                }
                self.begin_node(node_id.clone());

                // Map the start position of the paragraph (before any content)
                // This allows cursor to be placed at the very beginning
                let para_start_char = self.last_char_offset;
                let mapping = OffsetMapping {
                    byte_range: range.start..range.start,
                    char_range: para_start_char..para_start_char,
                    node_id,
                    char_offset_in_node: 0,
                    child_index: Some(0), // position before first child
                    utf16_len: 0,
                };
                self.offset_maps.push(mapping);

                Ok(())
            }
            Tag::Paragraph(_) => {
                // Handle wrapper before block
                self.emit_wrapper_start()?;

                // Record paragraph start for boundary tracking
                // BUT skip if inside a list - list owns the paragraph boundary
                if self.list_depth == 0 {
                    self.current_paragraph_start =
                        Some((self.last_byte_offset, self.last_char_offset));
                }

                let node_id = self.gen_node_id();
                if self.end_newline {
                    write!(&mut self.writer, "<p id=\"{}\">", node_id)?;
                } else {
                    write!(&mut self.writer, "\n<p id=\"{}\">", node_id)?;
                }
                self.begin_node(node_id.clone());

                // Map the start position of the paragraph (before any content)
                // This allows cursor to be placed at the very beginning
                let para_start_char = self.last_char_offset;
                let mapping = OffsetMapping {
                    byte_range: range.start..range.start,
                    char_range: para_start_char..para_start_char,
                    node_id,
                    char_offset_in_node: 0,
                    child_index: Some(0), // position before first child
                    utf16_len: 0,
                };
                self.offset_maps.push(mapping);

                // Emit > syntax if we're inside a blockquote
                if let Some(bq_range) = self.pending_blockquote_range.take() {
                    if bq_range.start < bq_range.end {
                        let raw_text = &self.source[bq_range.clone()];
                        if let Some(gt_pos) = raw_text.find('>') {
                            // Extract > [!NOTE] or just >
                            let after_gt = &raw_text[gt_pos + 1..];
                            let syntax_end = if after_gt.trim_start().starts_with("[!") {
                                // Find the closing ]
                                if let Some(close_bracket) = after_gt.find(']') {
                                    gt_pos + 1 + close_bracket + 1
                                } else {
                                    gt_pos + 1
                                }
                            } else {
                                // Just > and maybe a space
                                (gt_pos + 1).min(raw_text.len())
                            };

                            let syntax = &raw_text[gt_pos..syntax_end];
                            let syntax_byte_start = bq_range.start + gt_pos;
                            self.emit_inner_syntax(syntax, syntax_byte_start, SyntaxType::Block)?;
                        }
                    }
                }
                Ok(())
            }
            Tag::Heading {
                level,
                id,
                classes,
                attrs,
            } => {
                // Emit wrapper if pending (but don't close on heading end - wraps following block too)
                self.emit_wrapper_start()?;

                // Record paragraph start for boundary tracking
                // Treat headings as paragraph-level blocks
                self.current_paragraph_start = Some((self.last_byte_offset, self.last_char_offset));

                if !self.end_newline {
                    self.write("\n")?;
                }

                // Generate node ID for offset tracking
                let node_id = self.gen_node_id();

                self.write("<")?;
                write!(&mut self.writer, "{}", level)?;

                // Add our tracking ID as data attribute (preserve user's id if present)
                self.write(" data-node-id=\"")?;
                self.write(&node_id)?;
                self.write("\"")?;

                if let Some(id) = id {
                    self.write(" id=\"")?;
                    escape_html(&mut self.writer, &id)?;
                    self.write("\"")?;
                }
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
                for (attr, value) in attrs {
                    self.write(" ")?;
                    escape_html(&mut self.writer, &attr)?;
                    if let Some(val) = value {
                        self.write("=\"")?;
                        escape_html(&mut self.writer, &val)?;
                        self.write("\"")?;
                    } else {
                        self.write("=\"\"")?;
                    }
                }
                self.write(">")?;

                // Begin node tracking for offset mapping
                self.begin_node(node_id.clone());

                // Map the start position of the heading (before any content)
                // This allows cursor to be placed at the very beginning
                let heading_start_char = self.last_char_offset;
                let mapping = OffsetMapping {
                    byte_range: range.start..range.start,
                    char_range: heading_start_char..heading_start_char,
                    node_id: node_id.clone(),
                    char_offset_in_node: 0,
                    child_index: Some(0), // position before first child
                    utf16_len: 0,
                };
                self.offset_maps.push(mapping);

                // Emit # syntax inside the heading tag
                if range.start < range.end {
                    let raw_text = &self.source[range.clone()];
                    let count = level as usize;
                    let pattern = "#".repeat(count);

                    // Find where the # actually starts (might have leading whitespace)
                    if let Some(hash_pos) = raw_text.find(&pattern) {
                        // Extract "# " or "## " etc
                        let syntax_end = (hash_pos + count + 1).min(raw_text.len());
                        let syntax = &raw_text[hash_pos..syntax_end];
                        let syntax_byte_start = range.start + hash_pos;

                        self.emit_inner_syntax(syntax, syntax_byte_start, SyntaxType::Block)?;
                    }
                }
                Ok(())
            }
            Tag::Table(alignments) => {
                if self.render_tables_as_markdown {
                    // Store start offset and skip HTML rendering
                    self.table_start_offset = Some(range.start);
                    self.in_non_writing_block = true; // Suppress content output
                    Ok(())
                } else {
                    self.emit_wrapper_start()?;
                    self.table_alignments = alignments;
                    self.write("<table>")
                }
            }
            Tag::TableHead => {
                if self.render_tables_as_markdown {
                    Ok(()) // Skip HTML rendering
                } else {
                    self.table_state = TableState::Head;
                    self.table_cell_index = 0;
                    self.write("<thead><tr>")
                }
            }
            Tag::TableRow => {
                if self.render_tables_as_markdown {
                    Ok(()) // Skip HTML rendering
                } else {
                    self.table_cell_index = 0;
                    self.write("<tr>")
                }
            }
            Tag::TableCell => {
                if self.render_tables_as_markdown {
                    Ok(()) // Skip HTML rendering
                } else {
                    match self.table_state {
                        TableState::Head => self.write("<th")?,
                        TableState::Body => self.write("<td")?,
                    }
                    match self.table_alignments.get(self.table_cell_index) {
                        Some(&Alignment::Left) => self.write(" style=\"text-align: left\">"),
                        Some(&Alignment::Center) => self.write(" style=\"text-align: center\">"),
                        Some(&Alignment::Right) => self.write(" style=\"text-align: right\">"),
                        _ => self.write(">"),
                    }
                }
            }
            Tag::BlockQuote(kind) => {
                self.emit_wrapper_start()?;

                let class_str = match kind {
                    None => "",
                    Some(BlockQuoteKind::Note) => " class=\"markdown-alert-note\"",
                    Some(BlockQuoteKind::Tip) => " class=\"markdown-alert-tip\"",
                    Some(BlockQuoteKind::Important) => " class=\"markdown-alert-important\"",
                    Some(BlockQuoteKind::Warning) => " class=\"markdown-alert-warning\"",
                    Some(BlockQuoteKind::Caution) => " class=\"markdown-alert-caution\"",
                };
                if self.end_newline {
                    write!(&mut self.writer, "<blockquote{}>\n", class_str)?;
                } else {
                    write!(&mut self.writer, "\n<blockquote{}>\n", class_str)?;
                }

                // Store range for emitting > inside the next paragraph
                self.pending_blockquote_range = Some(range);
                Ok(())
            }
            Tag::CodeBlock(info) => {
                self.emit_wrapper_start()?;

                // Track code block as paragraph-level block
                self.current_paragraph_start = Some((self.last_byte_offset, self.last_char_offset));

                if !self.end_newline {
                    self.write_newline()?;
                }

                // Generate node ID for code block
                let node_id = self.gen_node_id();

                match info {
                    CodeBlockKind::Fenced(info) => {
                        // Emit opening ```language and track both char and byte offsets
                        if range.start < range.end {
                            let raw_text = &self.source[range.clone()];
                            if let Some(fence_pos) = raw_text.find("```") {
                                let fence_end = (fence_pos + 3 + info.len()).min(raw_text.len());
                                let syntax = &raw_text[fence_pos..fence_end];
                                let syntax_char_len = syntax.chars().count() + 1; // +1 for newline
                                let syntax_byte_len = syntax.len() + 1; // +1 for newline

                                let syn_id = self.gen_syn_id();
                                let char_start = self.last_char_offset;
                                let char_end = char_start + syntax_char_len;

                                write!(
                                    &mut self.writer,
                                    "<span class=\"md-syntax-block\" data-syn-id=\"{}\" data-char-start=\"{}\" data-char-end=\"{}\" spellcheck=\"false\">",
                                    syn_id, char_start, char_end
                                )?;
                                escape_html(&mut self.writer, syntax)?;
                                self.write("</span>\n")?;

                                // Track opening span index for formatted_range update later
                                self.code_block_opening_span_idx = Some(self.syntax_spans.len());
                                self.code_block_char_start = Some(char_start);

                                self.syntax_spans.push(SyntaxSpanInfo {
                                    syn_id,
                                    char_range: char_start..char_end,
                                    syntax_type: SyntaxType::Block,
                                    formatted_range: None, // Will be set in TagEnd::CodeBlock
                                });

                                self.last_char_offset += syntax_char_len;
                                self.last_byte_offset = range.start + fence_pos + syntax_byte_len;
                            }
                        }

                        let lang = info.split(' ').next().unwrap();
                        let lang_opt = if lang.is_empty() {
                            None
                        } else {
                            Some(lang.to_string())
                        };
                        // Start buffering
                        self.code_buffer = Some((lang_opt, String::new()));

                        // Begin node tracking for offset mapping
                        self.begin_node(node_id);
                        Ok(())
                    }
                    CodeBlockKind::Indented => {
                        // Ignore indented code blocks (as per executive decision)
                        self.code_buffer = Some((None, String::new()));

                        // Begin node tracking for offset mapping
                        self.begin_node(node_id);
                        Ok(())
                    }
                }
            }
            Tag::List(Some(1)) => {
                self.emit_wrapper_start()?;
                // Track list as paragraph-level block
                self.current_paragraph_start = Some((self.last_byte_offset, self.last_char_offset));
                self.list_depth += 1;
                if self.end_newline {
                    self.write("<ol>\n")
                } else {
                    self.write("\n<ol>\n")
                }
            }
            Tag::List(Some(start)) => {
                self.emit_wrapper_start()?;
                // Track list as paragraph-level block
                self.current_paragraph_start = Some((self.last_byte_offset, self.last_char_offset));
                self.list_depth += 1;
                if self.end_newline {
                    self.write("<ol start=\"")?;
                } else {
                    self.write("\n<ol start=\"")?;
                }
                write!(&mut self.writer, "{}", start)?;
                self.write("\">\n")
            }
            Tag::List(None) => {
                self.emit_wrapper_start()?;
                // Track list as paragraph-level block
                self.current_paragraph_start = Some((self.last_byte_offset, self.last_char_offset));
                self.list_depth += 1;
                if self.end_newline {
                    self.write("<ul>\n")
                } else {
                    self.write("\n<ul>\n")
                }
            }
            Tag::Item => {
                // Generate node ID for list item
                let node_id = self.gen_node_id();

                if self.end_newline {
                    write!(&mut self.writer, "<li data-node-id=\"{}\">", node_id)?;
                } else {
                    write!(&mut self.writer, "\n<li data-node-id=\"{}\">", node_id)?;
                }

                // Begin node tracking
                self.begin_node(node_id);

                // Emit list marker syntax inside the <li> tag and track both offsets
                if range.start < range.end {
                    let raw_text = &self.source[range.clone()];

                    // Try to find the list marker (-, *, or digit.)
                    let trimmed = raw_text.trim_start();
                    let leading_ws_bytes = raw_text.len() - trimmed.len();
                    let leading_ws_chars = raw_text.chars().count() - trimmed.chars().count();

                    if let Some(marker) = trimmed.chars().next() {
                        if marker == '-' || marker == '*' {
                            // Unordered list: extract "- " or "* "
                            let marker_end = trimmed
                                .find(|c: char| c != '-' && c != '*')
                                .map(|pos| pos + 1)
                                .unwrap_or(1);
                            let syntax = &trimmed[..marker_end.min(trimmed.len())];
                            let char_start = self.last_char_offset;
                            let syntax_char_len = leading_ws_chars + syntax.chars().count();
                            let syntax_byte_len = leading_ws_bytes + syntax.len();
                            let char_end = char_start + syntax_char_len;

                            let syn_id = self.gen_syn_id();
                            write!(
                                &mut self.writer,
                                "<span class=\"md-syntax-block\" data-syn-id=\"{}\" data-char-start=\"{}\" data-char-end=\"{}\" spellcheck=\"false\">",
                                syn_id, char_start, char_end
                            )?;
                            escape_html(&mut self.writer, syntax)?;
                            self.write("</span>")?;

                            self.syntax_spans.push(SyntaxSpanInfo {
                                syn_id,
                                char_range: char_start..char_end,
                                syntax_type: SyntaxType::Block,
                                formatted_range: None,
                            });

                            // Record offset mapping for cursor positioning
                            self.record_mapping(
                                range.start..range.start + syntax_byte_len,
                                char_start..char_end,
                            );
                            self.last_char_offset = char_end;
                            self.last_byte_offset = range.start + syntax_byte_len;
                        } else if marker.is_ascii_digit() {
                            // Ordered list: extract "1. " or similar (including trailing space)
                            if let Some(dot_pos) = trimmed.find('.') {
                                let syntax_end = (dot_pos + 2).min(trimmed.len());
                                let syntax = &trimmed[..syntax_end];
                                let char_start = self.last_char_offset;
                                let syntax_char_len = leading_ws_chars + syntax.chars().count();
                                let syntax_byte_len = leading_ws_bytes + syntax.len();
                                let char_end = char_start + syntax_char_len;

                                let syn_id = self.gen_syn_id();
                                write!(
                                    &mut self.writer,
                                    "<span class=\"md-syntax-block\" data-syn-id=\"{}\" data-char-start=\"{}\" data-char-end=\"{}\" spellcheck=\"false\">",
                                    syn_id, char_start, char_end
                                )?;
                                escape_html(&mut self.writer, syntax)?;
                                self.write("</span>")?;

                                self.syntax_spans.push(SyntaxSpanInfo {
                                    syn_id,
                                    char_range: char_start..char_end,
                                    syntax_type: SyntaxType::Block,
                                    formatted_range: None,
                                });

                                // Record offset mapping for cursor positioning
                                self.record_mapping(
                                    range.start..range.start + syntax_byte_len,
                                    char_start..char_end,
                                );
                                self.last_char_offset = char_end;
                                self.last_byte_offset = range.start + syntax_byte_len;
                            }
                        }
                    }
                }
                Ok(())
            }
            Tag::DefinitionList => {
                self.emit_wrapper_start()?;
                if self.end_newline {
                    self.write("<dl>\n")
                } else {
                    self.write("\n<dl>\n")
                }
            }
            Tag::DefinitionListTitle => {
                let node_id = self.gen_node_id();

                if self.end_newline {
                    write!(&mut self.writer, "<dt data-node-id=\"{}\">", node_id)?;
                } else {
                    write!(&mut self.writer, "\n<dt data-node-id=\"{}\">", node_id)?;
                }

                self.begin_node(node_id);
                Ok(())
            }
            Tag::DefinitionListDefinition => {
                let node_id = self.gen_node_id();

                if self.end_newline {
                    write!(&mut self.writer, "<dd data-node-id=\"{}\">", node_id)?;
                } else {
                    write!(&mut self.writer, "\n<dd data-node-id=\"{}\">", node_id)?;
                }

                self.begin_node(node_id);
                Ok(())
            }
            Tag::Subscript => self.write("<sub>"),
            Tag::Superscript => self.write("<sup>"),
            Tag::Emphasis => self.write("<em>"),
            Tag::Strong => self.write("<strong>"),
            Tag::Strikethrough => self.write("<s>"),
            Tag::Link {
                link_type: LinkType::Email,
                dest_url,
                title,
                ..
            } => {
                self.write("<a href=\"mailto:")?;
                escape_href(&mut self.writer, &dest_url)?;
                if !title.is_empty() {
                    self.write("\" title=\"")?;
                    escape_html(&mut self.writer, &title)?;
                }
                self.write("\">")
            }
            Tag::Link {
                link_type,
                dest_url,
                title,
                ..
            } => {
                // Collect refs for later resolution
                let url = dest_url.as_ref();
                if matches!(link_type, LinkType::WikiLink { .. }) {
                    let (target, fragment) = weaver_common::EntryIndex::parse_wikilink(url);
                    self.ref_collector.add_wikilink(target, fragment, None);
                } else if url.starts_with("at://") {
                    self.ref_collector.add_at_link(url);
                }

                // Determine link validity class for wikilinks
                let validity_class = if matches!(link_type, LinkType::WikiLink { .. }) {
                    if let Some(index) = &self.entry_index {
                        if index.resolve(dest_url.as_ref()).is_some() {
                            " link-valid"
                        } else {
                            " link-broken"
                        }
                    } else {
                        ""
                    }
                } else {
                    ""
                };

                self.write("<a class=\"link")?;
                self.write(validity_class)?;
                self.write("\" href=\"")?;
                escape_href(&mut self.writer, &dest_url)?;
                if !title.is_empty() {
                    self.write("\" title=\"")?;
                    escape_html(&mut self.writer, &title)?;
                }
                self.write("\">")
            }
            Tag::Image {
                link_type,
                dest_url,
                title,
                id,
                attrs,
            } => {
                // Check if this is actually an AT embed disguised as a wikilink image
                // (markdown-weaver parses ![[at://...]] as Image with WikiLink link_type)
                let url = dest_url.as_ref();
                if matches!(link_type, LinkType::WikiLink { .. })
                    && (url.starts_with("at://") || url.starts_with("did:"))
                {
                    return self.write_embed(
                        range,
                        EmbedType::Other, // AT embeds - disambiguated via NSID later
                        dest_url,
                        title,
                        id,
                        attrs,
                    );
                }

                // Image rendering: all syntax elements share one syn_id for visibility toggling
                // Structure: ![  alt text  ](url)  <img>  cursor-landing
                let raw_text = &self.source[range.clone()];
                let syn_id = self.gen_syn_id();
                let opening_char_start = self.last_char_offset;

                // Find the alt text and closing syntax positions
                let paren_pos = raw_text.rfind("](").unwrap_or(raw_text.len());
                let alt_text = if raw_text.starts_with("![") && paren_pos > 2 {
                    &raw_text[2..paren_pos]
                } else {
                    ""
                };
                let closing_syntax = if paren_pos < raw_text.len() {
                    &raw_text[paren_pos..]
                } else {
                    ""
                };

                // Calculate char positions
                let alt_char_len = alt_text.chars().count();
                let closing_char_len = closing_syntax.chars().count();
                let opening_char_end = opening_char_start + 2; // "!["
                let alt_char_start = opening_char_end;
                let alt_char_end = alt_char_start + alt_char_len;
                let closing_char_start = alt_char_end;
                let closing_char_end = closing_char_start + closing_char_len;
                let formatted_range = opening_char_start..closing_char_end;

                // 1. Emit opening ![ syntax span
                if raw_text.starts_with("![") {
                    write!(
                        &mut self.writer,
                        "<span class=\"md-syntax-inline\" data-syn-id=\"{}\" data-char-start=\"{}\" data-char-end=\"{}\" spellcheck=\"false\">![</span>",
                        syn_id, opening_char_start, opening_char_end
                    )?;

                    self.syntax_spans.push(SyntaxSpanInfo {
                        syn_id: syn_id.clone(),
                        char_range: opening_char_start..opening_char_end,
                        syntax_type: SyntaxType::Inline,
                        formatted_range: Some(formatted_range.clone()),
                    });

                    // Record offset mapping for ![
                    self.record_mapping(
                        range.start..range.start + 2,
                        opening_char_start..opening_char_end,
                    );
                }

                // 2. Emit alt text span (same syn_id, editable when visible)
                if !alt_text.is_empty() {
                    write!(
                        &mut self.writer,
                        "<span class=\"image-alt\" data-syn-id=\"{}\" data-char-start=\"{}\" data-char-end=\"{}\">",
                        syn_id, alt_char_start, alt_char_end
                    )?;
                    escape_html(&mut self.writer, alt_text)?;
                    self.write("</span>")?;

                    self.syntax_spans.push(SyntaxSpanInfo {
                        syn_id: syn_id.clone(),
                        char_range: alt_char_start..alt_char_end,
                        syntax_type: SyntaxType::Inline,
                        formatted_range: Some(formatted_range.clone()),
                    });

                    // Record offset mapping for alt text
                    self.record_mapping(
                        range.start + 2..range.start + 2 + alt_text.len(),
                        alt_char_start..alt_char_end,
                    );
                }

                // 3. Emit closing ](url) syntax span
                if !closing_syntax.is_empty() {
                    write!(
                        &mut self.writer,
                        "<span class=\"md-syntax-inline\" data-syn-id=\"{}\" data-char-start=\"{}\" data-char-end=\"{}\" spellcheck=\"false\">",
                        syn_id, closing_char_start, closing_char_end
                    )?;
                    escape_html(&mut self.writer, closing_syntax)?;
                    self.write("</span>")?;

                    self.syntax_spans.push(SyntaxSpanInfo {
                        syn_id: syn_id.clone(),
                        char_range: closing_char_start..closing_char_end,
                        syntax_type: SyntaxType::Inline,
                        formatted_range: Some(formatted_range.clone()),
                    });

                    // Record offset mapping for ](url)
                    self.record_mapping(
                        range.start + paren_pos..range.end,
                        closing_char_start..closing_char_end,
                    );
                }

                // 4. Emit <img> element (no syn_id - always visible)
                self.write("<img src=\"")?;
                let resolved_url = self
                    .image_resolver
                    .as_ref()
                    .and_then(|r| r.resolve_image_url(&dest_url));
                if let Some(ref cdn_url) = resolved_url {
                    escape_href(&mut self.writer, cdn_url)?;
                } else {
                    escape_href(&mut self.writer, &dest_url)?;
                }
                self.write("\" alt=\"")?;
                escape_html(&mut self.writer, alt_text)?;
                self.write("\"")?;
                if !title.is_empty() {
                    self.write(" title=\"")?;
                    escape_html(&mut self.writer, &title)?;
                    self.write("\"")?;
                }
                if let Some(attrs) = attrs {
                    if !attrs.classes.is_empty() {
                        self.write(" class=\"")?;
                        for (i, class) in attrs.classes.iter().enumerate() {
                            if i > 0 {
                                self.write(" ")?;
                            }
                            escape_html(&mut self.writer, class)?;
                        }
                        self.write("\"")?;
                    }
                    for (attr, value) in &attrs.attrs {
                        self.write(" ")?;
                        escape_html(&mut self.writer, attr)?;
                        self.write("=\"")?;
                        escape_html(&mut self.writer, value)?;
                        self.write("\"")?;
                    }
                }
                self.write(" />")?;

                // Consume the text events for alt (they're still in the iterator)
                // Use consume_until_end() since we already wrote alt text from source
                self.consume_until_end();

                // Update offsets
                self.last_char_offset = closing_char_end;
                self.last_byte_offset = range.end;

                Ok(())
            }
            Tag::Embed {
                embed_type,
                dest_url,
                title,
                id,
                attrs,
            } => self.write_embed(range, embed_type, dest_url, title, id, attrs),
            Tag::WeaverBlock(_, attrs) => {
                self.in_non_writing_block = true;
                self.weaver_block_buffer.clear();
                self.weaver_block_char_start = Some(self.last_char_offset);
                // Store attrs from Start tag, will merge with parsed text on End
                if !attrs.classes.is_empty() || !attrs.attrs.is_empty() {
                    self.pending_block_attrs = Some(attrs.into_static());
                }
                Ok(())
            }
            Tag::FootnoteDefinition(name) => {
                // Emit the [^name]: prefix as a hideable syntax span
                // The source should have "[^name]: " at the start
                let prefix = format!("[^{}]: ", name);
                let char_start = self.last_char_offset;
                let prefix_char_len = prefix.chars().count();
                let char_end = char_start + prefix_char_len;
                let syn_id = self.gen_syn_id();

                if !self.end_newline {
                    self.write("\n")?;
                }

                write!(
                    &mut self.writer,
                    "<span class=\"md-syntax-block\" data-syn-id=\"{}\" data-char-start=\"{}\" data-char-end=\"{}\" spellcheck=\"false\">",
                    syn_id, char_start, char_end
                )?;
                escape_html(&mut self.writer, &prefix)?;
                self.write("</span>")?;

                // Track this span for linking with the footnote reference
                let def_span_index = self.syntax_spans.len();
                self.syntax_spans.push(SyntaxSpanInfo {
                    syn_id,
                    char_range: char_start..char_end,
                    syntax_type: SyntaxType::Block,
                    formatted_range: None, // Set at FootnoteDefinition end
                });

                // Store the definition info for linking at end
                self.current_footnote_def = Some((name.to_string(), def_span_index, char_start));

                // Record offset mapping for the syntax span
                self.record_mapping(range.start..range.start + prefix.len(), char_start..char_end);

                // Update tracking for the prefix
                self.last_char_offset = char_end;
                self.last_byte_offset = range.start + prefix.len();

                // Emit the definition container
                write!(
                    &mut self.writer,
                    "<div class=\"footnote-definition\" id=\"fn-{}\">",
                    name
                )?;

                // Get/create footnote number for the label
                let len = self.numbers.len() + 1;
                let number = *self.numbers.entry(name.to_string()).or_insert(len);
                write!(
                    &mut self.writer,
                    "<sup class=\"footnote-definition-label\">{}</sup>",
                    number
                )?;

                Ok(())
            }
            Tag::MetadataBlock(_) => {
                self.in_non_writing_block = true;
                Ok(())
            }
        }
    }

    fn end_tag(
        &mut self,
        tag: markdown_weaver::TagEnd,
        range: Range<usize>,
    ) -> Result<(), fmt::Error> {
        use markdown_weaver::TagEnd;

        // Emit tag HTML first
        let result = match tag {
            TagEnd::HtmlBlock => {
                // Capture paragraph boundary info BEFORE writing closing HTML
                // Skip if inside a list - list owns the paragraph boundary
                let para_boundary = if self.list_depth == 0 {
                    self.current_paragraph_start
                        .take()
                        .map(|(byte_start, char_start)| {
                            (
                                byte_start..self.last_byte_offset,
                                char_start..self.last_char_offset,
                            )
                        })
                } else {
                    None
                };

                // Write closing HTML to current segment
                self.end_node();
                self.write("</p>\n")?;

                // Now finalize paragraph (starts new segment)
                if let Some((byte_range, char_range)) = para_boundary {
                    self.finalize_paragraph(byte_range, char_range);
                }
                Ok(())
            }
            TagEnd::Paragraph(_) => {
                // Capture paragraph boundary info BEFORE writing closing HTML
                // Skip if inside a list - list owns the paragraph boundary
                let para_boundary = if self.list_depth == 0 {
                    self.current_paragraph_start
                        .take()
                        .map(|(byte_start, char_start)| {
                            (
                                byte_start..self.last_byte_offset,
                                char_start..self.last_char_offset,
                            )
                        })
                } else {
                    None
                };

                // Write closing HTML to current segment
                self.end_node();
                self.write("</p>\n")?;
                self.close_wrapper()?;

                // Now finalize paragraph (starts new segment)
                if let Some((byte_range, char_range)) = para_boundary {
                    self.finalize_paragraph(byte_range, char_range);
                }
                Ok(())
            }
            TagEnd::Heading(level) => {
                // Capture paragraph boundary info BEFORE writing closing HTML
                let para_boundary =
                    self.current_paragraph_start
                        .take()
                        .map(|(byte_start, char_start)| {
                            (
                                byte_start..self.last_byte_offset,
                                char_start..self.last_char_offset,
                            )
                        });

                // Write closing HTML to current segment
                self.end_node();
                self.write("</")?;
                write!(&mut self.writer, "{}", level)?;
                self.write(">\n")?;
                // Note: Don't close wrapper here - headings typically go with following block

                // Now finalize paragraph (starts new segment)
                if let Some((byte_range, char_range)) = para_boundary {
                    self.finalize_paragraph(byte_range, char_range);
                }
                Ok(())
            }
            TagEnd::Table => {
                if self.render_tables_as_markdown {
                    // Emit the raw markdown table
                    if let Some(start) = self.table_start_offset.take() {
                        let table_text = &self.source[start..range.end];
                        self.in_non_writing_block = false;

                        // Wrap in a pre or div for styling
                        self.write("<pre class=\"table-markdown\">")?;
                        escape_html(&mut self.writer, table_text)?;
                        self.write("</pre>\n")?;
                    }
                    Ok(())
                } else {
                    self.write("</tbody></table>\n")
                }
            }
            TagEnd::TableHead => {
                if self.render_tables_as_markdown {
                    Ok(()) // Skip HTML rendering
                } else {
                    self.write("</tr></thead><tbody>\n")?;
                    self.table_state = TableState::Body;
                    Ok(())
                }
            }
            TagEnd::TableRow => {
                if self.render_tables_as_markdown {
                    Ok(()) // Skip HTML rendering
                } else {
                    self.write("</tr>\n")
                }
            }
            TagEnd::TableCell => {
                if self.render_tables_as_markdown {
                    Ok(()) // Skip HTML rendering
                } else {
                    match self.table_state {
                        TableState::Head => self.write("</th>")?,
                        TableState::Body => self.write("</td>")?,
                    }
                    self.table_cell_index += 1;
                    Ok(())
                }
            }
            TagEnd::BlockQuote(_) => {
                // If pending_blockquote_range is still set, the blockquote was empty
                // (no paragraph inside). Emit the > as its own minimal paragraph.
                let mut para_boundary = None;
                if let Some(bq_range) = self.pending_blockquote_range.take() {
                    if bq_range.start < bq_range.end {
                        let raw_text = &self.source[bq_range.clone()];
                        if let Some(gt_pos) = raw_text.find('>') {
                            let para_byte_start = bq_range.start + gt_pos;
                            let para_char_start = self.last_char_offset;

                            // Create a minimal paragraph for the empty blockquote
                            let node_id = self.gen_node_id();
                            write!(&mut self.writer, "<div id=\"{}\"", node_id)?;

                            // Record start-of-node mapping for cursor positioning
                            self.offset_maps.push(OffsetMapping {
                                byte_range: para_byte_start..para_byte_start,
                                char_range: para_char_start..para_char_start,
                                node_id: node_id.clone(),
                                char_offset_in_node: gt_pos,
                                child_index: Some(0),
                                utf16_len: 0,
                            });

                            // Emit the > as block syntax
                            let syntax = &raw_text[gt_pos..gt_pos + 1];
                            self.emit_inner_syntax(syntax, para_byte_start, SyntaxType::Block)?;

                            self.write("</div>\n")?;
                            self.end_node();

                            // Capture paragraph boundary for later finalization
                            let byte_range = para_byte_start..bq_range.end;
                            let char_range = para_char_start..self.last_char_offset;
                            para_boundary = Some((byte_range, char_range));
                        }
                    }
                }
                self.write("</blockquote>\n")?;
                self.close_wrapper()?;

                // Now finalize paragraph if we had one
                if let Some((byte_range, char_range)) = para_boundary {
                    self.finalize_paragraph(byte_range, char_range);
                }
                Ok(())
            }
            TagEnd::CodeBlock => {
                use std::sync::LazyLock;
                use syntect::parsing::SyntaxSet;
                static SYNTAX_SET: LazyLock<SyntaxSet> =
                    LazyLock::new(|| SyntaxSet::load_defaults_newlines());

                if let Some((lang, buffer)) = self.code_buffer.take() {
                    // Create offset mapping for code block content if we tracked ranges
                    if let (Some(code_byte_range), Some(code_char_range)) = (
                        self.code_buffer_byte_range.take(),
                        self.code_buffer_char_range.take(),
                    ) {
                        // Record mapping before writing HTML
                        // (current_node_id should be set by start_tag for CodeBlock)
                        self.record_mapping(code_byte_range, code_char_range);
                    }

                    // Get node_id for data-node-id attribute (needed for cursor positioning)
                    let node_id = self.current_node_id.clone();

                    if let Some(ref lang_str) = lang {
                        // Use a temporary String buffer for syntect
                        let mut temp_output = String::new();
                        match weaver_renderer::code_pretty::highlight(
                            &SYNTAX_SET,
                            Some(lang_str),
                            &buffer,
                            &mut temp_output,
                        ) {
                            Ok(_) => {
                                // Inject data-node-id into the <pre> tag for cursor positioning
                                if let Some(ref nid) = node_id {
                                    let injected = temp_output.replacen(
                                        "<pre>",
                                        &format!("<pre data-node-id=\"{}\">", nid),
                                        1,
                                    );
                                    self.write(&injected)?;
                                } else {
                                    self.write(&temp_output)?;
                                }
                            }
                            Err(_) => {
                                // Fallback to plain code block
                                if let Some(ref nid) = node_id {
                                    write!(
                                        &mut self.writer,
                                        "<pre data-node-id=\"{}\"><code class=\"language-",
                                        nid
                                    )?;
                                } else {
                                    self.write("<pre><code class=\"language-")?;
                                }
                                escape_html(&mut self.writer, lang_str)?;
                                self.write("\">")?;
                                escape_html_body_text(&mut self.writer, &buffer)?;
                                self.write("</code></pre>\n")?;
                            }
                        }
                    } else {
                        if let Some(ref nid) = node_id {
                            write!(&mut self.writer, "<pre data-node-id=\"{}\"><code>", nid)?;
                        } else {
                            self.write("<pre><code>")?;
                        }
                        escape_html_body_text(&mut self.writer, &buffer)?;
                        self.write("</code></pre>\n")?;
                    }

                    // End node tracking
                    self.end_node();
                } else {
                    self.write("</code></pre>\n")?;
                }

                // Emit closing ``` (emit_gap_before is skipped while buffering)
                // Track the opening span index and char start before we potentially clear them
                let opening_span_idx = self.code_block_opening_span_idx.take();
                let code_block_start = self.code_block_char_start.take();

                if range.start < range.end {
                    let raw_text = &self.source[range.clone()];
                    if let Some(fence_line) = raw_text.lines().last() {
                        if fence_line.trim().starts_with("```") {
                            let fence = fence_line.trim();
                            let fence_char_len = fence.chars().count();

                            let syn_id = self.gen_syn_id();
                            let char_start = self.last_char_offset;
                            let char_end = char_start + fence_char_len;

                            write!(
                                &mut self.writer,
                                "<span class=\"md-syntax-block\" data-syn-id=\"{}\" data-char-start=\"{}\" data-char-end=\"{}\" spellcheck=\"false\">",
                                syn_id, char_start, char_end
                            )?;
                            escape_html(&mut self.writer, fence)?;
                            self.write("</span>")?;

                            self.last_char_offset += fence_char_len;
                            self.last_byte_offset += fence.len();

                            // Compute formatted_range for entire code block (opening fence to closing fence)
                            let formatted_range =
                                code_block_start.map(|start| start..self.last_char_offset);

                            // Update opening fence span with formatted_range
                            if let (Some(idx), Some(fr)) =
                                (opening_span_idx, formatted_range.as_ref())
                            {
                                if let Some(span) = self.syntax_spans.get_mut(idx) {
                                    span.formatted_range = Some(fr.clone());
                                }
                            }

                            // Push closing fence span with formatted_range
                            self.syntax_spans.push(SyntaxSpanInfo {
                                syn_id,
                                char_range: char_start..char_end,
                                syntax_type: SyntaxType::Block,
                                formatted_range,
                            });
                        }
                    }
                }

                // Finalize code block paragraph
                if let Some((byte_start, char_start)) = self.current_paragraph_start.take() {
                    let byte_range = byte_start..self.last_byte_offset;
                    let char_range = char_start..self.last_char_offset;
                    self.finalize_paragraph(byte_range, char_range);
                }

                Ok(())
            }
            TagEnd::List(true) => {
                self.list_depth = self.list_depth.saturating_sub(1);
                // Capture paragraph boundary BEFORE writing closing HTML
                let para_boundary =
                    self.current_paragraph_start
                        .take()
                        .map(|(byte_start, char_start)| {
                            (
                                byte_start..self.last_byte_offset,
                                char_start..self.last_char_offset,
                            )
                        });

                self.write("</ol>\n")?;
                self.close_wrapper()?;

                // Finalize paragraph after closing HTML
                if let Some((byte_range, char_range)) = para_boundary {
                    self.finalize_paragraph(byte_range, char_range);
                }
                Ok(())
            }
            TagEnd::List(false) => {
                self.list_depth = self.list_depth.saturating_sub(1);
                // Capture paragraph boundary BEFORE writing closing HTML
                let para_boundary =
                    self.current_paragraph_start
                        .take()
                        .map(|(byte_start, char_start)| {
                            (
                                byte_start..self.last_byte_offset,
                                char_start..self.last_char_offset,
                            )
                        });

                self.write("</ul>\n")?;
                self.close_wrapper()?;

                // Finalize paragraph after closing HTML
                if let Some((byte_range, char_range)) = para_boundary {
                    self.finalize_paragraph(byte_range, char_range);
                }
                Ok(())
            }
            TagEnd::Item => {
                self.end_node();
                self.write("</li>\n")
            }
            TagEnd::DefinitionList => {
                self.write("</dl>\n")?;
                self.close_wrapper()
            }
            TagEnd::DefinitionListTitle => {
                self.end_node();
                self.write("</dt>\n")
            }
            TagEnd::DefinitionListDefinition => {
                self.end_node();
                self.write("</dd>\n")
            }
            TagEnd::Emphasis => {
                // Write closing tag FIRST, then emit closing syntax OUTSIDE the tag
                self.write("</em>")?;
                self.emit_gap_before(range.end)?;
                self.finalize_paired_inline_format();
                Ok(())
            }
            TagEnd::Superscript => self.write("</sup>"),
            TagEnd::Subscript => self.write("</sub>"),
            TagEnd::Strong => {
                // Write closing tag FIRST, then emit closing syntax OUTSIDE the tag
                self.write("</strong>")?;
                self.emit_gap_before(range.end)?;
                self.finalize_paired_inline_format();
                Ok(())
            }
            TagEnd::Strikethrough => {
                // Write closing tag FIRST, then emit closing syntax OUTSIDE the tag
                self.write("</s>")?;
                self.emit_gap_before(range.end)?;
                self.finalize_paired_inline_format();
                Ok(())
            }
            TagEnd::Link => {
                self.write("</a>")?;
                // Check if this is a wiki link (ends with ]]) vs regular link (ends with ))
                let raw_text = &self.source[range.clone()];
                if raw_text.ends_with("]]") {
                    // WikiLink: emit ]] as closing syntax
                    let syn_id = self.gen_syn_id();
                    let char_start = self.last_char_offset;
                    let char_end = char_start + 2;

                    write!(
                        &mut self.writer,
                        "<span class=\"md-syntax-inline\" data-syn-id=\"{}\" data-char-start=\"{}\" data-char-end=\"{}\" spellcheck=\"false\">]]</span>",
                        syn_id, char_start, char_end
                    )?;

                    self.syntax_spans.push(SyntaxSpanInfo {
                        syn_id,
                        char_range: char_start..char_end,
                        syntax_type: SyntaxType::Inline,
                        formatted_range: None, // Will be set by finalize
                    });

                    self.last_char_offset = char_end;
                    self.last_byte_offset = range.end;
                } else {
                    self.emit_gap_before(range.end)?;
                }
                self.finalize_paired_inline_format();
                Ok(())
            }
            TagEnd::Image => Ok(()), // No-op: raw_text() already consumed the End(Image) event
            TagEnd::Embed => Ok(()),
            TagEnd::WeaverBlock(_) => {
                self.in_non_writing_block = false;

                // Emit the { content } as a hideable syntax span
                if let Some(char_start) = self.weaver_block_char_start.take() {
                    // Build the full syntax text: { buffered_content }
                    let syntax_text = format!("{{{}}}", self.weaver_block_buffer);
                    let syntax_char_len = syntax_text.chars().count();
                    let char_end = char_start + syntax_char_len;

                    let syn_id = self.gen_syn_id();

                    write!(
                        &mut self.writer,
                        "<span class=\"md-syntax-block\" data-syn-id=\"{}\" data-char-start=\"{}\" data-char-end=\"{}\" spellcheck=\"false\">",
                        syn_id, char_start, char_end
                    )?;
                    escape_html(&mut self.writer, &syntax_text)?;
                    self.write("</span>")?;

                    // Track the syntax span
                    self.syntax_spans.push(SyntaxSpanInfo {
                        syn_id,
                        char_range: char_start..char_end,
                        syntax_type: SyntaxType::Block,
                        formatted_range: None,
                    });

                    // Record offset mapping for the syntax span
                    self.record_mapping(range.clone(), char_start..char_end);

                    // Update tracking
                    self.last_char_offset = char_end;
                    self.last_byte_offset = range.end;
                }

                // Parse the buffered text for attrs and store for next block
                if !self.weaver_block_buffer.is_empty() {
                    let parsed = Self::parse_weaver_attrs(&self.weaver_block_buffer);
                    self.weaver_block_buffer.clear();
                    // Merge with any existing pending attrs or set new
                    if let Some(ref mut existing) = self.pending_block_attrs {
                        existing.classes.extend(parsed.classes);
                        existing.attrs.extend(parsed.attrs);
                    } else {
                        self.pending_block_attrs = Some(parsed);
                    }
                }

                Ok(())
            }
            TagEnd::FootnoteDefinition => {
                self.write("</div>\n")?;

                // Link the footnote definition span with its reference span
                if let Some((name, def_span_index, _def_char_start)) =
                    self.current_footnote_def.take()
                {
                    let def_char_end = self.last_char_offset;

                    // Look up the reference span
                    if let Some(&(ref_span_index, ref_char_start)) =
                        self.footnote_ref_spans.get(&name)
                    {
                        // Create formatted_range spanning from ref start to def end
                        let formatted_range = ref_char_start..def_char_end;

                        // Update both spans with the same formatted_range
                        // so they show/hide together based on cursor proximity
                        if let Some(ref_span) = self.syntax_spans.get_mut(ref_span_index) {
                            ref_span.formatted_range = Some(formatted_range.clone());
                        }
                        if let Some(def_span) = self.syntax_spans.get_mut(def_span_index) {
                            def_span.formatted_range = Some(formatted_range);
                        }
                    }
                }

                Ok(())
            }
            TagEnd::MetadataBlock(_) => {
                self.in_non_writing_block = false;
                Ok(())
            }
        };

        result?;

        // Note: Closing syntax for inline formatting tags (Strong, Emphasis, Strikethrough)
        // is handled INSIDE their respective match arms above, AFTER writing the closing HTML.
        // This ensures the closing syntax span appears OUTSIDE the formatted element.
        // Other End events have their closing syntax emitted by emit_gap_before() in the main loop.

        Ok(())
    }
}

impl<'a, I: Iterator<Item = (Event<'a>, Range<usize>)>, E: EmbedContentProvider, R: ImageResolver>
    EditorWriter<'a, I, E, R>
{
    fn write_embed(
        &mut self,
        range: Range<usize>,
        embed_type: EmbedType,
        dest_url: CowStr<'_>,
        title: CowStr<'_>,
        id: CowStr<'_>,
        attrs: Option<markdown_weaver::WeaverAttributes<'_>>,
    ) -> Result<(), fmt::Error> {
        // Embed rendering: all syntax elements share one syn_id for visibility toggling
        // Structure: ![[  url-as-link  ]]  <embed-content>
        let raw_text = &self.source[range.clone()];
        let syn_id = self.gen_syn_id();
        let opening_char_start = self.last_char_offset;

        // Extract the URL from raw text (between ![[ and ]])
        let url_text = if raw_text.starts_with("![[") && raw_text.ends_with("]]") {
            &raw_text[3..raw_text.len() - 2]
        } else {
            dest_url.as_ref()
        };

        // Calculate char positions
        let url_char_len = url_text.chars().count();
        let opening_char_end = opening_char_start + 3; // "![["
        let url_char_start = opening_char_end;
        let url_char_end = url_char_start + url_char_len;
        let closing_char_start = url_char_end;
        let closing_char_end = closing_char_start + 2; // "]]"
        let formatted_range = opening_char_start..closing_char_end;

        // 1. Emit opening ![[ syntax span
        if raw_text.starts_with("![[") {
            write!(
                &mut self.writer,
                "<span class=\"md-syntax-inline\" data-syn-id=\"{}\" data-char-start=\"{}\" data-char-end=\"{}\" spellcheck=\"false\">![[</span>",
                syn_id, opening_char_start, opening_char_end
            )?;

            self.syntax_spans.push(SyntaxSpanInfo {
                syn_id: syn_id.clone(),
                char_range: opening_char_start..opening_char_end,
                syntax_type: SyntaxType::Inline,
                formatted_range: Some(formatted_range.clone()),
            });

            self.record_mapping(
                range.start..range.start + 3,
                opening_char_start..opening_char_end,
            );
        }

        // 2. Emit URL as a clickable link (same syn_id, shown/hidden with syntax)
        let url = dest_url.as_ref();
        let link_href = if url.starts_with("at://") {
            format!("https://alpha.weaver.sh/record/{}", url)
        } else {
            url.to_string()
        };

        write!(
            &mut self.writer,
            "<a class=\"image-alt embed-url\" href=\"{}\" data-syn-id=\"{}\" data-char-start=\"{}\" data-char-end=\"{}\" target=\"_blank\">",
            link_href, syn_id, url_char_start, url_char_end
        )?;
        escape_html(&mut self.writer, url_text)?;
        self.write("</a>")?;

        self.syntax_spans.push(SyntaxSpanInfo {
            syn_id: syn_id.clone(),
            char_range: url_char_start..url_char_end,
            syntax_type: SyntaxType::Inline,
            formatted_range: Some(formatted_range.clone()),
        });

        self.record_mapping(range.start + 3..range.end - 2, url_char_start..url_char_end);

        // 3. Emit closing ]] syntax span
        if raw_text.ends_with("]]") {
            write!(
                &mut self.writer,
                "<span class=\"md-syntax-inline\" data-syn-id=\"{}\" data-char-start=\"{}\" data-char-end=\"{}\" spellcheck=\"false\">]]</span>",
                syn_id, closing_char_start, closing_char_end
            )?;

            self.syntax_spans.push(SyntaxSpanInfo {
                syn_id: syn_id.clone(),
                char_range: closing_char_start..closing_char_end,
                syntax_type: SyntaxType::Inline,
                formatted_range: Some(formatted_range.clone()),
            });

            self.record_mapping(
                range.end - 2..range.end,
                closing_char_start..closing_char_end,
            );
        }

        // Collect AT URI for later resolution
        if url.starts_with("at://") || url.starts_with("did:") {
            self.ref_collector.add_at_embed(
                url,
                if title.is_empty() {
                    None
                } else {
                    Some(title.as_ref())
                },
            );
        }

        // 4. Emit the actual embed content
        // Try to get content from attributes first
        let content_from_attrs = if let Some(ref attrs) = attrs {
            attrs
                .attrs
                .iter()
                .find(|(k, _)| k.as_ref() == "content")
                .map(|(_, v)| v.as_ref().to_string())
        } else {
            None
        };

        // If no content in attrs, try provider
        let content = if let Some(content) = content_from_attrs {
            Some(content)
        } else if let Some(ref provider) = self.embed_provider {
            let tag = Tag::Embed {
                embed_type,
                dest_url: dest_url.clone(),
                title: title.clone(),
                id: id.clone(),
                attrs: attrs.clone(),
            };
            provider.get_embed_content(&tag)
        } else {
            None
        };

        if let Some(html_content) = content {
            // Write the pre-rendered content directly
            self.write(&html_content)?;
        } else {
            // Fallback: render as placeholder div (iframe doesn't make sense for at:// URIs)
            self.write("<div class=\"atproto-embed atproto-embed-placeholder\">")?;
            self.write("<span class=\"embed-loading\">Loading embed...</span>")?;
            self.write("</div>")?;
        }

        // Consume the text events for the URL (they're still in the iterator)
        // Use consume_until_end() since we already wrote the URL from source
        self.consume_until_end();

        // Update offsets
        self.last_char_offset = closing_char_end;
        self.last_byte_offset = range.end;

        Ok(())
    }
}
