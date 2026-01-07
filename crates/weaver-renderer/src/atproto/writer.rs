//! HTML writer for client-side rendering of AT Protocol entries
//!
//! Similar to StaticPageWriter but designed for client-side use with
//! synchronous embed content injection.

use jacquard::types::string::AtUri;
use markdown_weaver::{
    Alignment, BlockQuoteKind, CodeBlockKind, CowStr, EmbedType, Event, LinkType, ParagraphContext,
    Tag, WeaverAttributes,
};
use markdown_weaver_escape::{StrWrite, escape_href, escape_html, escape_html_body_text};
use std::collections::HashMap;
use std::ops::Range;
use weaver_common::ResolvedContent;

/// Tracks the type of wrapper element emitted for WeaverBlock prefix
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WrapperElement {
    Aside,
    Div,
}

/// Synchronous callback for injecting embed content.
///
/// Takes the embed tag and returns optional HTML content to inject.
pub trait EmbedContentProvider {
    fn get_embed_content(&self, tag: &Tag<'_>) -> Option<&str>;
}

impl EmbedContentProvider for () {
    fn get_embed_content(&self, _tag: &Tag<'_>) -> Option<&str> {
        None
    }
}

impl EmbedContentProvider for ResolvedContent {
    fn get_embed_content(&self, tag: &Tag<'_>) -> Option<&str> {
        let url = match tag {
            Tag::Embed { dest_url, .. } => Some(dest_url.as_ref()),
            // WikiLink images with at:// URLs are embeds in disguise.
            Tag::Image {
                link_type: LinkType::WikiLink { .. },
                dest_url,
                ..
            } if dest_url.starts_with("at://") || dest_url.starts_with("did:") => {
                Some(dest_url.as_ref())
            }
            _ => None,
        };

        if let Some(url) = url {
            if url.starts_with("at://") {
                if let Ok(at_uri) = AtUri::new(url) {
                    // Call the inherent method which returns Option<&str>.
                    return ResolvedContent::get_embed_content(self, &at_uri);
                }
            }
        }
        None
    }
}

impl EmbedContentProvider for &ResolvedContent {
    fn get_embed_content(&self, tag: &Tag<'_>) -> Option<&str> {
        <ResolvedContent as EmbedContentProvider>::get_embed_content(*self, tag)
    }
}

/// Simple writer that outputs HTML from markdown events
///
/// This writer is designed for client-side rendering where embeds may have
/// pre-rendered content in their attributes.
pub struct ClientWriter<'a, I: Iterator<Item = (Event<'a>, Range<usize>)>, W: StrWrite, E = ()> {
    events: I,
    writer: W,
    /// Source text for gap detection
    source: &'a str,
    end_newline: bool,
    in_non_writing_block: bool,

    table_state: TableState,
    table_alignments: Vec<Alignment>,
    table_cell_index: usize,

    numbers: HashMap<String, usize>,

    embed_provider: Option<E>,

    code_buffer: Option<(Option<String>, String)>, // (lang, content)

    /// Pending WeaverBlock attrs to apply to the next block element
    pending_block_attrs: Option<WeaverAttributes<'static>>,
    /// Type of wrapper element currently open (needs closing on block end)
    active_wrapper: Option<WrapperElement>,
    /// Buffer for WeaverBlock text content (to parse for attrs)
    weaver_block_buffer: String,
    /// Pending footnote reference waiting to see if definition follows immediately
    pending_footnote: Option<(String, usize)>,
    /// Buffer for content between footnote ref and resolution
    pending_footnote_content: String,
    /// Whether current footnote definition is being rendered as a sidenote
    in_sidenote: bool,
    /// Whether we're deferring paragraph close for sidenote handling
    defer_paragraph_close: bool,
    /// Buffered paragraph opening tag (without closing `>`) for dir attribute emission
    pending_paragraph_open: Option<String>,
    /// Byte offset where last sidenote ended (for gap detection)
    sidenote_end_offset: Option<usize>,

    _phantom: std::marker::PhantomData<&'a ()>,
}

#[derive(Debug, Clone, Copy)]
enum TableState {
    Head,
    Body,
}

