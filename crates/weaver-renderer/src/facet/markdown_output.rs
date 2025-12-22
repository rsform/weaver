use super::{FacetFeature, FacetOutput};
use std::fmt::Write;

pub struct MarkdownFacetOutput<W: Write> {
    writer: W,
}

impl<W: Write> MarkdownFacetOutput<W> {
    pub fn new(writer: W) -> Self {
        Self { writer }
    }

    pub fn into_inner(self) -> W {
        self.writer
    }
}

impl<W: Write> FacetOutput for MarkdownFacetOutput<W> {
    type Error = std::fmt::Error;

    fn write_text(&mut self, text: &str) -> Result<(), Self::Error> {
        self.writer.write_str(text)
    }

    fn start_feature(&mut self, feature: &FacetFeature<'_>) -> Result<(), Self::Error> {
        match feature {
            FacetFeature::Bold => write!(self.writer, "**"),
            FacetFeature::Italic => write!(self.writer, "_"),
            FacetFeature::Code => write!(self.writer, "`"),
            FacetFeature::Strikethrough => write!(self.writer, "~~"),
            FacetFeature::Highlight => write!(self.writer, "=="),
            FacetFeature::Link { .. }
            | FacetFeature::DidMention { .. }
            | FacetFeature::AtMention { .. }
            | FacetFeature::Tag { .. } => write!(self.writer, "["),
            // No markdown equivalent
            FacetFeature::Underline | FacetFeature::Id { .. } => Ok(()),
        }
    }

    fn end_feature(&mut self, feature: &FacetFeature<'_>) -> Result<(), Self::Error> {
        match feature {
            FacetFeature::Bold => write!(self.writer, "**"),
            FacetFeature::Italic => write!(self.writer, "_"),
            FacetFeature::Code => write!(self.writer, "`"),
            FacetFeature::Strikethrough => write!(self.writer, "~~"),
            FacetFeature::Highlight => write!(self.writer, "=="),
            FacetFeature::Link { uri } => write!(self.writer, "]({})", uri),
            FacetFeature::DidMention { did } => {
                write!(self.writer, "](https://bsky.app/profile/{})", did)
            }
            FacetFeature::AtMention { at_uri } => write!(self.writer, "]({})", at_uri),
            FacetFeature::Tag { tag } => {
                write!(self.writer, "](https://bsky.app/hashtag/{})", tag)
            }
            // No markdown equivalent
            FacetFeature::Underline | FacetFeature::Id { .. } => Ok(()),
        }
    }
}

pub fn render_faceted_markdown(
    text: &str,
    facets: &[super::NormalizedFacet<'_>],
) -> Result<String, std::fmt::Error> {
    let mut output = MarkdownFacetOutput::new(String::new());
    super::process_faceted_text(text, facets, &mut output)?;
    Ok(output.into_inner())
}
