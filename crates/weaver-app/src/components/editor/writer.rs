//! HTML writer for markdown editor with visible formatting characters.
//!
//! Based on ClientWriter from weaver-renderer, but modified to preserve
//! formatting characters (**, *, #, etc) wrapped in styled spans.
//!
//! Uses Parser::into_offset_iter() to track gaps between events, which
//! represent consumed formatting characters.

use super::offset_map::{OffsetMapping, RenderResult};
use loro::LoroText;
use markdown_weaver::{
    Alignment, BlockQuoteKind, CodeBlockKind, CowStr, EmbedType, Event, LinkType, Tag,
};
use markdown_weaver_escape::{
    StrWrite, escape_href, escape_html, escape_html_body_text,
    escape_html_body_text_with_char_count,
};
use std::collections::HashMap;
use std::ops::Range;

/// Result of rendering with the EditorWriter.
#[derive(Debug, Clone)]
pub struct WriterResult {
    /// Offset mappings from source to DOM positions
    pub offset_maps: Vec<OffsetMapping>,

    /// Paragraph boundaries in source: (byte_range, char_range)
    /// These are extracted during rendering by tracking Tag::Paragraph events
    pub paragraph_ranges: Vec<(Range<usize>, Range<usize>)>,

    /// Syntax spans that can be conditionally hidden
    pub syntax_spans: Vec<SyntaxSpanInfo>,
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
/// Supports two states for images:
/// - Pending: uses data URL for immediate preview while upload is in progress
/// - Uploaded: uses CDN URL format `https://cdn.bsky.app/img/feed_fullsize/plain/{did}/{cid}@{format}`
///
/// Image URLs in markdown use the format `/image/{name}`.
#[derive(Clone, Default)]
pub struct EditorImageResolver {
    /// Pending images: name -> data URL (still uploading)
    pending: std::collections::HashMap<String, String>,
    /// Uploaded images: name -> (CID string, DID string, format)
    uploaded: std::collections::HashMap<String, (String, String, String)>,
}

impl EditorImageResolver {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a pending image with a data URL for immediate preview.
    ///
    /// # Arguments
    /// * `name` - The image name used in markdown (e.g., "photo.jpg")
    /// * `data_url` - The base64 data URL for preview
    pub fn add_pending(&mut self, name: String, data_url: String) {
        self.pending.insert(name, data_url);
    }

    /// Promote a pending image to uploaded status.
    ///
    /// Removes from pending and adds to uploaded with CDN info.
    pub fn promote_to_uploaded(&mut self, name: &str, cid: String, did: String, format: String) {
        self.pending.remove(name);
        self.uploaded.insert(name.to_string(), (cid, did, format));
    }

    /// Add an already-uploaded image.
    ///
    /// # Arguments
    /// * `name` - The name/URL used in markdown (e.g., "photo.jpg")
    /// * `cid` - The blob CID
    /// * `did` - The DID of the blob owner
    /// * `format` - The image format (e.g., "jpeg", "png")
    pub fn add_uploaded(&mut self, name: String, cid: String, did: String, format: String) {
        self.uploaded.insert(name, (cid, did, format));
    }

    /// Check if an image is pending upload.
    pub fn is_pending(&self, name: &str) -> bool {
        self.pending.contains_key(name)
    }

    /// Build a resolver from editor images and user DID.
    pub fn from_images<'a>(
        images: impl IntoIterator<Item = &'a super::document::EditorImage>,
        user_did: &str,
    ) -> Self {
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

            // Get CID and format from the blob ref
            let blob = editor_image.image.image.blob();
            let cid = blob.cid().to_string();
            let format = blob
                .mime_type
                .0
                .strip_prefix("image/")
                .unwrap_or("jpeg")
                .to_string();

            resolver.add_uploaded(name, cid, user_did.to_string(), format);
        }
        resolver
    }
}

