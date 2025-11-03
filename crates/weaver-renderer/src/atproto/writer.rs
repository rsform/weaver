//! HTML writer for client-side rendering of AT Protocol entries
//!
//! Similar to StaticPageWriter but designed for client-side use with
//! synchronous embed content injection.

use markdown_weaver::{
    Alignment, BlockQuoteKind, CodeBlockKind, CowStr, EmbedType, Event, LinkType, Tag,
};
use markdown_weaver_escape::{StrWrite, escape_href, escape_html, escape_html_body_text};
use std::collections::HashMap;

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

/// Simple writer that outputs HTML from markdown events
///
/// This writer is designed for client-side rendering where embeds may have
/// pre-rendered content in their attributes.
pub struct ClientWriter<W: StrWrite, E = ()> {
    writer: W,
    end_newline: bool,
    in_non_writing_block: bool,

    table_state: TableState,
    table_alignments: Vec<Alignment>,
    table_cell_index: usize,

    numbers: HashMap<String, usize>,

    embed_provider: Option<E>,
}

#[derive(Debug, Clone, Copy)]
enum TableState {
    Head,
    Body,
}

impl<W: StrWrite, E: EmbedContentProvider> ClientWriter<W, E> {
    pub fn new(writer: W) -> Self {
        Self {
            writer,
            end_newline: true,
            in_non_writing_block: false,
            table_state: TableState::Head,
            table_alignments: vec![],
            table_cell_index: 0,
            numbers: HashMap::new(),
            embed_provider: None,
        }
    }

    /// Add an embed content provider
    pub fn with_embed_provider(self, provider: E) -> ClientWriter<W, E> {
        ClientWriter {
            writer: self.writer,
            end_newline: self.end_newline,
            in_non_writing_block: self.in_non_writing_block,
            table_state: self.table_state,
            table_alignments: self.table_alignments,
            table_cell_index: self.table_cell_index,
            numbers: self.numbers,
            embed_provider: Some(provider),
        }
    }
}

impl<W: StrWrite, E: EmbedContentProvider> ClientWriter<W, E> {
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
    pub fn run<'a>(mut self, events: impl Iterator<Item = Event<'a>>) -> Result<W, W::Error> {
        for event in events {
            self.process_event(event)?;
        }
        Ok(self.writer)
    }

    fn process_event(&mut self, event: Event<'_>) -> Result<(), W::Error> {
        use Event::*;
        match event {
            Start(tag) => self.start_tag(tag)?,
            End(tag) => self.end_tag(tag)?,
            Text(text) => {
                if !self.in_non_writing_block {
                    escape_html_body_text(&mut self.writer, &text)?;
                    self.end_newline = text.ends_with('\n');
                }
            }
            Code(text) => {
                self.write("<code>")?;
                escape_html_body_text(&mut self.writer, &text)?;
                self.write("</code>")?;
            }
            InlineMath(text) => {
                self.write(r#"<span class="math math-inline">"#)?;
                escape_html(&mut self.writer, &text)?;
                self.write("</span>")?;
            }
            DisplayMath(text) => {
                self.write(r#"<span class="math math-display">"#)?;
                escape_html(&mut self.writer, &text)?;
                self.write("</span>")?;
            }
            Html(html) | InlineHtml(html) => {
                self.write(&html)?;
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
            Tag::HtmlBlock => Ok(()),
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
                        let lang = info.split(' ').next().unwrap_or("");
                        if !lang.is_empty() {
                            self.write("<pre><code class=\"language-")?;
                            escape_html(&mut self.writer, lang)?;
                            self.write("\">")?;
                        } else {
                            self.write("<pre><code>")?;
                        }
                    }
                    CodeBlockKind::Indented => {
                        self.write("<pre><code>")?;
                    }
                }
                Ok(())
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
            Tag::Strikethrough => self.write("<del>"),
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
                self.write("<img src=\"")?;
                escape_href(&mut self.writer, &dest_url)?;
                self.write("\" alt=\"")?;
                if !title.is_empty() {
                    escape_html(&mut self.writer, &title)?;
                }
                if let Some(attrs) = attrs {
                    if !attrs.classes.is_empty() {
                        self.write("\" class=\"")?;
                        for (i, class) in attrs.classes.iter().enumerate() {
                            if i > 0 {
                                self.write(" ")?;
                            }
                            escape_html(&mut self.writer, class)?;
                        }
                    }
                    self.write("\"")?;
                    for (attr, value) in &attrs.attrs {
                        self.write(" ")?;
                        escape_html(&mut self.writer, attr)?;
                        self.write("=\"")?;
                        escape_html(&mut self.writer, value)?;
                        self.write("\"")?;
                    }
                } else {
                    self.write("\"")?;
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
            TagEnd::HtmlBlock => Ok(()),
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
            TagEnd::CodeBlock => self.write("</code></pre>\n"),
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
            TagEnd::Strikethrough => self.write("</del>"),
            TagEnd::Link => self.write("</a>"),
            TagEnd::Image => Ok(()),
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

impl<W: StrWrite, E: EmbedContentProvider> ClientWriter<W, E> {
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
