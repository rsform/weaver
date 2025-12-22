use super::{FacetFeature, FacetOutput};
use std::fmt::Write;

pub struct HtmlFacetOutput<W: Write> {
    writer: W,
}

impl<W: Write> HtmlFacetOutput<W> {
    pub fn new(writer: W) -> Self {
        Self { writer }
    }

    pub fn into_inner(self) -> W {
        self.writer
    }
}

impl<W: Write> FacetOutput for HtmlFacetOutput<W> {
    type Error = std::fmt::Error;

    fn write_text(&mut self, text: &str) -> Result<(), Self::Error> {
        for c in text.chars() {
            match c {
                '&' => self.writer.write_str("&amp;")?,
                '<' => self.writer.write_str("&lt;")?,
                '>' => self.writer.write_str("&gt;")?,
                _ => self.writer.write_char(c)?,
            }
        }
        Ok(())
    }

    fn start_feature(&mut self, feature: &FacetFeature<'_>) -> Result<(), Self::Error> {
        match feature {
            FacetFeature::Bold => write!(self.writer, "<strong>"),
            FacetFeature::Italic => write!(self.writer, "<em>"),
            FacetFeature::Code => write!(self.writer, "<code>"),
            FacetFeature::Underline => write!(self.writer, "<u>"),
            FacetFeature::Strikethrough => write!(self.writer, "<s>"),
            FacetFeature::Highlight => write!(self.writer, "<mark>"),
            FacetFeature::Link { uri } => {
                write!(self.writer, "<a href=\"")?;
                for c in uri.chars() {
                    match c {
                        '"' => self.writer.write_str("%22")?,
                        _ => self.writer.write_char(c)?,
                    }
                }
                write!(self.writer, "\">")
            }
            FacetFeature::DidMention { did } => {
                write!(
                    self.writer,
                    "<a class=\"mention\" href=\"https://bsky.app/profile/{}\">",
                    did
                )
            }
            FacetFeature::AtMention { at_uri } => {
                write!(self.writer, "<a class=\"at-mention\" href=\"{}\">", at_uri)
            }
            FacetFeature::Tag { tag } => {
                write!(
                    self.writer,
                    "<a class=\"hashtag\" href=\"https://bsky.app/hashtag/{}\">",
                    tag
                )
            }
            FacetFeature::Id { id } => {
                if let Some(id) = id {
                    write!(self.writer, "<span id=\"{}\">", id)
                } else {
                    write!(self.writer, "<span>")
                }
            }
        }
    }

    fn end_feature(&mut self, feature: &FacetFeature<'_>) -> Result<(), Self::Error> {
        match feature {
            FacetFeature::Bold => write!(self.writer, "</strong>"),
            FacetFeature::Italic => write!(self.writer, "</em>"),
            FacetFeature::Code => write!(self.writer, "</code>"),
            FacetFeature::Underline => write!(self.writer, "</u>"),
            FacetFeature::Strikethrough => write!(self.writer, "</s>"),
            FacetFeature::Highlight => write!(self.writer, "</mark>"),
            FacetFeature::Link { .. }
            | FacetFeature::DidMention { .. }
            | FacetFeature::AtMention { .. }
            | FacetFeature::Tag { .. } => write!(self.writer, "</a>"),
            FacetFeature::Id { .. } => write!(self.writer, "</span>"),
        }
    }
}

pub fn render_faceted_html(
    text: &str,
    facets: &[super::NormalizedFacet<'_>],
) -> Result<String, std::fmt::Error> {
    let mut output = HtmlFacetOutput::new(String::new());
    super::process_faceted_text(text, facets, &mut output)?;
    Ok(output.into_inner())
}