impl ImageResolver for EditorImageResolver {
    fn resolve_image_url(&self, url: &str) -> Option<String> {
        // Extract image name from /image/{name} format
        let name = url.strip_prefix("/image/").unwrap_or(url);

        // Check pending first (data URL for immediate preview)
        if let Some(data_url) = self.pending.get(name) {
            return Some(data_url.clone());
        }

        // Then check uploaded (CDN URL)
        let (cid, did, format) = self.uploaded.get(name)?;
        Some(format!(
            "https://cdn.bsky.app/img/feed_fullsize/plain/{}/{}@{}",
            did, cid, format
        ))
    }
}

impl ImageResolver for &EditorImageResolver {
    fn resolve_image_url(&self, url: &str) -> Option<String> {
        (*self).resolve_image_url(url)
    }
}

/// HTML writer that preserves markdown formatting characters.
///
/// This writer processes offset-iter events to detect gaps (consumed formatting)
/// and emits them as styled spans for visibility in the editor.
pub struct EditorWriter<'a, I: Iterator<Item = (Event<'a>, Range<usize>)>, W: StrWrite, E = (), R = ()> {
    source: &'a str,
    source_text: &'a LoroText,
    events: I,
    writer: W,
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

    code_buffer: Option<(Option<String>, String)>, // (lang, content)
    code_buffer_byte_range: Option<Range<usize>>,  // byte range of buffered code content
    code_buffer_char_range: Option<Range<usize>>,  // char range of buffered code content
    pending_blockquote_range: Option<Range<usize>>, // range for emitting > inside next paragraph

    // Table rendering mode
    render_tables_as_markdown: bool,
    table_start_offset: Option<usize>, // track start of table for markdown rendering

    // Offset mapping tracking
    offset_maps: Vec<OffsetMapping>,
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

    /// When true, skip HTML generation and only track paragraph boundaries.
    /// Used for fast boundary discovery in incremental rendering.
    boundary_only: bool,

    // Syntax span tracking for conditional visibility
    syntax_spans: Vec<SyntaxSpanInfo>,
    next_syn_id: usize,
    /// Stack of pending inline formats: (syn_id of opening span, char start of region)
    /// Used to set formatted_range when closing paired inline markers
    pending_inline_formats: Vec<(String, usize)>,

    _phantom: std::marker::PhantomData<&'a ()>,
}

#[derive(Debug, Clone, Copy)]
enum TableState {
    Head,
    Body,
}

