//! OpenGraph image generation module
//!
//! Generates social card images for entry pages using SVG templates rendered to PNG.

use askama::Template;
use std::sync::OnceLock;
use std::time::Duration;

use crate::cache_impl::{Cache, new_cache};

/// Cache for generated OG images
/// Key: "{ident}/{book}/{entry}/{cid}" - includes CID for invalidation
static OG_CACHE: OnceLock<Cache<String, Vec<u8>>> = OnceLock::new();

fn get_cache() -> &'static Cache<String, Vec<u8>> {
    OG_CACHE.get_or_init(|| {
        // Cache up to 1000 images for 1 hour
        new_cache(1000, Duration::from_secs(3600))
    })
}

/// Generate cache key from entry identifiers
pub fn cache_key(ident: &str, book: &str, entry: &str, cid: &str) -> String {
    format!("{}/{}/{}/{}", ident, book, entry, cid)
}

/// Try to get a cached OG image
pub fn get_cached(key: &str) -> Option<Vec<u8>> {
    get_cache().get(&key.to_string())
}

/// Store an OG image in the cache
pub fn cache_image(key: String, image: Vec<u8>) {
    get_cache().insert(key, image);
}

/// Error type for OG image generation
#[derive(Debug)]
pub enum OgError {
    NotFound,
    FetchError(String),
    RenderError(String),
    TemplateError(String),
}

impl std::fmt::Display for OgError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OgError::NotFound => write!(f, "Entry not found"),
            OgError::FetchError(e) => write!(f, "Fetch error: {}", e),
            OgError::RenderError(e) => write!(f, "Render error: {}", e),
            OgError::TemplateError(e) => write!(f, "Template error: {}", e),
        }
    }
}

impl std::error::Error for OgError {}

/// Standard OG image dimensions
pub const OG_WIDTH: u32 = 1200;
pub const OG_HEIGHT: u32 = 630;

/// Rose Pine theme colors
mod colors {
    pub const BASE: &str = "#191724";
    pub const TEXT: &str = "#e0def4";
    pub const SUBTLE: &str = "#908caa";
    pub const MUTED: &str = "#6e6a86";
    pub const OVERLAY: &str = "#524f67";
}

/// Text-only template (no hero image)
#[derive(Template)]
#[template(path = "og_text_only.svg", escape = "none")]
pub struct TextOnlyTemplate {
    pub title_lines: Vec<String>,
    pub content_lines: Vec<String>,
    pub notebook_title: String,
    pub author_handle: String,
}

/// Hero image template (full-bleed image with overlay)
#[derive(Template)]
#[template(path = "og_hero_image.svg", escape = "none")]
pub struct HeroImageTemplate {
    pub hero_image_data: String,
    pub title_lines: Vec<String>,
    pub notebook_title: String,
    pub author_handle: String,
}

/// Global font database, initialized once
static FONTDB: OnceLock<fontdb::Database> = OnceLock::new();

fn get_fontdb() -> &'static fontdb::Database {
    FONTDB.get_or_init(|| {
        let mut db = fontdb::Database::new();
        // Load IBM Plex Sans from embedded bytes
        let font_data = include_bytes!("../../assets/fonts/IBMPlexSans-VariableFont_wdth,wght.ttf");
        db.load_font_data(font_data.to_vec());
        let font_data =
            include_bytes!("../../assets/fonts/ioskeley-mono/IoskeleyMono-Regular.woff2");
        db.load_font_data(font_data.to_vec());
        db
    })
}

/// Wrap title text into lines that fit the SVG width
pub fn wrap_title(title: &str, max_chars: usize, max_lines: usize) -> Vec<String> {
    textwrap::wrap(title, max_chars)
        .into_iter()
        .take(max_lines)
        .map(|s| s.to_string())
        .collect()
}

/// Render an SVG string to PNG bytes
pub fn render_svg_to_png(svg: &str) -> Result<Vec<u8>, OgError> {
    let fontdb = get_fontdb();

    let options = usvg::Options {
        fontdb: std::sync::Arc::new(fontdb.clone()),
        ..Default::default()
    };

    let tree = usvg::Tree::from_str(svg, &options)
        .map_err(|e| OgError::RenderError(format!("Failed to parse SVG: {}", e)))?;

    let mut pixmap = tiny_skia::Pixmap::new(OG_WIDTH, OG_HEIGHT)
        .ok_or_else(|| OgError::RenderError("Failed to create pixmap".to_string()))?;

    resvg::render(&tree, tiny_skia::Transform::default(), &mut pixmap.as_mut());

    pixmap
        .encode_png()
        .map_err(|e| OgError::RenderError(format!("Failed to encode PNG: {}", e)))
}

/// Generate a text-only OG image
pub fn generate_text_only(
    title: &str,
    content: &str,
    notebook_title: &str,
    author_handle: &str,
) -> Result<Vec<u8>, OgError> {
    let title_lines = wrap_title(title, 50, 2);
    let content_lines = wrap_title(content, 70, 5);

    let template = TextOnlyTemplate {
        title_lines,
        content_lines,
        notebook_title: notebook_title.to_string(),
        author_handle: author_handle.to_string(),
    };

    let svg = template
        .render()
        .map_err(|e| OgError::TemplateError(e.to_string()))?;

    render_svg_to_png(&svg)
}

/// Generate a hero image OG image
pub fn generate_hero_image(
    hero_image_data: &str,
    title: &str,
    notebook_title: &str,
    author_handle: &str,
) -> Result<Vec<u8>, OgError> {
    let title_lines = wrap_title(title, 50, 2);

    let template = HeroImageTemplate {
        hero_image_data: hero_image_data.to_string(),
        title_lines,
        notebook_title: notebook_title.to_string(),
        author_handle: author_handle.to_string(),
    };

    let svg = template
        .render()
        .map_err(|e| OgError::TemplateError(e.to_string()))?;

    render_svg_to_png(&svg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wrap_title_short() {
        let lines = wrap_title("Hello World", 28, 3);
        assert_eq!(lines, vec!["Hello World"]);
    }

    #[test]
    fn test_wrap_title_long() {
        let lines = wrap_title(
            "This is a very long title that should wrap onto multiple lines",
            28,
            3,
        );
        assert!(lines.len() > 1);
        assert!(lines.len() <= 3);
    }
}