impl<'a, I: Iterator<Item = (Event<'a>, Range<usize>)>, W: StrWrite> ClientWriter<'a, I, W> {
    /// Add an embed content provider
    pub fn with_embed_provider<E: EmbedContentProvider>(
        self,
        provider: E,
    ) -> ClientWriter<'a, I, W, E> {
        ClientWriter {
            events: self.events,
            writer: self.writer,
            source: self.source,
            end_newline: self.end_newline,
            in_non_writing_block: self.in_non_writing_block,
            table_state: self.table_state,
            table_alignments: self.table_alignments,
            table_cell_index: self.table_cell_index,
            numbers: self.numbers,
            embed_provider: Some(provider),
            code_buffer: self.code_buffer,
            pending_block_attrs: self.pending_block_attrs,
            active_wrapper: self.active_wrapper,
            weaver_block_buffer: self.weaver_block_buffer,
            pending_footnote: self.pending_footnote,
            pending_footnote_content: self.pending_footnote_content,
            in_sidenote: self.in_sidenote,
            defer_paragraph_close: self.defer_paragraph_close,
            pending_paragraph_open: self.pending_paragraph_open,
            sidenote_end_offset: self.sidenote_end_offset,
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<'a, I: Iterator<Item = (Event<'a>, Range<usize>)>, W: StrWrite, E: EmbedContentProvider>
    ClientWriter<'a, I, W, E>
{
    pub fn new(events: I, writer: W, source: &'a str) -> Self {
        Self {
            events,
            writer,
            source,
            end_newline: true,
            in_non_writing_block: false,
            table_state: TableState::Head,
            table_alignments: vec![],
            table_cell_index: 0,
            numbers: HashMap::new(),
            embed_provider: None,
            code_buffer: None,
            pending_block_attrs: None,
            active_wrapper: None,
            weaver_block_buffer: String::new(),
            pending_footnote: None,
            pending_footnote_content: String::new(),
            in_sidenote: false,
            defer_paragraph_close: false,
            pending_paragraph_open: None,
            sidenote_end_offset: None,
            _phantom: std::marker::PhantomData,
        }
    }

    /// Parse WeaverBlock text content into attributes.
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
                let class = part.strip_prefix('.').unwrap_or(part);
                if !class.is_empty() {
                    classes.push(CowStr::from(class.to_string()));
                }
            }
        }

        WeaverAttributes { classes, attrs }
    }

    /// Close deferred paragraph if we're in that state.
    /// Called when a non-paragraph block element starts.
    fn close_deferred_paragraph(&mut self) -> Result<(), W::Error> {
        // Also flush any pending paragraph open (shouldn't happen in normal flow, but be defensive)
        if let Some(opening) = self.pending_paragraph_open.take() {
            self.write(&opening)?;
            self.write(">")?;
        }
        if self.defer_paragraph_close {
            // Flush pending footnote as traditional before closing
            self.flush_pending_footnote()?;
            self.write("</p>\n")?;
            self.close_wrapper()?;
            self.defer_paragraph_close = false;
        }
        Ok(())
    }

    /// Flush any pending footnote reference as a traditional footnote
    fn flush_pending_footnote(&mut self) -> Result<(), W::Error> {
        if let Some((name, number)) = self.pending_footnote.take() {
            self.write("<sup class=\"footnote-reference\"><a href=\"#")?;
            escape_html(&mut self.writer, &name)?;
            self.write("\">")?;
            write!(&mut self.writer, "{}", number)?;
            self.write("</a></sup>")?;
            if !self.pending_footnote_content.is_empty() {
                let content = std::mem::take(&mut self.pending_footnote_content);
                escape_html_body_text(&mut self.writer, &content)?;
                self.end_newline = content.ends_with('\n');
            }
        }
        Ok(())
    }

    /// Emit wrapper element start based on pending block attrs
    fn emit_wrapper_start(&mut self) -> Result<bool, W::Error> {
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
    fn close_wrapper(&mut self) -> Result<(), W::Error> {
        if let Some(wrapper) = self.active_wrapper.take() {
            match wrapper {
                WrapperElement::Aside => self.write("</aside>\n")?,
                WrapperElement::Div => self.write("</div>\n")?,
            }
        }
        Ok(())
    }

    #[inline]
    fn write_newline(&mut self) -> Result<(), W::Error> {
        self.end_newline = true;
        self.writer.write_str("\n")
    }

    #[inline]
    fn write(&mut self, s: &str) -> Result<(), W::Error> {
        self.writer.write_str(s)?;
        if !s.is_empty() {
            self.end_newline = s.ends_with('\n');
        }
        Ok(())
    }

    /// Process markdown events and write HTML
    pub fn run(mut self) -> Result<W, W::Error> {
        while let Some((event, range)) = self.events.next() {
            self.process_event(event, range)?;
        }
        self.finalize()?;
        Ok(self.writer)
    }

    /// Finalize output, closing any deferred state
    fn finalize(&mut self) -> Result<(), W::Error> {
        // Flush any pending footnote as traditional
        self.flush_pending_footnote()?;
        // Close deferred paragraph if any
        if self.defer_paragraph_close {
            self.write("</p>\n")?;
            self.close_wrapper()?;
            self.defer_paragraph_close = false;
        }
        Ok(())
    }

    /// Consume events until End tag without writing anything.
    /// Used when we've already rendered content and just need to advance the iterator.
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
                } else if self.pending_footnote.is_some() {
                    // Buffer text while waiting to see if footnote def follows
                    self.pending_footnote_content.push_str(&text);
                } else if !self.in_non_writing_block {
                    // Flush pending paragraph with dir attribute if needed
                    if let Some(opening) = self.pending_paragraph_open.take() {
                        if let Some(dir) = crate::utils::detect_text_direction(&text) {
                            self.write(&opening)?;
                            self.write(" dir=\"")?;
                            self.write(dir)?;
                            self.write("\">")?;
                        } else {
                            self.write(&opening)?;
                            self.write(">")?;
                        }
                    }
                    escape_html_body_text(&mut self.writer, &text)?;
                    self.end_newline = text.ends_with('\n');
                }
            }
            Code(text) => {
                self.write("<code>")?;
                escape_html_body_text(&mut self.writer, &text)?;
                self.write("</code>")?;
            }
            InlineMath(text) => match crate::math::render_math(&text, false) {
                crate::math::MathResult::Success(mathml) => {
                    self.write(r#"<span class="math math-inline">"#)?;
                    self.write(&mathml)?;
                    self.write("</span>")?;
                }
                crate::math::MathResult::Error { html, .. } => {
                    self.write(&html)?;
                }
            },
            DisplayMath(text) => match crate::math::render_math(&text, true) {
                crate::math::MathResult::Success(mathml) => {
                    self.write(r#"<span class="math math-display">"#)?;
                    self.write(&mathml)?;
                    self.write("</span>")?;
                }
                crate::math::MathResult::Error { html, .. } => {
                    self.write(&html)?;
                }
            },
            Html(html) => {
                self.write(&html)?;
            }
            InlineHtml(html) => {
                self.write(r#"<span class="html-embed html-embed-inline">"#)?;
                self.write(&html)?;
                self.write("</span>")?;
            }
            SoftBreak => {
                if self.pending_footnote.is_some() {
                    self.pending_footnote_content.push('\n');
                } else {
                    self.write_newline()?;
                }
            }
            HardBreak => {
                if self.pending_footnote.is_some() {
                    self.pending_footnote_content.push_str("<br />\n");
                } else {
                    self.write("<br />\n")?;
                }
            }
            Rule => {
                if self.end_newline {
                    self.write("<hr />\n")?;
                } else {
                    self.write("\n<hr />\n")?;
                }
            }
            FootnoteReference(name) => {
                // Flush any existing pending footnote as traditional
                self.flush_pending_footnote()?;
                // Get/create footnote number
                let len = self.numbers.len() + 1;
                let number = *self.numbers.entry(name.to_string()).or_insert(len);
                // Buffer this reference to see if definition follows immediately
                self.pending_footnote = Some((name.to_string(), number));
            }
            TaskListMarker(checked) => {
                if checked {
                    self.write("<input disabled=\"\" type=\"checkbox\" checked=\"\" aria-label=\"Completed task\"/>\n")?;
                } else {
                    self.write(
                        "<input disabled=\"\" type=\"checkbox\" aria-label=\"Incomplete task\"/>\n",
                    )?;
                }
            }
            WeaverBlock(text) => {
                // Buffer WeaverBlock content for parsing on End
                self.weaver_block_buffer.push_str(&text);
            }
        }
        Ok(())
    }

    fn start_tag(&mut self, tag: Tag<'_>, range: Range<usize>) -> Result<(), W::Error> {
        /// Minimum gap size that indicates a paragraph break (represents \n\n)
        const MIN_PARAGRAPH_GAP: usize = 2;

        match tag {
            Tag::HtmlBlock => self.write(r#"<span class="html-embed html-embed-block">"#),
            Tag::Paragraph(_) => {
                if self.in_sidenote {
                    // Inside sidenote span - don't emit paragraph tags
                    Ok(())
                } else if self.defer_paragraph_close {
                    // Check gap size to decide whether to continue or start new paragraph
                    if let Some(sidenote_end) = self.sidenote_end_offset.take() {
                        let gap = range.start.saturating_sub(sidenote_end);
                        if gap > MIN_PARAGRAPH_GAP {
                            // Large gap - close deferred paragraph and start new one
                            self.write("</p>\n")?;
                            self.close_wrapper()?;
                            self.defer_paragraph_close = false;
                            // Now start the new paragraph normally
                            self.flush_pending_footnote()?;
                            self.emit_wrapper_start()?;
                            let opening = if self.end_newline {
                                String::from("<p")
                            } else {
                                String::from("\n<p")
                            };
                            self.pending_paragraph_open = Some(opening);
                        } else {
                            // Small gap - continue same paragraph, just clear defer flag
                            self.defer_paragraph_close = false;
                        }
                    } else {
                        // No sidenote offset recorded, fall back to old behavior
                        self.defer_paragraph_close = false;
                    }
                    Ok(())
                } else {
                    self.flush_pending_footnote()?;
                    self.emit_wrapper_start()?;
                    // Buffer paragraph opening for dir attribute detection
                    let opening = if self.end_newline {
                        String::from("<p")
                    } else {
                        String::from("\n<p")
                    };
                    self.pending_paragraph_open = Some(opening);
                    Ok(())
                }
            }
            Tag::Heading {
                level,
                id,
                classes,
                attrs,
            } => {
                self.close_deferred_paragraph()?;
                self.emit_wrapper_start()?;
                if !self.end_newline {
                    self.write("\n")?;
                }
                self.write("<")?;
                write!(&mut self.writer, "{}", level)?;
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
                self.write(">")
            }
            Tag::Table(alignments) => {
                self.close_deferred_paragraph()?;
                self.emit_wrapper_start()?;
                self.table_alignments = alignments;
                self.write("<table>")
            }
            Tag::TableHead => {
                self.table_state = TableState::Head;
                self.table_cell_index = 0;
                self.write("<thead><tr>")
            }
            Tag::TableRow => {
                self.table_cell_index = 0;
                self.write("<tr>")
            }
            Tag::TableCell => {
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
            Tag::BlockQuote(kind) => {
                self.close_deferred_paragraph()?;
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
                Ok(())
            }
            Tag::CodeBlock(info) => {
                self.close_deferred_paragraph()?;
                self.emit_wrapper_start()?;
                if !self.end_newline {
                    self.write_newline()?;
                }
                match info {
                    CodeBlockKind::Fenced(info) => {
                        let lang = info.split(' ').next().unwrap();
                        let lang_opt = if lang.is_empty() {
                            None
                        } else {
                            Some(lang.to_string())
                        };
                        // Start buffering
                        self.code_buffer = Some((lang_opt, String::new()));
                        Ok(())
                    }
                    CodeBlockKind::Indented => {
                        // Start buffering with no language
                        self.code_buffer = Some((None, String::new()));
                        Ok(())
                    }
                }
            }
            Tag::List(Some(1)) => {
                self.close_deferred_paragraph()?;
                self.emit_wrapper_start()?;
                if self.end_newline {
                    self.write("<ol>\n")
                } else {
                    self.write("\n<ol>\n")
                }
            }
            Tag::List(Some(start)) => {
                self.close_deferred_paragraph()?;
                self.emit_wrapper_start()?;
                if self.end_newline {
                    self.write("<ol start=\"")?;
                } else {
                    self.write("\n<ol start=\"")?;
                }
                write!(&mut self.writer, "{}", start)?;
                self.write("\">\n")
            }
            Tag::List(None) => {
                self.close_deferred_paragraph()?;
                self.emit_wrapper_start()?;
                if self.end_newline {
                    self.write("<ul>\n")
                } else {
                    self.write("\n<ul>\n")
                }
            }
            Tag::Item => {
                if self.end_newline {
                    self.write("<li>")
                } else {
                    self.write("\n<li>")
                }
            }
            Tag::DefinitionList => {
                self.close_deferred_paragraph()?;
                self.emit_wrapper_start()?;
                if self.end_newline {
                    self.write("<dl>\n")
                } else {
                    self.write("\n<dl>\n")
                }
            }
            Tag::DefinitionListTitle => {
                if self.end_newline {
                    self.write("<dt>")
                } else {
                    self.write("\n<dt>")
                }
            }
            Tag::DefinitionListDefinition => {
                if self.end_newline {
                    self.write("<dd>")
                } else {
                    self.write("\n<dd>")
                }
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
            ref tag @ Tag::Image {
                ref dest_url,
                ref title,
                ref attrs,
                ref link_type,
                ..
            } => {
                // Check if this is an AT embed disguised as a WikiLink image
                // (markdown-weaver parses ![[at://...]] as Image with WikiLink link_type)
                if matches!(link_type, LinkType::WikiLink { .. })
                    && (dest_url.starts_with("at://") || dest_url.starts_with("did:"))
                {
                    tracing::debug!("[ClientWriter] AT embed image detected: {}", dest_url);
                    if let Some(ref embed_provider) = self.embed_provider {
                        if let Some(html) = embed_provider.get_embed_content(&tag) {
                            tracing::debug!("[ClientWriter] Got embed content for {}", dest_url);
                            // Use direct field access to avoid borrow conflict.
                            self.writer.write_str(html)?;
                            self.end_newline = html.ends_with('\n');
                            // Consume events without writing - we've replaced with embed HTML.
                            self.consume_until_end();
                            return Ok(());
                        } else {
                            tracing::debug!(
                                "[ClientWriter] No embed content from provider for {}",
                                dest_url
                            );
                        }
                    } else {
                        tracing::debug!("[ClientWriter] No embed provider available");
                    }
                    // Fallback: render as link if no embed content available
                    tracing::debug!("[ClientWriter] Using fallback link for {}", dest_url);
                    self.consume_until_end();
                    self.write("<a class=\"embed-fallback\" href=\"")?;
                    escape_href(&mut self.writer, &dest_url)?;
                    self.write("\">")?;
                    escape_html(&mut self.writer, &dest_url)?;
                    return self.write("</a>");
                }

                // Regular image handling
                self.write("<img src=\"")?;
                escape_href(&mut self.writer, &dest_url)?;
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
                self.write(" />")
            }
            Tag::Embed {
                embed_type,
                dest_url,
                title,
                id,
                attrs,
            } => self.write_embed(embed_type, dest_url, title, id, attrs),
            Tag::WeaverBlock(_, attrs) => {
                self.in_non_writing_block = true;
                self.weaver_block_buffer.clear();
                // Store attrs from Start tag, will merge with parsed text on End
                if !attrs.classes.is_empty() || !attrs.attrs.is_empty() {
                    self.pending_block_attrs = Some(attrs.into_static());
                }
                Ok(())
            }
            Tag::FootnoteDefinition(name) => {
                // Check if this matches a pending footnote reference (sidenote case)
                let is_sidenote = self
                    .pending_footnote
                    .as_ref()
                    .map(|(n, _)| n.as_str() == name.as_ref())
                    .unwrap_or(false);

                if is_sidenote {
                    // Emit sidenote structure at reference position
                    let (_, number) = self.pending_footnote.take().unwrap();
                    let id = format!("sn-{}", number);

                    // Emit: <label><input/><span class="sidenote">
                    self.write("<label for=\"")?;
                    self.write(&id)?;
                    self.write("\" class=\"sidenote-number\"></label>")?;
                    self.write("<input type=\"checkbox\" id=\"")?;
                    self.write(&id)?;
                    self.write("\" class=\"margin-toggle\"/>")?;
                    self.write("<span class=\"sidenote\">")?;

                    self.in_sidenote = true;
                } else {
                    // Traditional footnote - close any deferred paragraph (which also flushes pending ref)
                    self.close_deferred_paragraph()?;

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
                    self.write("</sup>")?;
                }
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
    ) -> Result<(), W::Error> {
        use markdown_weaver::TagEnd;
        match tag {
            TagEnd::HtmlBlock => self.write("</span>\n"),
            TagEnd::Paragraph(ctx) => {
                if self.in_sidenote {
                    // Inside sidenote span - don't emit paragraph tags
                    Ok(())
                } else if ctx == ParagraphContext::Interrupted && self.pending_footnote.is_some() {
                    // Paragraph was interrupted AND we have a pending footnote,
                    // defer the </p> close - the sidenote will be rendered inline
                    self.defer_paragraph_close = true;
                    Ok(())
                } else if self.defer_paragraph_close {
                    // We were deferring but now closing for real
                    self.write("</p>\n")?;
                    self.close_wrapper()?;
                    self.defer_paragraph_close = false;
                    Ok(())
                } else {
                    // Flush any pending paragraph open (for empty paragraphs)
                    if let Some(opening) = self.pending_paragraph_open.take() {
                        self.write(&opening)?;
                        self.write(">")?;
                    }
                    self.write("</p>\n")?;
                    self.close_wrapper()
                }
            }
            TagEnd::Heading(level) => {
                self.write("</")?;
                write!(&mut self.writer, "{}", level)?;
                // Don't close wrapper - headings typically go with following block
                self.write(">\n")
            }
            TagEnd::Table => self.write("</tbody></table>\n"),
            TagEnd::TableHead => {
                self.write("</tr></thead><tbody>\n")?;
                self.table_state = TableState::Body;
                Ok(())
            }
            TagEnd::TableRow => self.write("</tr>\n"),
            TagEnd::TableCell => {
                match self.table_state {
                    TableState::Head => self.write("</th>")?,
                    TableState::Body => self.write("</td>")?,
                }
                self.table_cell_index += 1;
                Ok(())
            }
            TagEnd::BlockQuote(_) => {
                // Close any deferred paragraph before closing blockquote
                // (footnotes inside blockquotes can't be sidenotes since def is outside)
                self.close_deferred_paragraph()?;
                self.write("</blockquote>\n")?;
                self.close_wrapper()
            }
            TagEnd::CodeBlock => {
                use std::sync::LazyLock;
                use syntect::parsing::SyntaxSet;
                static SYNTAX_SET: LazyLock<SyntaxSet> =
                    LazyLock::new(|| SyntaxSet::load_defaults_newlines());

                if let Some((lang, buffer)) = self.code_buffer.take() {
                    if let Some(ref lang_str) = lang {
                        // Use a temporary String buffer for syntect
                        let mut temp_output = String::new();
                        match crate::code_pretty::highlight(
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
                } else {
                    self.write("</code></pre>\n")?;
                }
                Ok(())
            }
            TagEnd::List(true) => {
                self.write("</ol>\n")?;
                self.close_wrapper()
            }
            TagEnd::List(false) => {
                self.write("</ul>\n")?;
                self.close_wrapper()
            }
            TagEnd::Item => self.write("</li>\n"),
            TagEnd::DefinitionList => {
                self.write("</dl>\n")?;
                self.close_wrapper()
            }
            TagEnd::DefinitionListTitle => self.write("</dt>\n"),
            TagEnd::DefinitionListDefinition => self.write("</dd>\n"),
            TagEnd::Emphasis => self.write("</em>"),
            TagEnd::Superscript => self.write("</sup>"),
            TagEnd::Subscript => self.write("</sub>"),
            TagEnd::Strong => self.write("</strong>"),
            TagEnd::Strikethrough => self.write("</s>"),
            TagEnd::Link => self.write("</a>"),
            TagEnd::Image => Ok(()), // No-op: raw_text() already consumed the End(Image) event
            TagEnd::Embed => Ok(()),
            TagEnd::WeaverBlock(_) => {
                self.in_non_writing_block = false;
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
                if self.in_sidenote {
                    self.write("</span>")?;
                    self.in_sidenote = false;
                    // Record where sidenote ended for gap detection
                    self.sidenote_end_offset = Some(range.end);
                    // Write any buffered content that came after the ref
                    if !self.pending_footnote_content.is_empty() {
                        let content = std::mem::take(&mut self.pending_footnote_content);
                        escape_html_body_text(&mut self.writer, &content)?;
                        self.end_newline = content.ends_with('\n');
                    }
                } else {
                    self.write("</div>\n")?;
                }
                Ok(())
            }
            TagEnd::MetadataBlock(_) => {
                self.in_non_writing_block = false;
                Ok(())
            }
        }
    }
}

impl<'a, I: Iterator<Item = (Event<'a>, Range<usize>)>, W: StrWrite, E: EmbedContentProvider>
    ClientWriter<'a, I, W, E>
{
    fn write_embed(
        &mut self,
        embed_type: EmbedType,
        dest_url: CowStr<'_>,
        title: CowStr<'_>,
        id: CowStr<'_>,
        attrs: Option<markdown_weaver::WeaverAttributes<'_>>,
    ) -> Result<(), W::Error> {
        // Try to get content from attributes first.
        let content_from_attrs: Option<&str> = attrs
            .as_ref()
            .and_then(|a| a.attrs.iter().find(|(k, _)| k.as_ref() == "content"))
            .map(|(_, v)| v.as_ref());

        // Write content if found in attrs, otherwise try provider, otherwise fallback.
        if let Some(content) = content_from_attrs {
            self.write(content)?;
            self.write_newline()?;
        } else if let Some(ref provider) = self.embed_provider {
            let tag = Tag::Embed {
                embed_type,
                dest_url: dest_url.clone(),
                title: title.clone(),
                id: id.clone(),
                attrs: attrs.clone(),
            };
            if let Some(content) = provider.get_embed_content(&tag) {
                // Use direct field access to avoid borrow conflict:
                // `provider` borrows self.embed_provider, `content` borrows from provider,
                // but self.writer is a different field so we can borrow it independently.
                self.writer.write_str(content)?;
                self.end_newline = content.ends_with('\n');
                self.writer.write_str("\n")?;
                self.end_newline = true;
            } else {
                self.write_embed_fallback(&dest_url, &title, &id, attrs.as_ref())?;
            }
        } else {
            self.write_embed_fallback(&dest_url, &title, &id, attrs.as_ref())?;
        }
        Ok(())
    }

    fn write_embed_fallback(
        &mut self,
        dest_url: &str,
        title: &str,
        id: &str,
        attrs: Option<&markdown_weaver::WeaverAttributes<'_>>,
    ) -> Result<(), W::Error> {
        self.write("<iframe src=\"")?;
        escape_href(&mut self.writer, dest_url)?;
        self.write("\" title=\"")?;
        escape_html(&mut self.writer, title)?;
        if !id.is_empty() {
            self.write("\" id=\"")?;
            escape_html(&mut self.writer, id)?;
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
                // Skip the content attr in HTML output.
                if attr.as_ref() != "content" {
                    self.write(" ")?;
                    escape_html(&mut self.writer, attr)?;
                    self.write("=\"")?;
                    escape_html(&mut self.writer, value)?;
                    self.write("\"")?;
                }
            }
        }
        self.write("></iframe>")
    }
}
