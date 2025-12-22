mod html_output;
mod markdown_output;
mod processor;
mod types;

pub use html_output::{render_faceted_html, HtmlFacetOutput};
pub use markdown_output::{render_faceted_markdown, MarkdownFacetOutput};
pub use processor::process_faceted_text;
pub use types::{ByteRange, FacetFeature, NormalizedFacet};

pub trait FacetOutput {
    type Error;

    fn write_text(&mut self, text: &str) -> Result<(), Self::Error>;
    fn start_feature(&mut self, feature: &FacetFeature<'_>) -> Result<(), Self::Error>;
    fn end_feature(&mut self, feature: &FacetFeature<'_>) -> Result<(), Self::Error>;
}
