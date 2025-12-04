//! OpenGraph image generation module
//!
//! Generates social card images for entry pages using SVG templates rendered to PNG.
pub mod server;

use crate::cache_impl::{Cache, new_cache};
use askama::Template;
use std::sync::OnceLock;
use std::time::Duration;

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

/// Notebook index template
#[derive(Template)]
#[template(path = "og_notebook.svg", escape = "none")]
pub struct NotebookTemplate {
    pub title_lines: Vec<String>,
    pub author_handle: String,
    pub entry_count: usize,
    pub entry_titles: Vec<String>,
}

/// Profile template (text-only, no banner)
#[derive(Template)]
#[template(path = "og_profile.svg", escape = "none")]
pub struct ProfileTemplate {
    pub avatar_data: Option<String>,
    pub display_name_lines: Vec<String>,
    pub handle: String,
    pub bio_lines: Vec<String>,
    pub notebook_count: usize,
}

/// Profile template with banner image
#[derive(Template)]
#[template(path = "og_profile_banner.svg", escape = "none")]
pub struct ProfileBannerTemplate {
    pub banner_image_data: String,
    pub avatar_data: Option<String>,
    pub display_name_lines: Vec<String>,
    pub handle: String,
    pub bio_lines: Vec<String>,
    pub notebook_count: usize,
}

/// Site homepage template
#[derive(Template)]
#[template(path = "og_site.svg", escape = "none")]
pub struct SiteTemplate {}

/// Global font database, initialized once
static FONTDB: OnceLock<fontdb::Database> = OnceLock::new();

fn get_fontdb() -> &'static fontdb::Database {
    FONTDB.get_or_init(|| {
        let mut db = fontdb::Database::new();
        // Load IBM Plex Sans from embedded bytes
        let font_data = include_bytes!("../../assets/fonts/IBMPlexSans-VariableFont_wdth,wght.ttf");
        db.load_font_data(font_data.to_vec());
        // Load IBM Plex Sans Bold (static weight for proper bold rendering)
        let font_data = include_bytes!("../../assets/fonts/IBMPlexSans-Bold.ttf");
        db.load_font_data(font_data.to_vec());
        let font_data = include_bytes!("../../assets/fonts/ioskeley-mono/IoskeleyMono-Regular.ttf");
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

/// Generate cache key for notebook OG images
pub fn notebook_cache_key(ident: &str, book: &str, cid: &str) -> String {
    format!("notebook/{}/{}/{}", ident, book, cid)
}

/// Generate cache key for profile OG images
pub fn profile_cache_key(ident: &str, cid: &str) -> String {
    format!("profile/{}/{}", ident, cid)
}

/// Generate a notebook index OG image
pub fn generate_notebook_og(
    title: &str,
    author_handle: &str,
    entry_count: usize,
    entry_titles: Vec<String>,
) -> Result<Vec<u8>, OgError> {
    let title_lines = wrap_title(title, 40, 2);
    // Limit to first 4 entries, truncate long titles
    let entry_titles: Vec<String> = entry_titles
        .into_iter()
        .take(4)
        .map(|t| {
            if t.len() > 60 {
                format!("{}...", &t[..57])
            } else {
                t
            }
        })
        .collect();

    let template = NotebookTemplate {
        title_lines,
        author_handle: author_handle.to_string(),
        entry_count,
        entry_titles,
    };

    let svg = template
        .render()
        .map_err(|e| OgError::TemplateError(e.to_string()))?;

    render_svg_to_png(&svg)
}

/// Generate a profile OG image (text-only version)
pub fn generate_profile_og(
    display_name: &str,
    handle: &str,
    bio: &str,
    avatar_data: Option<String>,
    notebook_count: usize,
) -> Result<Vec<u8>, OgError> {
    let display_name_lines = wrap_title(display_name, 30, 2);
    let bio_lines = wrap_title(bio, 60, 4);

    let template = ProfileTemplate {
        avatar_data,
        display_name_lines,
        handle: handle.to_string(),
        bio_lines,
        notebook_count,
    };

    let svg = template
        .render()
        .map_err(|e| OgError::TemplateError(e.to_string()))?;

    render_svg_to_png(&svg)
}

/// Generate a profile OG image with banner
pub fn generate_profile_banner_og(
    display_name: &str,
    handle: &str,
    bio: &str,
    banner_image_data: String,
    avatar_data: Option<String>,
    notebook_count: usize,
) -> Result<Vec<u8>, OgError> {
    let display_name_lines = wrap_title(display_name, 25, 1);
    let bio_lines = wrap_title(bio, 70, 1);

    let template = ProfileBannerTemplate {
        banner_image_data,
        avatar_data,
        display_name_lines,
        handle: handle.to_string(),
        bio_lines,
        notebook_count,
    };

    let svg = template
        .render()
        .map_err(|e| OgError::TemplateError(e.to_string()))?;

    render_svg_to_png(&svg)
}

/// Generate site homepage OG image
pub fn generate_site_og() -> Result<Vec<u8>, OgError> {
    let template = SiteTemplate {};

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
