//! WASM bindings for weaver-renderer.
//!
//! Exposes sync rendering functions to JS/TS apps via wasm-bindgen.

use jacquard::types::string::AtUri;
use jacquard::types::value::Data;
use serde::Deserialize;
use serde_wasm_bindgen::Deserializer;
use wasm_bindgen::prelude::*;

mod types;

pub use types::*;

/// Initialize panic hook for better error messages in console.
#[wasm_bindgen(start)]
pub fn init() {
    console_error_panic_hook::set_once();
}

/// Render an AT Protocol record as HTML.
///
/// Takes a record URI and the record data (typically fetched from an appview).
/// Returns the rendered HTML string.
///
/// # Arguments
/// * `at_uri` - The AT Protocol URI (e.g., "at://did:plc:.../app.bsky.feed.post/...")
/// * `record_json` - The record data as JSON
/// * `fallback_author` - Optional author profile for records that don't include author info
/// * `resolved_content` - Optional pre-rendered embed content
#[wasm_bindgen]
pub fn render_record(
    at_uri: &str,
    record_json: JsValue,
    fallback_author: Option<JsValue>,
    resolved_content: Option<JsResolvedContent>,
) -> Result<String, JsError> {
    let uri = AtUri::new(at_uri).map_err(|e| JsError::new(&format!("Invalid AT URI: {}", e)))?;

    // Use Deserializer directly to avoid DeserializeOwned bounds (breaks Jacquard types).
    let deserializer = Deserializer::from(record_json);
    let data = Data::deserialize(deserializer)
        .map_err(|e| JsError::new(&format!("Invalid record JSON: {}", e)))?;

    let author: Option<weaver_api::sh_weaver::actor::ProfileDataView<'_>> = fallback_author
        .map(|v| {
            let deserializer = Deserializer::from(v);
            weaver_api::sh_weaver::actor::ProfileDataView::deserialize(deserializer)
        })
        .transpose()
        .map_err(|e| JsError::new(&format!("Invalid author JSON: {}", e)))?;

    let resolved = resolved_content.map(|r| r.into_inner());

    weaver_renderer::atproto::render_record(&uri, &data, author.as_ref(), resolved.as_ref())
        .map_err(|e| JsError::new(&e.to_string()))
}

/// Render markdown to HTML.
///
/// # Arguments
/// * `markdown` - The markdown source text
/// * `resolved_content` - Optional pre-rendered embed content
#[wasm_bindgen]
pub fn render_markdown(
    markdown: &str,
    resolved_content: Option<JsResolvedContent>,
) -> Result<String, JsError> {
    use weaver_renderer::atproto::ClientWriter;

    let resolved = resolved_content.map(|r| r.into_inner()).unwrap_or_default();

    let parser = markdown_weaver::Parser::new_ext(markdown, weaver_renderer::default_md_options())
        .into_offset_iter();
    let events: Vec<_> = parser.collect();

    let mut html_buf = String::new();
    let writer = ClientWriter::new(events.into_iter(), &mut html_buf, markdown)
        .with_embed_provider(&resolved);

    writer.run().map_err(|_| JsError::new("Render error"))?;

    Ok(html_buf)
}

/// Render LaTeX math to MathML.
///
/// # Arguments
/// * `latex` - The LaTeX math expression
/// * `display_mode` - true for display math (block), false for inline math
#[wasm_bindgen]
pub fn render_math(latex: &str, display_mode: bool) -> JsMathResult {
    match weaver_renderer::math::render_math(latex, display_mode) {
        weaver_renderer::math::MathResult::Success(html) => JsMathResult {
            success: true,
            html,
            error: None,
        },
        weaver_renderer::math::MathResult::Error { html, message } => JsMathResult {
            success: false,
            html,
            error: Some(message),
        },
    }
}

/// Render faceted text (rich text with mentions, links, etc.) to HTML.
///
/// Accepts facets from several AT Protocol lexicons (app.bsky, pub.leaflet, blog.pckt).
///
/// # Arguments
/// * `text` - The plain text content
/// * `facets_json` - Array of facets with `index` (byteStart/byteEnd) and `features` array
#[wasm_bindgen]
pub fn render_faceted_text(text: &str, facets_json: JsValue) -> Result<String, JsError> {
    use weaver_renderer::facet::NormalizedFacet;

    let deserializer = Deserializer::from(facets_json);
    let facets = Vec::<NormalizedFacet<'_>>::deserialize(deserializer)
        .map_err(|e| JsError::new(&format!("Invalid facets JSON: {}", e)))?;

    weaver_renderer::facet::render_faceted_html(text, &facets)
        .map_err(|e| JsError::new(&e.to_string()))
}