impl<
        'a,
        I: Iterator<Item = (Event<'a>, Range<usize>)>,
        W: StrWrite,
        E: EmbedContentProvider,
        R: ImageResolver,
    > EditorWriter<'a, I, W, E, R>
{
    pub fn new(source: &'a str, source_text: &'a LoroText, events: I, writer: W) -> Self {
        Self::new_with_node_offset(source, source_text, events, writer, 0)
    }

    pub fn new_with_node_offset(
        source: &'a str,
        source_text: &'a LoroText,
        events: I,
        writer: W,
        node_id_offset: usize,
    ) -> Self {
        Self::new_with_offsets(source, source_text, events, writer, node_id_offset, 0)
    }

    pub fn new_with_offsets(
        source: &'a str,
        source_text: &'a LoroText,
        events: I,
        writer: W,
        node_id_offset: usize,
        syn_id_offset: usize,
    ) -> Self {
        Self {
            source,
            source_text,
            events,
            writer,
            last_byte_offset: 0,
            last_char_offset: 0,
            end_newline: true,
            in_non_writing_block: false,
            table_state: TableState::Head,
            table_alignments: vec![],
            table_cell_index: 0,
            numbers: HashMap::new(),
            embed_provider: None,
            image_resolver: None,
            code_buffer: None,
            code_buffer_byte_range: None,
            code_buffer_char_range: None,
            pending_blockquote_range: None,
            render_tables_as_markdown: true, // Default to markdown rendering
            table_start_offset: None,
            offset_maps: Vec::new(),
            next_node_id: node_id_offset,
            current_node_id: None,
            current_node_char_offset: 0,
            current_node_child_count: 0,
            utf16_checkpoints: vec![(0, 0)],
            paragraph_ranges: Vec::new(),
            current_paragraph_start: None,
            list_depth: 0,
            boundary_only: false,
            syntax_spans: Vec::new(),
            next_syn_id: syn_id_offset,
            pending_inline_formats: Vec::new(),
            _phantom: std::marker::PhantomData,
        }
    }

    /// Create a writer that only tracks paragraph boundaries without generating HTML.
    /// Used for fast boundary discovery in incremental rendering.
    pub fn new_boundary_only(
        source: &'a str,
        source_text: &'a LoroText,
        events: I,
        writer: W,
    ) -> Self {
        Self {
            source,
            source_text,
            events,
            writer,
            last_byte_offset: 0,
            last_char_offset: 0,
            end_newline: true,
            in_non_writing_block: false,
            table_state: TableState::Head,
            table_alignments: vec![],
            table_cell_index: 0,
            numbers: HashMap::new(),
            embed_provider: None,
            image_resolver: None,
            code_buffer: None,
            code_buffer_byte_range: None,
            code_buffer_char_range: None,
            pending_blockquote_range: None,
            render_tables_as_markdown: true,
            table_start_offset: None,
            offset_maps: Vec::new(),
            next_node_id: 0,
            current_node_id: None,
            current_node_char_offset: 0,
            current_node_child_count: 0,
            utf16_checkpoints: vec![(0, 0)],
            syntax_spans: Vec::new(),
            next_syn_id: 0,
            pending_inline_formats: Vec::new(),
            paragraph_ranges: Vec::new(),
            current_paragraph_start: None,
            list_depth: 0,
            boundary_only: true,
            _phantom: std::marker::PhantomData,
        }
    }

    /// Add an embed content provider
    pub fn with_embed_provider(self, provider: E) -> EditorWriter<'a, I, W, E, R> {
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
            embed_provider: Some(provider),
            image_resolver: self.image_resolver,
            code_buffer: self.code_buffer,
            code_buffer_byte_range: self.code_buffer_byte_range,
            code_buffer_char_range: self.code_buffer_char_range,
            pending_blockquote_range: self.pending_blockquote_range,
            render_tables_as_markdown: self.render_tables_as_markdown,
            table_start_offset: self.table_start_offset,
            offset_maps: self.offset_maps,
            next_node_id: self.next_node_id,
            current_node_id: self.current_node_id,
            current_node_char_offset: self.current_node_char_offset,
            current_node_child_count: self.current_node_child_count,
            utf16_checkpoints: self.utf16_checkpoints,
            paragraph_ranges: self.paragraph_ranges,
            current_paragraph_start: self.current_paragraph_start,
            list_depth: self.list_depth,
            boundary_only: self.boundary_only,
            syntax_spans: self.syntax_spans,
            next_syn_id: self.next_syn_id,
            pending_inline_formats: self.pending_inline_formats,
            _phantom: std::marker::PhantomData,
        }
    }

    /// Add an image resolver for mapping markdown image URLs to CDN URLs
    pub fn with_image_resolver<R2: ImageResolver>(
        self,
        resolver: R2,
    ) -> EditorWriter<'a, I, W, E, R2> {
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
            code_buffer: self.code_buffer,
            code_buffer_byte_range: self.code_buffer_byte_range,
            code_buffer_char_range: self.code_buffer_char_range,
            pending_blockquote_range: self.pending_blockquote_range,
            render_tables_as_markdown: self.render_tables_as_markdown,
            table_start_offset: self.table_start_offset,
            offset_maps: self.offset_maps,
            next_node_id: self.next_node_id,
            current_node_id: self.current_node_id,
            current_node_char_offset: self.current_node_char_offset,
            current_node_child_count: self.current_node_child_count,
            utf16_checkpoints: self.utf16_checkpoints,
            paragraph_ranges: self.paragraph_ranges,
            current_paragraph_start: self.current_paragraph_start,
            list_depth: self.list_depth,
            boundary_only: self.boundary_only,
            syntax_spans: self.syntax_spans,
            next_syn_id: self.next_syn_id,
            pending_inline_formats: self.pending_inline_formats,
            _phantom: std::marker::PhantomData,
        }
    }
    #[inline]
    fn write_newline(&mut self) -> Result<(), W::Error> {
        self.end_newline = true;
        if self.boundary_only {
            return Ok(());
        }
        self.writer.write_str("\n")
    }

    #[inline]
    fn write(&mut self, s: &str) -> Result<(), W::Error> {
        if !s.is_empty() {
            self.end_newline = s.ends_with('\n');
        }
        if self.boundary_only {
            return Ok(());
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
    fn emit_syntax(&mut self, range: Range<usize>) -> Result<(), W::Error> {
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

                // In boundary_only mode, just update offsets without HTML
                if self.boundary_only {
                    self.last_char_offset = char_end;
                    self.last_byte_offset = range.end;
                    return Ok(());
                }

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

                    if created_node {
                        self.write("</span>")?;
                        self.end_node();
                    }

                    // Record offset mapping but no syntax span info
                    self.record_mapping(range.clone(), char_start..char_end);
                    self.last_char_offset = char_end;
                    self.last_byte_offset = range.end;
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
    ) -> Result<(), W::Error> {
        if syntax.is_empty() {
            return Ok(());
        }

        let char_start = self.last_char_offset;
        let syntax_char_len = syntax.chars().count();
        let char_end = char_start + syntax_char_len;
        let byte_end = byte_start + syntax.len();

        // In boundary_only mode, just update offsets
        if self.boundary_only {
            self.last_char_offset = char_end;
            self.last_byte_offset = byte_end;
            return Ok(());
        }

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
    fn emit_gap_before(&mut self, next_offset: usize) -> Result<(), W::Error> {
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

    /// Generate a unique node ID
    fn gen_node_id(&mut self) -> String {
        let id = format!("n{}", self.next_node_id);
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
            tracing::warn!("[RECORD_MAPPING] SKIPPED - current_node_id is None!");
        }
    }

    /// Process markdown events and write HTML.
    ///
    /// Returns offset mappings and paragraph boundaries. The HTML is written
    /// to the writer passed in the constructor.
    pub fn run(mut self) -> Result<WriterResult, W::Error> {
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
            use markdown_weaver::TagEnd;
            let is_inline_format_end = matches!(
                &event,
                Event::End(TagEnd::Strong | TagEnd::Emphasis | TagEnd::Strikethrough)
            );

            if matches!(&event, Event::End(_)) && !is_inline_format_end {
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

        Ok(WriterResult {
            offset_maps: self.offset_maps,
            paragraph_ranges: self.paragraph_ranges,
            syntax_spans: self.syntax_spans,
        })
    }

    // Consume raw text events until end tag, for alt attributes
    fn raw_text(&mut self) -> Result<(), W::Error> {
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

    fn process_event(&mut self, event: Event<'_>, range: Range<usize>) -> Result<(), W::Error> {
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
                let char_start = self.last_char_offset;
                let raw_text = &self.source[range.clone()];

                // Emit opening backtick and track it
                if raw_text.starts_with('`') {
                    let syn_id = self.gen_syn_id();
                    let backtick_char_end = char_start + 1;
                    write!(
                        &mut self.writer,
                        "<span class=\"md-syntax-inline\" data-syn-id=\"{}\" data-char-start=\"{}\" data-char-end=\"{}\">`</span>",
                        syn_id, char_start, backtick_char_end
                    )?;
                    self.syntax_spans.push(SyntaxSpanInfo {
                        syn_id,
                        char_range: char_start..backtick_char_end,
                        syntax_type: SyntaxType::Inline,
                        formatted_range: None,
                    });
                    self.last_char_offset += 1;
                }

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
                        "<span class=\"md-syntax-inline\" data-syn-id=\"{}\" data-char-start=\"{}\" data-char-end=\"{}\">`</span>",
                        syn_id, backtick_char_start, backtick_char_end
                    )?;
                    self.syntax_spans.push(SyntaxSpanInfo {
                        syn_id,
                        char_range: backtick_char_start..backtick_char_end,
                        syntax_type: SyntaxType::Inline,
                        formatted_range: None,
                    });
                    self.last_char_offset += 1;
                }
            }
            InlineMath(text) => {
                let raw_text = &self.source[range.clone()];

                // Emit opening $ and track it
                if raw_text.starts_with('$') {
                    let syn_id = self.gen_syn_id();
                    let char_start = self.last_char_offset;
                    let char_end = char_start + 1;
                    write!(
                        &mut self.writer,
                        "<span class=\"md-syntax-inline\" data-syn-id=\"{}\" data-char-start=\"{}\" data-char-end=\"{}\">$</span>",
                        syn_id, char_start, char_end
                    )?;
                    self.syntax_spans.push(SyntaxSpanInfo {
                        syn_id,
                        char_range: char_start..char_end,
                        syntax_type: SyntaxType::Inline,
                        formatted_range: None,
                    });
                    self.last_char_offset += 1;
                }

                self.write(r#"<span class="math math-inline">"#)?;
                let text_char_len = text.chars().count();
                escape_html(&mut self.writer, &text)?;
                self.last_char_offset += text_char_len;
                self.write("</span>")?;

                // Emit closing $ and track it
                if raw_text.ends_with('$') {
                    let syn_id = self.gen_syn_id();
                    let char_start = self.last_char_offset;
                    let char_end = char_start + 1;
                    write!(
                        &mut self.writer,
                        "<span class=\"md-syntax-inline\" data-syn-id=\"{}\" data-char-start=\"{}\" data-char-end=\"{}\">$</span>",
                        syn_id, char_start, char_end
                    )?;
                    self.syntax_spans.push(SyntaxSpanInfo {
                        syn_id,
                        char_range: char_start..char_end,
                        syntax_type: SyntaxType::Inline,
                        formatted_range: None,
                    });
                    self.last_char_offset += 1;
                }
            }
            DisplayMath(text) => {
                let raw_text = &self.source[range.clone()];

                // Emit opening $$ and track it
                if raw_text.starts_with("$$") {
                    let syn_id = self.gen_syn_id();
                    let char_start = self.last_char_offset;
                    let char_end = char_start + 2;
                    write!(
                        &mut self.writer,
                        "<span class=\"md-syntax-inline\" data-syn-id=\"{}\" data-char-start=\"{}\" data-char-end=\"{}\">$$</span>",
                        syn_id, char_start, char_end
                    )?;
                    self.syntax_spans.push(SyntaxSpanInfo {
                        syn_id,
                        char_range: char_start..char_end,
                        syntax_type: SyntaxType::Inline,
                        formatted_range: None,
                    });
                    self.last_char_offset += 2;
                }

                self.write(r#"<span class="math math-display">"#)?;
                let text_char_len = text.chars().count();
                escape_html(&mut self.writer, &text)?;
                self.last_char_offset += text_char_len;
                self.write("</span>")?;

                // Emit closing $$ and track it
                if raw_text.ends_with("$$") {
                    let syn_id = self.gen_syn_id();
                    let char_start = self.last_char_offset;
                    let char_end = char_start + 2;
                    write!(
                        &mut self.writer,
                        "<span class=\"md-syntax-inline\" data-syn-id=\"{}\" data-char-start=\"{}\" data-char-end=\"{}\">$$</span>",
                        syn_id, char_start, char_end
                    )?;
                    self.syntax_spans.push(SyntaxSpanInfo {
                        syn_id,
                        char_range: char_start..char_end,
                        syntax_type: SyntaxType::Inline,
                        formatted_range: None,
                    });
                    self.last_char_offset += 2;
                }
            }
            Html(html) | InlineHtml(html) => {
                // Track offset mapping for raw HTML
                let char_start = self.last_char_offset;
                let html_char_len = html.chars().count();
                let char_end = char_start + html_char_len;

                self.write(&html)?;

                // Record mapping for inline HTML
                self.record_mapping(range.clone(), char_start..char_end);
                self.last_char_offset = char_end;
            }
            SoftBreak => self.write_newline()?,
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

                    // Record syntax span info
                    // self.syntax_spans.push(SyntaxSpanInfo {
                    //     syn_id,
                    //     char_range: char_start..char_end,
                    //     syntax_type: SyntaxType::Inline,
                    //     formatted_range: None,
                    // });

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
                    //self.write("\u{200B}")?;

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
                            "<span class=\"md-syntax-block\" data-syn-id=\"{}\" data-char-start=\"{}\" data-char-end=\"{}\">",
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
                let len = self.numbers.len() + 1;
                self.write("<sup class=\"footnote-reference\"><a href=\"#")?;
                escape_html(&mut self.writer, &name)?;
                self.write("\">")?;
                let number = *self.numbers.entry(name.to_string()).or_insert(len);
                write!(&mut self.writer, "{}", number)?;
                self.write("</a></sup>")?;
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
                            "<span class=\"md-syntax-inline\" data-syn-id=\"{}\" data-char-start=\"{}\" data-char-end=\"{}\">",
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
            WeaverBlock(_) => {}
        }
        Ok(())
    }

    fn start_tag(&mut self, tag: Tag<'_>, range: Range<usize>) -> Result<(), W::Error> {
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
                Tag::Link { .. } => {
                    if raw_text.starts_with('[') {
                        Some("[")
                    } else {
                        None
                    }
                }
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

                // For paired inline syntax (Strong, Emphasis, Strikethrough),
                // track the opening span so we can set formatted_range when closing
                if matches!(tag, Tag::Strong | Tag::Emphasis | Tag::Strikethrough) {
                    self.pending_inline_formats.push((syn_id, char_start));
                }

                // Update tracking - we've consumed this opening syntax
                self.last_char_offset = char_end;
                self.last_byte_offset = range.start + syntax_byte_len;
            }
        }

        // Emit the opening tag
        match tag {
            Tag::HtmlBlock => Ok(()),
            Tag::Paragraph => {
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
                                    "<span class=\"md-syntax-block\" data-syn-id=\"{}\" data-char-start=\"{}\" data-char-end=\"{}\">",
                                    syn_id, char_start, char_end
                                )?;
                                escape_html(&mut self.writer, syntax)?;
                                self.write("</span>\n")?;

                                self.syntax_spans.push(SyntaxSpanInfo {
                                    syn_id,
                                    char_range: char_start..char_end,
                                    syntax_type: SyntaxType::Block,
                                    formatted_range: None,
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
                                "<span class=\"md-syntax-block\" data-syn-id=\"{}\" data-char-start=\"{}\" data-char-end=\"{}\">",
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
                                    "<span class=\"md-syntax-block\" data-syn-id=\"{}\" data-char-start=\"{}\" data-char-end=\"{}\">",
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
                dest_url, title, ..
            } => {
                self.write("<a href=\"")?;
                escape_href(&mut self.writer, &dest_url)?;
                if !title.is_empty() {
                    self.write("\" title=\"")?;
                    escape_html(&mut self.writer, &title)?;
                }
                self.write("\">")
            }
            Tag::Image {
                dest_url,
                title,
                attrs,
                ..
            } => {
                // Emit opening ![
                if range.start < range.end {
                    let raw_text = &self.source[range.clone()];
                    if raw_text.starts_with("![") {
                        let syn_id = self.gen_syn_id();
                        let char_start = self.last_char_offset;
                        let char_end = char_start + 2;

                        write!(
                            &mut self.writer,
                            "<span class=\"md-syntax-inline\" data-syn-id=\"{}\" data-char-start=\"{}\" data-char-end=\"{}\">![</span>",
                            syn_id, char_start, char_end
                        )?;

                        self.syntax_spans.push(SyntaxSpanInfo {
                            syn_id,
                            char_range: char_start..char_end,
                            syntax_type: SyntaxType::Inline,
                            formatted_range: None,
                        });
                    }
                }

                self.write("<img src=\"")?;
                // Try to resolve image URL via resolver, fall back to original
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
                // Consume text events for alt attribute
                self.raw_text()?;
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

                // Emit closing ](url)
                if range.start < range.end {
                    let raw_text = &self.source[range];
                    if let Some(paren_pos) = raw_text.rfind("](") {
                        let syntax = &raw_text[paren_pos..];
                        let syn_id = self.gen_syn_id();
                        let char_start = self.last_char_offset;
                        let syntax_char_len = syntax.chars().count();
                        let char_end = char_start + syntax_char_len;

                        write!(
                            &mut self.writer,
                            "<span class=\"md-syntax-inline\" data-syn-id=\"{}\" data-char-start=\"{}\" data-char-end=\"{}\">",
                            syn_id, char_start, char_end
                        )?;
                        escape_html(&mut self.writer, syntax)?;
                        self.write("</span>")?;

                        self.syntax_spans.push(SyntaxSpanInfo {
                            syn_id,
                            char_range: char_start..char_end,
                            syntax_type: SyntaxType::Inline,
                            formatted_range: None,
                        });
                    }
                }
                Ok(())
            }
            Tag::Embed {
                embed_type,
                dest_url,
                title,
                id,
                attrs,
            } => self.write_embed(embed_type, dest_url, title, id, attrs),
            Tag::WeaverBlock(_, _) => {
                self.in_non_writing_block = true;
                Ok(())
            }
            Tag::FootnoteDefinition(name) => {
                if self.end_newline {
                    self.write("<div class=\"footnote-definition\" id=\"")?;
                } else {
                    self.write("\n<div class=\"footnote-definition\" id=\"")?;
                }
                escape_html(&mut self.writer, &name)?;
                self.write("\"><sup class=\"footnote-definition-label\">")?;
                let len = self.numbers.len() + 1;
                let number = *self.numbers.entry(name.to_string()).or_insert(len);
                write!(&mut self.writer, "{}", number)?;
                self.write("</sup>")
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
    ) -> Result<(), W::Error> {
        use markdown_weaver::TagEnd;

        // Emit tag HTML first
        let result = match tag {
            TagEnd::HtmlBlock => Ok(()),
            TagEnd::Paragraph => {
                // Record paragraph end for boundary tracking
                // BUT skip if inside a list - list owns the paragraph boundary
                if self.list_depth == 0 {
                    if let Some((byte_start, char_start)) = self.current_paragraph_start.take() {
                        let byte_range = byte_start..self.last_byte_offset;
                        let char_range = char_start..self.last_char_offset;
                        self.paragraph_ranges.push((byte_range, char_range));
                    }
                }

                self.end_node();
                self.write("</p>\n")
            }
            TagEnd::Heading(level) => {
                // Record paragraph end for boundary tracking
                if let Some((byte_start, char_start)) = self.current_paragraph_start.take() {
                    let byte_range = byte_start..self.last_byte_offset;
                    let char_range = char_start..self.last_char_offset;
                    self.paragraph_ranges.push((byte_range, char_range));
                }

                self.end_node();
                self.write("</")?;
                write!(&mut self.writer, "{}", level)?;
                self.write(">\n")
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
                if let Some(bq_range) = self.pending_blockquote_range.take() {
                    if bq_range.start < bq_range.end {
                        let raw_text = &self.source[bq_range.clone()];
                        if let Some(gt_pos) = raw_text.find('>') {
                            let para_byte_start = bq_range.start + gt_pos;
                            let para_char_start = self.last_char_offset;

                            // Create a minimal paragraph for the empty blockquote
                            let node_id = self.gen_node_id();
                            write!(&mut self.writer, "<div id=\"{}\"", node_id)?;
                            // self.begin_node(node_id.clone());

                            // // Record start-of-node mapping for cursor positioning
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

                            // Record paragraph boundary for incremental rendering
                            let byte_range = para_byte_start..bq_range.end;
                            let char_range = para_char_start..self.last_char_offset;
                            self.paragraph_ranges.push((byte_range, char_range));
                        }
                    }
                }
                self.write("</blockquote>\n")
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
                                self.write(&temp_output)?;
                            }
                            Err(_) => {
                                // Fallback to plain code block
                                self.write("<pre><code class=\"language-")?;
                                escape_html(&mut self.writer, lang_str)?;
                                self.write("\">")?;
                                escape_html_body_text(&mut self.writer, &buffer)?;
                                self.write("</code></pre>\n")?;
                            }
                        }
                    } else {
                        self.write("<pre><code>")?;
                        escape_html_body_text(&mut self.writer, &buffer)?;
                        self.write("</code></pre>\n")?;
                    }

                    // End node tracking
                    self.end_node();
                } else {
                    self.write("</code></pre>\n")?;
                }

                // Emit closing ``` (emit_gap_before is skipped while buffering)
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
                                "<span class=\"md-syntax-block\" data-syn-id=\"{}\" data-char-start=\"{}\" data-char-end=\"{}\">",
                                syn_id, char_start, char_end
                            )?;
                            escape_html(&mut self.writer, fence)?;
                            self.write("</span>")?;

                            self.syntax_spans.push(SyntaxSpanInfo {
                                syn_id,
                                char_range: char_start..char_end,
                                syntax_type: SyntaxType::Block,
                                formatted_range: None,
                            });

                            self.last_char_offset += fence_char_len;
                            self.last_byte_offset += fence.len();
                        }
                    }
                }

                // Record code block end for paragraph boundary tracking
                if let Some((byte_start, char_start)) = self.current_paragraph_start.take() {
                    let byte_range = byte_start..self.last_byte_offset;
                    let char_range = char_start..self.last_char_offset;
                    self.paragraph_ranges.push((byte_range, char_range));
                }

                Ok(())
            }
            TagEnd::List(true) => {
                self.list_depth = self.list_depth.saturating_sub(1);
                // Record list end for paragraph boundary tracking
                if let Some((byte_start, char_start)) = self.current_paragraph_start.take() {
                    let byte_range = byte_start..self.last_byte_offset;
                    let char_range = char_start..self.last_char_offset;
                    self.paragraph_ranges.push((byte_range, char_range));
                }
                self.write("</ol>\n")
            }
            TagEnd::List(false) => {
                self.list_depth = self.list_depth.saturating_sub(1);
                // Record list end for paragraph boundary tracking
                if let Some((byte_start, char_start)) = self.current_paragraph_start.take() {
                    let byte_range = byte_start..self.last_byte_offset;
                    let char_range = char_start..self.last_char_offset;
                    self.paragraph_ranges.push((byte_range, char_range));
                }
                self.write("</ul>\n")
            }
            TagEnd::Item => {
                self.end_node();
                self.write("</li>\n")
            }
            TagEnd::DefinitionList => self.write("</dl>\n"),
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
            TagEnd::Link => self.write("</a>"),
            TagEnd::Image => Ok(()), // No-op: raw_text() already consumed the End(Image) event
            TagEnd::Embed => Ok(()),
            TagEnd::WeaverBlock(_) => {
                self.in_non_writing_block = false;
                Ok(())
            }
            TagEnd::FootnoteDefinition => self.write("</div>\n"),
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

