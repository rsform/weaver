use crate::{NotebookProcessor, base_html::TableState, static_site::context::StaticSiteContext};
use dashmap::DashMap;
use markdown_weaver::{
    Alignment, BlockQuoteKind, CodeBlockKind, CowStr, EmbedType, Event, LinkType,
    ParagraphContext, Tag, WeaverAttributes,
};
use markdown_weaver_escape::{StrWrite, escape_href, escape_html, escape_html_body_text};
use n0_future::StreamExt;
use weaver_common::jacquard::{client::AgentSession, prelude::*};

/// Tracks the type of wrapper element emitted for WeaverBlock prefix
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WrapperElement {
    Aside,
    Div,
}

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

    /// Pending WeaverBlock attrs to apply to the next block element
    pending_block_attrs: Option<WeaverAttributes<'static>>,
    /// Type of wrapper element currently open, and the block depth at which it was opened
    active_wrapper: Option<(WrapperElement, usize)>,
    /// Current block nesting depth (for wrapper close tracking)
    block_depth: usize,
    /// Buffer for WeaverBlock text content (to parse for attrs)
    weaver_block_buffer: String,
    /// Pending footnote reference waiting to see if definition follows immediately
    pending_footnote: Option<(CowStr<'static>, usize)>,
    /// Buffer for content between footnote ref and resolution
    pending_footnote_content: String,
    /// Whether current footnote definition is being rendered as a sidenote
    in_sidenote: bool,
    /// Whether we're deferring paragraph close for sidenote handling
    defer_paragraph_close: bool,
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
            pending_block_attrs: None,
            active_wrapper: None,
            block_depth: 0,
            weaver_block_buffer: String::new(),
            pending_footnote: None,
            pending_footnote_content: String::new(),
            in_sidenote: false,
            defer_paragraph_close: false,
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

    /// Close deferred paragraph if we're in that state.
    /// Called when a non-paragraph block element starts.
    fn close_deferred_paragraph(&mut self) -> Result<(), W::Error> {
        if self.defer_paragraph_close {
            // Flush pending footnote as traditional before closing
            self.flush_pending_footnote()?;
            self.write("</p>\n")?;
            self.block_depth -= 1;
            self.close_wrapper()?;
            self.defer_paragraph_close = false;
        }
        Ok(())
    }

    /// Flush any pending footnote reference as a traditional footnote,
    /// then write any buffered content that came after the reference.
    fn flush_pending_footnote(&mut self) -> Result<(), W::Error> {
        if let Some((name, number)) = self.pending_footnote.take() {
            // Emit traditional footnote reference
            self.write("<sup class=\"footnote-reference\"><a href=\"#")?;
            escape_html(&mut self.writer, &name)?;
            self.write("\">")?;
            write!(&mut self.writer, "{}", number)?;
            self.write("</a></sup>")?;
            // Write any buffered content
            if !self.pending_footnote_content.is_empty() {
                let content = std::mem::take(&mut self.pending_footnote_content);
                escape_html_body_text(&mut self.writer, &content)?;
                self.end_newline = content.ends_with('\n');
            }
        }
        Ok(())
    }

    /// Emit wrapper element start based on pending block attrs
    /// Returns true if a wrapper was emitted
    fn emit_wrapper_start(&mut self) -> Result<bool, W::Error> {
        if let Some(attrs) = self.pending_block_attrs.take() {
            let is_aside = attrs.classes.iter().any(|c| c.as_ref() == "aside");

            if !self.end_newline {
                self.write("\n")?;
            }

            if is_aside {
                self.write("<aside")?;
                self.active_wrapper = Some((WrapperElement::Aside, self.block_depth));
            } else {
                self.write("<div")?;
                self.active_wrapper = Some((WrapperElement::Div, self.block_depth));
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

    /// Close active wrapper element if one is open and we're at the right depth
    fn close_wrapper(&mut self) -> Result<(), W::Error> {
        if let Some((wrapper, open_depth)) = self.active_wrapper.take() {
            if self.block_depth == open_depth {
                match wrapper {
                    WrapperElement::Aside => self.write("</aside>\n")?,
                    WrapperElement::Div => self.write("</div>\n")?,
                }
            } else {
                // Not at the right depth yet, put it back
                self.active_wrapper = Some((wrapper, open_depth));
            }
        }
        Ok(())
    }

    fn end_tag(&mut self, tag: markdown_weaver::TagEnd) -> Result<(), W::Error> {
        use markdown_weaver::TagEnd;
        match tag {
            TagEnd::HtmlBlock => {}
            TagEnd::Paragraph(ctx) => {
                if self.in_sidenote {
                    // Inside sidenote span - don't emit paragraph tags
                } else if ctx == ParagraphContext::Interrupted && self.pending_footnote.is_some() {
                    // Paragraph was interrupted AND we have a pending footnote,
                    // defer the </p> close - the sidenote will be rendered inline
                    self.defer_paragraph_close = true;
                    // Don't decrement block_depth yet - we're continuing the virtual paragraph
                } else if self.defer_paragraph_close {
                    // We were deferring but now closing for real
                    self.write("</p>\n")?;
                    self.block_depth -= 1;
                    self.close_wrapper()?;
                    self.defer_paragraph_close = false;
                } else {
                    self.write("</p>\n")?;
                    self.block_depth -= 1;
                    self.close_wrapper()?;
                }
            }
            TagEnd::Heading(level) => {
                self.write("</")?;
                write!(&mut self.writer, "{}", level)?;
                self.block_depth -= 1;
                // Don't close wrapper - headings typically go with following block
                self.write(">\n")?;
            }
            TagEnd::Table => {
                self.write("</tbody></table>\n")?;
                self.block_depth -= 1;
                self.close_wrapper()?;
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
                // Close any deferred paragraph before closing blockquote
                // (footnotes inside blockquotes can't be sidenotes since def is outside)
                self.close_deferred_paragraph()?;
                self.write("</blockquote>\n")?;
                self.block_depth -= 1;
                self.close_wrapper()?;
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
                self.block_depth -= 1;
                self.close_wrapper()?;
            }
            TagEnd::List(true) => {
                self.write("</ol>\n")?;
                self.block_depth -= 1;
                self.close_wrapper()?;
            }
            TagEnd::List(false) => {
                self.write("</ul>\n")?;
                self.block_depth -= 1;
                self.close_wrapper()?;
            }
            TagEnd::Item => {
                self.write("</li>\n")?;
            }
            TagEnd::DefinitionList => {
                self.write("</dl>\n")?;
                self.block_depth -= 1;
                self.close_wrapper()?;
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
                eprintln!(
                    "[TagEnd::WeaverBlock] buffer: {:?}",
                    self.weaver_block_buffer
                );
                // Parse the buffered text for attrs and store for next block
                if !self.weaver_block_buffer.is_empty() {
                    let parsed = Self::parse_weaver_attrs(&self.weaver_block_buffer);
                    eprintln!("[TagEnd::WeaverBlock] parsed: {:?}", parsed);
                    self.weaver_block_buffer.clear();
                    // Merge with any existing pending attrs or set new
                    if let Some(ref mut existing) = self.pending_block_attrs {
                        existing.classes.extend(parsed.classes);
                        existing.attrs.extend(parsed.attrs);
                    } else {
                        self.pending_block_attrs = Some(parsed);
                    }
                    eprintln!(
                        "[TagEnd::WeaverBlock] pending_block_attrs now: {:?}",
                        self.pending_block_attrs
                    );
                }
            }
            TagEnd::FootnoteDefinition => {
                if self.in_sidenote {
                    self.write("</span>")?;
                    self.in_sidenote = false;
                    // Write any buffered content that came after the ref
                    if !self.pending_footnote_content.is_empty() {
                        let content = std::mem::take(&mut self.pending_footnote_content);
                        escape_html_body_text(&mut self.writer, &content)?;
                        self.end_newline = content.ends_with('\n');
                    }
                } else {
                    self.write("</div>\n")?;
                }
            }
            TagEnd::MetadataBlock(_) => {
                self.in_non_writing_block = false;
            }
        }
        Ok(())
    }
}

impl<
    'input,
    I: Iterator<Item = Event<'input>>,
    A: AgentSession + IdentityResolver + 'input,
    W: StrWrite,
> StaticPageWriter<'input, I, A, W>
{
    pub async fn run(mut self) -> Result<(), W::Error> {
        while let Some(event) = self.context.next().await {
            self.process_event(event).await?
        }
        self.finalize()
    }

    /// Finalize output, closing any deferred state
    fn finalize(&mut self) -> Result<(), W::Error> {
        // Flush any pending footnote as traditional
        self.flush_pending_footnote()?;
        // Close deferred paragraph if any
        if self.defer_paragraph_close {
            self.write("</p>\n")?;
            self.block_depth -= 1;
            self.close_wrapper()?;
            self.defer_paragraph_close = false;
        }
        Ok(())
    }

    async fn process_event(&mut self, event: Event<'input>) -> Result<(), W::Error> {
        use markdown_weaver::Event::*;
        match event {
            Start(tag) => {
                println!("Start tag: {:?}", tag);
                self.start_tag(tag).await?;
            }
            End(tag) => {
                self.end_tag(tag)?;
            }
            Text(text) => {
                // If buffering code, append to buffer instead of writing
                if let Some((_, ref mut buffer)) = self.code_buffer {
                    buffer.push_str(&text);
                } else if self.pending_footnote.is_some() {
                    // Buffer text while waiting to see if footnote def follows
                    self.pending_footnote_content.push_str(&text);
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
                let number = *self
                    .numbers
                    .entry(name.clone().into_static())
                    .or_insert(len);
                // Buffer this reference to see if definition follows immediately
                self.pending_footnote = Some((name.into_static(), number));
            }
            TaskListMarker(true) => {
                self.write("<input disabled=\"\" type=\"checkbox\" checked=\"\"/>\n")?;
            }
            TaskListMarker(false) => {
                self.write("<input disabled=\"\" type=\"checkbox\"/>\n")?;
            }
            WeaverBlock(text) => {
                // Buffer WeaverBlock content for parsing on End
                eprintln!("[WeaverBlock event] text: {:?}", text);
                self.weaver_block_buffer.push_str(&text);
            }
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
            Tag::Paragraph(_) => {
                if self.in_sidenote {
                    // Inside sidenote span - don't emit paragraph tags
                    Ok(())
                } else if self.defer_paragraph_close {
                    // We're continuing a virtual paragraph after a sidenote
                    // Don't emit <p> or increment block_depth (already counted)
                    // Clear defer flag - we'll set it again at end if another sidenote follows
                    self.defer_paragraph_close = false;
                    Ok(())
                } else {
                    self.flush_pending_footnote()?;
                    self.emit_wrapper_start()?;
                    self.block_depth += 1;
                    if self.end_newline {
                        self.write("<p>")
                    } else {
                        self.write("\n<p>")
                    }
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
                self.block_depth += 1;
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
                self.close_deferred_paragraph()?;
                self.emit_wrapper_start()?;
                self.block_depth += 1;
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
                self.close_deferred_paragraph()?;
                self.emit_wrapper_start()?;
                self.block_depth += 1;
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
                self.close_deferred_paragraph()?;
                self.emit_wrapper_start()?;
                self.block_depth += 1;
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
                self.block_depth += 1;
                if self.end_newline {
                    self.write("<ol>\n")
                } else {
                    self.write("\n<ol>\n")
                }
            }
            Tag::List(Some(start)) => {
                self.close_deferred_paragraph()?;
                self.emit_wrapper_start()?;
                self.block_depth += 1;
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
                self.block_depth += 1;
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
                self.block_depth += 1;
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
                //println!("Image tag {}", dest_url);
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
                //println!("Embed {:?}: {} - {}", embed_type, title, dest_url);
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
                    .map(|(n, _)| n.as_ref() == name.as_ref())
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

                    // Write any buffered content AFTER the sidenote span closes
                    // (we'll do this in end_tag)
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
                    let number = *self.numbers.entry(name.into_static()).or_insert(len);
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
