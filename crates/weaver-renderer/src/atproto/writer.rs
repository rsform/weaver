//! HTML writer for client-side rendering of AT Protocol entries
//!
//! Similar to StaticPageWriter but designed for client-side use with
//! synchronous embed content injection.

use jacquard::types::string::AtUri;
use markdown_weaver::{
    Alignment, BlockQuoteKind, CodeBlockKind, CowStr, EmbedType, Event, LinkType, Tag,
};
use markdown_weaver_escape::{StrWrite, escape_href, escape_html, escape_html_body_text};
use std::collections::HashMap;
use weaver_common::ResolvedContent;

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

impl EmbedContentProvider for ResolvedContent {
    fn get_embed_content(&self, tag: &Tag<'_>) -> Option<String> {
        let url = match tag {
            Tag::Embed { dest_url, .. } => Some(dest_url.as_ref()),
            // WikiLink images with at:// URLs are embeds in disguise
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
                    return self.get_embed_content(&at_uri).map(|s| s.to_string());
                }
            }
        }
        None
    }
}

/// Simple writer that outputs HTML from markdown events
///
/// This writer is designed for client-side rendering where embeds may have
/// pre-rendered content in their attributes.
pub struct ClientWriter<'a, I: Iterator<Item = Event<'a>>, W: StrWrite, E = ()> {
    events: I,
    writer: W,
    end_newline: bool,
    in_non_writing_block: bool,

    table_state: TableState,
    table_alignments: Vec<Alignment>,
    table_cell_index: usize,

    numbers: HashMap<String, usize>,

    embed_provider: Option<E>,

    code_buffer: Option<(Option<String>, String)>, // (lang, content)
    _phantom: std::marker::PhantomData<&'a ()>,
}

#[derive(Debug, Clone, Copy)]
enum TableState {
    Head,
    Body,
}

impl<'a, I: Iterator<Item = Event<'a>>, W: StrWrite> ClientWriter<'a, I, W> {
    /// Add an embed content provider
    pub fn with_embed_provider<E: EmbedContentProvider>(
        self,
        provider: E,
    ) -> ClientWriter<'a, I, W, E> {
        ClientWriter {
            events: self.events,
            writer: self.writer,
            end_newline: self.end_newline,
            in_non_writing_block: self.in_non_writing_block,
            table_state: self.table_state,
            table_alignments: self.table_alignments,
            table_cell_index: self.table_cell_index,
            numbers: self.numbers,
            embed_provider: Some(provider),
            code_buffer: self.code_buffer,
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<'a, I: Iterator<Item = Event<'a>>, W: StrWrite, E: EmbedContentProvider>
    ClientWriter<'a, I, W, E>
{
    pub fn new(events: I, writer: W) -> Self {
        Self {
            events,
            writer,
            end_newline: true,
            in_non_writing_block: false,
            table_state: TableState::Head,
            table_alignments: vec![],
            table_cell_index: 0,
            numbers: HashMap::new(),
            embed_provider: None,
            code_buffer: None,
            _phantom: std::marker::PhantomData,
        }
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
        while let Some(event) = self.events.next() {
            self.process_event(event)?;
        }
        Ok(self.writer)
    }

    /// Consume events until End tag without writing anything.
    /// Used when we've already rendered content and just need to advance the iterator.
    fn consume_until_end(&mut self) {
        use Event::*;
        let mut nest = 0;
        while let Some(event) = self.events.next() {
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
        while let Some(event) = self.events.next() {
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

    fn process_event(&mut self, event: Event<'_>) -> Result<(), W::Error> {
        use Event::*;
        match event {
            Start(tag) => self.start_tag(tag)?,
            End(tag) => self.end_tag(tag)?,
            Text(text) => {
                // If buffering code, append to buffer instead of writing
                if let Some((_, ref mut buffer)) = self.code_buffer {
                    buffer.push_str(&text);
                } else if !self.in_non_writing_block {
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
            SoftBreak => self.write_newline()?,
            HardBreak => self.write("<br />\n")?,
            Rule => {
                if self.end_newline {
                    self.write("<hr />\n")?;
                } else {
                    self.write("\n<hr />\n")?;
                }
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

    fn start_tag(&mut self, tag: Tag<'_>) -> Result<(), W::Error> {
        match tag {
            Tag::HtmlBlock => self.write(r#"<span class="html-embed html-embed-block">"#),
            Tag::Paragraph => {
                if self.end_newline {
                    self.write("<p>")
                } else {
                    self.write("\n<p>")
                }
            }
            Tag::Heading {
                level,
                id,
                classes,
                attrs,
            } => {
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
                if self.end_newline {
                    self.write("<ol>\n")
                } else {
                    self.write("\n<ol>\n")
                }
            }
            Tag::List(Some(start)) => {
                if self.end_newline {
                    self.write("<ol start=\"")?;
                } else {
                    self.write("\n<ol start=\"")?;
                }
                write!(&mut self.writer, "{}", start)?;
                self.write("\">\n")
            }
            Tag::List(None) => {
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
                    if let Some(embed_provider) = &self.embed_provider {
                        if let Some(html) = embed_provider.get_embed_content(&tag) {
                            tracing::debug!("[ClientWriter] Got embed content for {}", dest_url);
                            // Consume events without writing - we're replacing with embed HTML
                            self.consume_until_end();
                            return self.write(&html);
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

    fn end_tag(&mut self, tag: markdown_weaver::TagEnd) -> Result<(), W::Error> {
        use markdown_weaver::TagEnd;
        match tag {
            TagEnd::HtmlBlock => self.write("</span>\n"),
            TagEnd::Paragraph => self.write("</p>\n"),
            TagEnd::Heading(level) => {
                self.write("</")?;
                write!(&mut self.writer, "{}", level)?;
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
            TagEnd::BlockQuote(_) => self.write("</blockquote>\n"),
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
            TagEnd::List(true) => self.write("</ol>\n"),
            TagEnd::List(false) => self.write("</ul>\n"),
            TagEnd::Item => self.write("</li>\n"),
            TagEnd::DefinitionList => self.write("</dl>\n"),
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
                Ok(())
            }
            TagEnd::FootnoteDefinition => self.write("</div>\n"),
            TagEnd::MetadataBlock(_) => {
                self.in_non_writing_block = false;
                Ok(())
            }
        }
    }
}

impl<'a, I: Iterator<Item = Event<'a>>, W: StrWrite, E: EmbedContentProvider>
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
