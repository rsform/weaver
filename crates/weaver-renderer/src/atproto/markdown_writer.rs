use markdown_weaver::{Event, Tag, TagEnd, CowStr};
use markdown_weaver_escape::StrWrite;

/// Writes markdown events back to markdown text
pub struct MarkdownWriter<W: StrWrite> {
    writer: W,
    in_list: bool,
    list_depth: usize,
    current_link_url: Option<CowStr<'static>>,
    current_link_title: Option<CowStr<'static>>,
}

impl<W: StrWrite> MarkdownWriter<W> {
    pub fn new(writer: W) -> Self {
        Self {
            writer,
            in_list: false,
            list_depth: 0,
            current_link_url: None,
            current_link_title: None,
        }
    }

    pub fn write_event(&mut self, event: Event<'_>) -> Result<(), W::Error> {
        match event {
            Event::Start(tag) => self.start_tag(tag),
            Event::End(tag) => self.end_tag(tag),
            Event::Text(text) => write!(self.writer, "{}", text),
            Event::Code(code) => write!(self.writer, "`{}`", code),
            Event::Html(html) => write!(self.writer, "{}", html),
            Event::InlineHtml(html) => write!(self.writer, "{}", html),
            Event::SoftBreak => write!(self.writer, "\n"),
            Event::HardBreak => write!(self.writer, "  \n"),
            Event::Rule => write!(self.writer, "\n---\n\n"),
            Event::InlineMath(math) => write!(self.writer, "${}$", math),
            Event::DisplayMath(math) => write!(self.writer, "\n$$\n{}\n$$\n", math),
            Event::FootnoteReference(name) => write!(self.writer, "[^{}]", name),
            Event::TaskListMarker(checked) => {
                if checked {
                    write!(self.writer, "[x] ")
                } else {
                    write!(self.writer, "[ ] ")
                }
            }
            _ => Ok(()),
        }
    }

    fn start_tag(&mut self, tag: Tag<'_>) -> Result<(), W::Error> {
        match tag {
            Tag::Paragraph(_) => Ok(()),
            Tag::Heading { level, .. } => {
                write!(self.writer, "{} ", "#".repeat(level as usize))
            }
            Tag::BlockQuote(_) => write!(self.writer, "> "),
            Tag::CodeBlock(kind) => {
                match kind {
                    markdown_weaver::CodeBlockKind::Fenced(lang) => {
                        write!(self.writer, "\n```{}\n", lang)
                    }
                    markdown_weaver::CodeBlockKind::Indented => {
                        write!(self.writer, "\n    ")
                    }
                }
            }
            Tag::List(_) => {
                self.in_list = true;
                self.list_depth += 1;
                Ok(())
            }
            Tag::Item => {
                let indent = "  ".repeat(self.list_depth.saturating_sub(1));
                write!(self.writer, "{}* ", indent)
            }
            Tag::Link { dest_url, title, .. } => {
                self.current_link_url = Some(dest_url.into_static());
                self.current_link_title = if title.is_empty() {
                    None
                } else {
                    Some(title.into_static())
                };
                write!(self.writer, "[")
            }
            Tag::Image { dest_url, title, .. } => {
                self.current_link_url = Some(dest_url.into_static());
                self.current_link_title = if title.is_empty() {
                    None
                } else {
                    Some(title.into_static())
                };
                write!(self.writer, "![")
            }
            Tag::Embed { dest_url, title, .. } => {
                self.current_link_url = Some(dest_url.into_static());
                self.current_link_title = if title.is_empty() {
                    None
                } else {
                    Some(title.into_static())
                };
                write!(self.writer, "![")
            }
            Tag::Emphasis => write!(self.writer, "*"),
            Tag::Strong => write!(self.writer, "**"),
            Tag::Strikethrough => write!(self.writer, "~~"),
            Tag::Table(_) => write!(self.writer, "\n"),
            _ => Ok(()),
        }
    }

