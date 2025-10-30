use crate::{NotebookProcessor, base_html::TableState, static_site::context::StaticSiteContext};
use dashmap::DashMap;
use markdown_weaver::{
    Alignment, BlockQuoteKind, CodeBlockKind, CowStr, EmbedType, Event, LinkType, Tag,
};
use markdown_weaver_escape::{StrWrite, escape_href, escape_html, escape_html_body_text};
use n0_future::StreamExt;
use weaver_common::jacquard::{client::AgentSession, prelude::*};

pub struct StaticPageWriter<'input, I: Iterator<Item = Event<'input>>, A: AgentSession, W: StrWrite>
{
    context: NotebookProcessor<'input, I, StaticSiteContext<A>>,
    writer: W,
    /// Whether or not the last write wrote a newline.
    end_newline: bool,

    /// Whether if inside a metadata block (text should not be written)
    in_non_writing_block: bool,

    table_state: TableState,
    table_alignments: Vec<Alignment>,
    table_cell_index: usize,
    numbers: DashMap<CowStr<'input>, usize>,

    code_buffer: Option<(Option<String>, String)>, // (lang, content)
}

impl<'input, I: Iterator<Item = Event<'input>>, A: AgentSession, W: StrWrite>
    StaticPageWriter<'input, I, A, W>
{
    pub fn new(context: NotebookProcessor<'input, I, StaticSiteContext<A>>, writer: W) -> Self {
        Self {
            context,
            writer,
            end_newline: true,
            in_non_writing_block: false,
            table_state: TableState::Head,
            table_alignments: vec![],
            table_cell_index: 0,
            numbers: DashMap::new(),
            code_buffer: None,
        }
    }

    /// Writes a new line.
    #[inline]
    fn write_newline(&mut self) -> Result<(), W::Error> {
        self.end_newline = true;
        self.writer.write_str("\n")
    }

    /// Writes a buffer, and tracks whether or not a newline was written.
    #[inline]
    fn write(&mut self, s: &str) -> Result<(), W::Error> {
        self.writer.write_str(s)?;

        if !s.is_empty() {
            self.end_newline = s.ends_with('\n');
        }
        Ok(())
    }

    fn end_tag(&mut self, tag: markdown_weaver::TagEnd) -> Result<(), W::Error> {
        use markdown_weaver::TagEnd;
        match tag {
            TagEnd::HtmlBlock => {}
            TagEnd::Paragraph => {
                self.write("</p>\n")?;
            }
            TagEnd::Heading(level) => {
                self.write("</")?;
                write!(&mut self.writer, "{}", level)?;
                self.write(">\n")?;
            }
            TagEnd::Table => {
                self.write("</tbody></table>\n")?;
            }
            TagEnd::TableHead => {
                self.write("</tr></thead><tbody>\n")?;
                self.table_state = TableState::Body;
            }
            TagEnd::TableRow => {
                self.write("</tr>\n")?;
            }
            TagEnd::TableCell => {
                match self.table_state {
                    TableState::Head => {
                        self.write("</th>")?;
                    }
                    TableState::Body => {
                        self.write("</td>")?;
                    }
                }
                self.table_cell_index += 1;
            }
            TagEnd::BlockQuote(_) => {
                self.write("</blockquote>\n")?;
            }
            TagEnd::CodeBlock => {
                if let Some((lang, buffer)) = self.code_buffer.take() {
                    if let Some(ref lang_str) = lang {
                        // Use a temporary String buffer for syntect
                        let mut temp_output = String::new();
                        match crate::code_pretty::highlight(
                            &self.context.context.syntax_set,
                            Some(lang_str),
                            &buffer,
                            &mut temp_output,
                        ) {
                            Ok(_) => {
                                self.write(&temp_output)?;
                            },
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
            }
            TagEnd::List(true) => {
                self.write("</ol>\n")?;
            }
            TagEnd::List(false) => {
                self.write("</ul>\n")?;
            }
            TagEnd::Item => {
                self.write("</li>\n")?;
            }
            TagEnd::DefinitionList => {
                self.write("</dl>\n")?;
            }
            TagEnd::DefinitionListTitle => {
                self.write("</dt>\n")?;
            }
            TagEnd::DefinitionListDefinition => {
                self.write("</dd>\n")?;
            }
            TagEnd::Emphasis => {
                self.write("</em>")?;
            }
            TagEnd::Superscript => {
                self.write("</sup>")?;
            }
            TagEnd::Subscript => {
                self.write("</sub>")?;
            }
            TagEnd::Strong => {
                self.write("</strong>")?;
            }
            TagEnd::Strikethrough => {
                self.write("</del>")?;
            }
            TagEnd::Link => {
                self.write("</a>")?;
            }
            TagEnd::Image => (), // shouldn't happen, handled in start
            TagEnd::Embed => (), // shouldn't happen, handled in start
            TagEnd::WeaverBlock(_) => {
                self.in_non_writing_block = false;
            }
            TagEnd::FootnoteDefinition => {
                self.write("</div>\n")?;
            }
            TagEnd::MetadataBlock(_) => {
                self.in_non_writing_block = false;
            }
        }
        Ok(())
    }
}

impl<'input, I: Iterator<Item = Event<'input>>, A: AgentSession + IdentityResolver, W: StrWrite>
    StaticPageWriter<'input, I, A, W>
{
    pub async fn run(mut self) -> Result<(), W::Error> {
        while let Some(event) = self.context.next().await {
            self.process_event(event).await?
        }
        Ok(())
    }

    async fn process_event(&mut self, event: Event<'input>) -> Result<(), W::Error> {
        use markdown_weaver::Event::*;
        match event {
            Start(tag) => {
                self.start_tag(tag).await?;
            }
            End(tag) => {
                self.end_tag(tag)?;
            }
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
            SoftBreak => {
                self.write_newline()?;
            }
            HardBreak => {
                self.write("<br />\n")?;
            }
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
                let number = *self.numbers.entry(name.into_static()).or_insert(len);
                write!(&mut self.writer, "{}", number)?;
                self.write("</a></sup>")?;
            }
            TaskListMarker(true) => {
                self.write("<input disabled=\"\" type=\"checkbox\" checked=\"\"/>\n")?;
            }
            TaskListMarker(false) => {
                self.write("<input disabled=\"\" type=\"checkbox\"/>\n")?;
            }
            WeaverBlock(_text) => {}
        }
        Ok(())
    }

    // run raw text, consuming end tag
    async fn raw_text(&mut self) -> Result<(), W::Error> {
        use markdown_weaver::Event::*;
        let mut nest = 0;
        while let Some(event) = self.context.next().await {
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
                    let number = *self.numbers.entry(name.into_static()).or_insert(len);
                    write!(&mut self.writer, "[{}]", number)?;
                }
                TaskListMarker(true) => self.write("[x]")?,
                TaskListMarker(false) => self.write("[ ]")?,
                WeaverBlock(_) => {
                    println!("Weaver block internal");
                }
            }
        }
        Ok(())
    }

    /// Writes the start of an HTML tag.
    async fn start_tag(&mut self, tag: Tag<'input>) -> Result<(), W::Error> {
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
                if self.end_newline {
                    self.write("<")?;
                } else {
                    self.write("\n<")?;
                }
                write!(&mut self.writer, "{}", level)?;
                if let Some(id) = id {
                    self.write(" id=\"")?;
                    escape_html(&mut self.writer, &id)?;
                    self.write("\"")?;
                }
                let mut classes = classes.iter();
                if let Some(class) = classes.next() {
                    self.write(" class=\"")?;
                    escape_html(&mut self.writer, class)?;
                    for class in classes {
                        self.write(" ")?;
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
                    TableState::Head => {
                        self.write("<th")?;
                    }
                    TableState::Body => {
                        self.write("<td")?;
                    }
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
                    Some(kind) => match kind {
                        BlockQuoteKind::Note => " class=\"markdown-alert-note\"",
                        BlockQuoteKind::Tip => " class=\"markdown-alert-tip\"",
                        BlockQuoteKind::Important => " class=\"markdown-alert-important\"",
                        BlockQuoteKind::Warning => " class=\"markdown-alert-warning\"",
                        BlockQuoteKind::Caution => " class=\"markdown-alert-caution\"",
                    },
                };
                if self.end_newline {
                    self.write(&format!("<blockquote{}>\n", class_str))
                } else {
                    self.write(&format!("\n<blockquote{}>\n", class_str))
                }
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
            Tag::Strikethrough => self.write("<del>"),
            Tag::Link {
                link_type: LinkType::Email,
                dest_url,
                title,
                id: _,
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
                link_type: _,
                dest_url,
                title,
                id: _,
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
                link_type,
                dest_url,
                title,
                id,
                attrs,
            } => {
                self.write_image(Tag::Image {
                    link_type,
                    dest_url,
                    title,
                    id,
                    attrs,
                })
                .await
            }
            Tag::Embed {
                embed_type,
                dest_url,
                title,
                id,
                attrs,
            } => {
                if let Some(attrs) = attrs {
                    if let Some((_, content)) = attrs
                        .attrs
                        .iter()
                        .find(|(attr, _)| attr.as_ref() == "content")
                    {
                        match embed_type {
                            EmbedType::Image => {
                                self.write_image(Tag::Image {
                                    link_type: LinkType::Inline,
                                    dest_url,
                                    title,
                                    id,
                                    attrs: Some(attrs.clone()),
                                })
                                .await?
                            }
                            EmbedType::Comments => {
                                self.write("leaflet would go here\n")?;
                            }
                            EmbedType::Post => {
                                // Bluesky post embed, basically just render the raw html we got
                                self.write(content)?;
                                self.write_newline()?;
                            }
                            EmbedType::Markdown => {
                                // let context = self
                                //     .context
                                //     .context
                                //     .clone_with_path(&Path::new(&dest_url.to_string()));
                                // let callback =
                                //     if let Some(dir_contents) = context.dir_contents.clone() {
                                //         Some(VaultBrokenLinkCallback {
                                //             vault_contents: dir_contents,
                                //         })
                                //     } else {
                                //         None
                                //     };
                                // let parser = Parser::new_with_broken_link_callback(
                                //     &content,
                                //     context.md_options,
                                //     callback,
                                // );
                                // let iterator = ContextIterator::default(parser);
                                // let mut stream = NotebookProcessor::new(context, iterator);
                                // while let Some(event) = stream.next().await {
                                //     self.process_event(event).await?;
                                // }
                                //
                                self.write("markdown embed would go here\n")?;
                            }
                            EmbedType::Leaflet => {
                                self.write("leaflet would go here\n")?;
                            }
                            EmbedType::Other => {
                                self.write("other embed would go here\n")?;
                            }
                        }
                    }
                } else {
                    self.write("<iframe src=\"")?;
                    escape_href(&mut self.writer, &dest_url)?;
                    self.write("\" title=\"")?;
                    escape_html(&mut self.writer, &title)?;
                    if !id.is_empty() {
                        self.write("\" id=\"")?;
                        escape_html(&mut self.writer, &id)?;
                        self.write("\"")?;
                    }
                    if let Some(attrs) = attrs {
                        self.write(" ")?;
                        if !attrs.classes.is_empty() {
                            self.write("class=\"")?;
                            for class in &attrs.classes {
                                escape_html(&mut self.writer, class)?;
                                self.write(" ")?;
                            }
                            self.write("\" ")?;
                        }
                        if !attrs.attrs.is_empty() {
                            for (attr, value) in &attrs.attrs {
                                escape_html(&mut self.writer, attr)?;
                                self.write("=\"")?;
                                escape_html(&mut self.writer, value)?;
                                self.write("\" ")?;
                            }
                        }
                    }
                    self.write("/>")?;
                }
                Ok(())
            }
            Tag::WeaverBlock(_, _attrs) => {
                println!("Weaver block");
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
                let number = *self.numbers.entry(name.into_static()).or_insert(len);
                write!(&mut self.writer, "{}", number)?;
                self.write("</sup>")
            }
            Tag::MetadataBlock(_) => {
                self.in_non_writing_block = true;
                Ok(())
            }
        }
    }

    async fn write_image(&mut self, tag: Tag<'input>) -> Result<(), W::Error> {
        if let Tag::Image {
            link_type: _,
            dest_url,
            title,
            id: _,
            attrs,
        } = tag
        {
            self.write("<img src=\"")?;
            escape_href(&mut self.writer, &dest_url)?;
            if let Some(attrs) = attrs {
                if !attrs.classes.is_empty() {
                    self.write("\" class=\"")?;
                    for class in &attrs.classes {
                        escape_html(&mut self.writer, class)?;
                        self.write(" ")?;
                    }
                    self.write("\" ")?;
                } else {
                    self.write("\" ")?;
                }
                if !attrs.attrs.is_empty() {
                    for (attr, value) in &attrs.attrs {
                        escape_html(&mut self.writer, attr)?;
                        self.write("=\"")?;
                        escape_html(&mut self.writer, value)?;
                        self.write("\" ")?;
                    }
                }
            } else {
                self.write("\" ")?;
            }
            self.write("alt=\"")?;
            self.raw_text().await?;
            if !title.is_empty() {
                self.write("\" title=\"")?;
                escape_html(&mut self.writer, &title)?;
            }
            self.write("\" />")
        } else {
            self.write_newline()
        }
    }
}