impl<
        'a,
        I: Iterator<Item = (Event<'a>, Range<usize>)>,
        W: StrWrite,
        E: EmbedContentProvider,
        R: ImageResolver,
    > EditorWriter<'a, I, W, E, R>
{
    fn write_embed(
        &mut self,
        embed_type: EmbedType,
        dest_url: CowStr<'_>,
        title: CowStr<'_>,
        id: CowStr<'_>,
        attrs: Option<markdown_weaver::WeaverAttributes<'_>>,
    ) -> Result<(), W::Error> {
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
            self.write_newline()?;
        } else {
            // Fallback: render as iframe
            self.write("<iframe src=\"")?;
            escape_href(&mut self.writer, &dest_url)?;
            self.write("\" title=\"")?;
            escape_html(&mut self.writer, &title)?;
            if !id.is_empty() {
                self.write("\" id=\"")?;
                escape_html(&mut self.writer, &id)?;
            }
            self.write("\"")?;

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
                    // Skip the content attr in HTML output
                    if attr.as_ref() != "content" {
                        self.write(" ")?;
                        escape_html(&mut self.writer, attr)?;
                        self.write("=\"")?;
                        escape_html(&mut self.writer, value)?;
                        self.write("\"")?;
                    }
                }
            }
            self.write("></iframe>")?;
        }
        Ok(())
    }
}
