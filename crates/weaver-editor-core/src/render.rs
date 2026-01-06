//! Rendering traits for the editor.
//!
//! These traits abstract over external concerns during rendering:
//! - Resolving embed URLs to HTML content
//! - Resolving image URLs to CDN paths
//! - Validating wikilinks
//!
//! Implementations are provided by the consuming application (e.g., weaver-app).

/// Provides HTML content for embedded resources.
///
/// When rendering markdown with embeds (e.g., `![[at://...]]`), this trait
/// is consulted to get the pre-rendered HTML for the embed.
pub trait EmbedContentProvider {
    /// Get HTML content for an embed URL.
    ///
    /// Returns `Some(html)` if the embed content is available,
    /// `None` to render a placeholder.
    fn get_embed_html(&self, url: &str) -> Option<&str>;
}

/// Unit type implementation - no embeds available.
impl EmbedContentProvider for () {
    fn get_embed_html(&self, _url: &str) -> Option<&str> {
        None
    }
}

/// Resolves image URLs from markdown to actual paths.
///
/// Markdown may reference images by name (e.g., `/image/photo.jpg`).
/// This trait maps those to actual CDN URLs or data URLs.
pub trait ImageResolver {
    /// Resolve an image URL from markdown to an actual URL.
    ///
    /// Returns `Some(resolved_url)` if the image is found,
    /// `None` to use the original URL unchanged.
    fn resolve_image_url(&self, url: &str) -> Option<String>;
}

/// Unit type implementation - no image resolution.
impl ImageResolver for () {
    fn resolve_image_url(&self, _url: &str) -> Option<String> {
        None
    }
}

/// Validates wikilinks during rendering.
///
/// Used to add CSS classes indicating whether a wikilink target exists.
pub trait WikilinkValidator {
    /// Check if a wikilink target is valid (exists).
    fn is_valid_link(&self, target: &str) -> bool;
}

/// Unit type implementation - all links are valid.
impl WikilinkValidator for () {
    fn is_valid_link(&self, _target: &str) -> bool {
        true
    }
}

/// Reference implementations for common patterns.

impl<T: EmbedContentProvider> EmbedContentProvider for &T {
    fn get_embed_html(&self, url: &str) -> Option<&str> {
        (*self).get_embed_html(url)
    }
}

impl<T: ImageResolver> ImageResolver for &T {
    fn resolve_image_url(&self, url: &str) -> Option<String> {
        (*self).resolve_image_url(url)
    }
}

impl<T: WikilinkValidator> WikilinkValidator for &T {
    fn is_valid_link(&self, target: &str) -> bool {
        (*self).is_valid_link(target)
    }
}

impl<T: EmbedContentProvider> EmbedContentProvider for Option<T> {
    fn get_embed_html(&self, url: &str) -> Option<&str> {
        self.as_ref().and_then(|p| p.get_embed_html(url))
    }
}

impl<T: ImageResolver> ImageResolver for Option<T> {
    fn resolve_image_url(&self, url: &str) -> Option<String> {
        self.as_ref().and_then(|r| r.resolve_image_url(url))
    }
}

impl<T: WikilinkValidator> WikilinkValidator for Option<T> {
    fn is_valid_link(&self, target: &str) -> bool {
        self.as_ref().map(|v| v.is_valid_link(target)).unwrap_or(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestEmbedProvider;

    impl EmbedContentProvider for TestEmbedProvider {
        fn get_embed_html(&self, url: &str) -> Option<&str> {
            if url == "at://test/embed" {
                Some("<div>Test Embed</div>")
            } else {
                None
            }
        }
    }

    struct TestImageResolver;

    impl ImageResolver for TestImageResolver {
        fn resolve_image_url(&self, url: &str) -> Option<String> {
            if url.starts_with("/image/") {
                Some(format!("https://cdn.example.com{}", url))
            } else {
                None
            }
        }
    }

    struct TestWikilinkValidator {
        valid: Vec<String>,
    }

    impl WikilinkValidator for TestWikilinkValidator {
        fn is_valid_link(&self, target: &str) -> bool {
            self.valid.iter().any(|v| v == target)
        }
    }

    #[test]
    fn test_embed_provider() {
        let provider = TestEmbedProvider;
        assert_eq!(
            provider.get_embed_html("at://test/embed"),
            Some("<div>Test Embed</div>")
        );
        assert_eq!(provider.get_embed_html("at://other"), None);
    }

    #[test]
    fn test_image_resolver() {
        let resolver = TestImageResolver;
        assert_eq!(
            resolver.resolve_image_url("/image/photo.jpg"),
            Some("https://cdn.example.com/image/photo.jpg".to_string())
        );
        assert_eq!(resolver.resolve_image_url("https://other.com/img.png"), None);
    }

    #[test]
    fn test_wikilink_validator() {
        let validator = TestWikilinkValidator {
            valid: vec!["Home".to_string(), "About".to_string()],
        };
        assert!(validator.is_valid_link("Home"));
        assert!(validator.is_valid_link("About"));
        assert!(!validator.is_valid_link("Missing"));
    }

    #[test]
    fn test_unit_impls() {
        let embed: () = ();
        assert_eq!(embed.get_embed_html("anything"), None);

        let image: () = ();
        assert_eq!(image.resolve_image_url("anything"), None);

        let wiki: () = ();
        assert!(wiki.is_valid_link("anything")); // default true
    }

    #[test]
    fn test_option_impls() {
        let some_provider: Option<TestEmbedProvider> = Some(TestEmbedProvider);
        assert_eq!(
            some_provider.get_embed_html("at://test/embed"),
            Some("<div>Test Embed</div>")
        );

        let none_provider: Option<TestEmbedProvider> = None;
        assert_eq!(none_provider.get_embed_html("at://test/embed"), None);
    }
}