    fn end_tag(&mut self, tag: TagEnd) -> Result<(), W::Error> {
        match tag {
            TagEnd::Paragraph(_) => write!(self.writer, "\n\n"),
            TagEnd::Heading(_) => write!(self.writer, "\n\n"),
            TagEnd::BlockQuote(_) => write!(self.writer, "\n\n"),
            TagEnd::CodeBlock => write!(self.writer, "```\n\n"),
            TagEnd::List(_) => {
                self.list_depth = self.list_depth.saturating_sub(1);
                if self.list_depth == 0 {
                    self.in_list = false;
                    write!(self.writer, "\n")
                } else {
                    Ok(())
                }
            }
            TagEnd::Item => write!(self.writer, "\n"),
            TagEnd::Link => {
                let url = self.current_link_url.take().unwrap_or(CowStr::Borrowed(""));
                if let Some(title) = self.current_link_title.take() {
                    write!(self.writer, "]({} \"{}\")", url, title)
                } else {
                    write!(self.writer, "]({})", url)
                }
            }
            TagEnd::Image => {
                let url = self.current_link_url.take().unwrap_or(CowStr::Borrowed(""));
                if let Some(title) = self.current_link_title.take() {
                    write!(self.writer, "]({} \"{}\")", url, title)
                } else {
                    write!(self.writer, "]({})", url)
                }
            }
            TagEnd::Embed => {
                let url = self.current_link_url.take().unwrap_or(CowStr::Borrowed(""));
                if let Some(title) = self.current_link_title.take() {
                    write!(self.writer, "]({} \"{}\")", url, title)
                } else {
                    write!(self.writer, "]({})", url)
                }
            }
            TagEnd::Emphasis => write!(self.writer, "*"),
            TagEnd::Strong => write!(self.writer, "**"),
            TagEnd::Strikethrough => write!(self.writer, "~~"),
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use markdown_weaver::{Event, Tag, CowStr, ParagraphContext};
    use markdown_weaver_escape::FmtWriter;

    #[test]
    fn test_write_paragraph() {
        let mut output = String::new();
        let mut writer = MarkdownWriter::new(FmtWriter(&mut output));

        writer.write_event(Event::Start(Tag::Paragraph(ParagraphContext::Complete))).unwrap();
        writer.write_event(Event::Text(CowStr::Borrowed("Hello"))).unwrap();
        writer.write_event(Event::End(markdown_weaver::TagEnd::Paragraph(ParagraphContext::Complete))).unwrap();

        assert_eq!(output, "Hello\n\n");
    }

    #[test]
    fn test_write_heading() {
        let mut output = String::new();
        let mut writer = MarkdownWriter::new(FmtWriter(&mut output));

        writer.write_event(Event::Start(Tag::Heading {
            level: markdown_weaver::HeadingLevel::H2,
            id: None,
            classes: vec![],
            attrs: vec![],
        })).unwrap();
        writer.write_event(Event::Text(CowStr::Borrowed("Title"))).unwrap();
        writer.write_event(Event::End(markdown_weaver::TagEnd::Heading(markdown_weaver::HeadingLevel::H2))).unwrap();

        assert_eq!(output, "## Title\n\n");
    }

    #[test]
    fn test_write_code() {
        let mut output = String::new();
        let mut writer = MarkdownWriter::new(FmtWriter(&mut output));

        writer.write_event(Event::Code(CowStr::Borrowed("let x = 5;"))).unwrap();

        assert_eq!(output, "`let x = 5;`");
    }

    #[test]
    fn test_write_link() {
        let mut output = String::new();
        let mut writer = MarkdownWriter::new(FmtWriter(&mut output));

        writer.write_event(Event::Start(Tag::Link {
            link_type: markdown_weaver::LinkType::Inline,
            dest_url: CowStr::Borrowed("/path/to/page"),
            title: CowStr::Borrowed(""),
            id: CowStr::Borrowed(""),
        })).unwrap();
        writer.write_event(Event::Text(CowStr::Borrowed("Link text"))).unwrap();
        writer.write_event(Event::End(markdown_weaver::TagEnd::Link)).unwrap();

        assert_eq!(output, "[Link text](/path/to/page)");
    }

    #[test]
    fn test_write_link_with_title() {
        let mut output = String::new();
        let mut writer = MarkdownWriter::new(FmtWriter(&mut output));

        writer.write_event(Event::Start(Tag::Link {
            link_type: markdown_weaver::LinkType::Inline,
            dest_url: CowStr::Borrowed("/path"),
            title: CowStr::Borrowed("Hover tooltip"),  // The quoted "title" attribute
            id: CowStr::Borrowed(""),
        })).unwrap();
        writer.write_event(Event::Text(CowStr::Borrowed("link text"))).unwrap();
        writer.write_event(Event::End(markdown_weaver::TagEnd::Link)).unwrap();

        assert_eq!(output, "[link text](/path \"Hover tooltip\")");
    }

    #[test]
    fn test_write_image() {
        let mut output = String::new();
        let mut writer = MarkdownWriter::new(FmtWriter(&mut output));

        writer.write_event(Event::Start(Tag::Image {
            link_type: markdown_weaver::LinkType::Inline,
            dest_url: CowStr::Borrowed("/image.png"),
            title: CowStr::Borrowed("Hover tooltip"),  // The quoted "title" attribute
            id: CowStr::Borrowed(""),
            attrs: None,
        })).unwrap();
        writer.write_event(Event::Text(CowStr::Borrowed("Alt text in brackets"))).unwrap();
        writer.write_event(Event::End(markdown_weaver::TagEnd::Image)).unwrap();

        assert_eq!(output, "![Alt text in brackets](/image.png \"Hover tooltip\")");
    }
}
