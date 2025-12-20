use core::fmt;
use std::ops::Range;

use jacquard::types::{ident::AtIdentifier, string::Rkey};
use markdown_weaver::{CowStr, EmbedType, Event, Tag};
use markdown_weaver_escape::{StrWrite, escape_html};
use weaver_common::ResolvedContent;

use crate::components::editor::{
    SyntaxSpanInfo, SyntaxType, document::EditorImage, writer::EditorWriter,
};

/// Synchronous callback for injecting embed content
///
/// Takes the embed tag and returns optional HTML content to inject.
pub trait EmbedContentProvider {
    fn get_embed_content(&self, tag: &Tag<'_>) -> Option<String>;
}

impl EmbedContentProvider for () {
    fn get_embed_content(&self, _tag: &Tag<'_>) -> Option<String> {
        None
    }
}

impl EmbedContentProvider for &ResolvedContent {
    fn get_embed_content(&self, tag: &Tag<'_>) -> Option<String> {
        if let Tag::Embed { dest_url, .. } = tag {
            let url = dest_url.as_ref();
            if url.starts_with("at://") {
                if let Ok(at_uri) = jacquard::types::string::AtUri::new(url) {
                    return ResolvedContent::get_embed_content(self, &at_uri)
                        .map(|s| s.to_string());
                }
            }
        }
        None
    }
}

/// Resolves image URLs to CDN URLs based on stored images.
///
/// The markdown may reference images by name (e.g., "photo.jpg" or "/notebook/image.png").
/// This trait maps those names to the actual CDN URL using the blob CID and owner DID.
pub trait ImageResolver {
    /// Resolve an image URL from markdown to a CDN URL.
    ///
    /// Returns `Some(cdn_url)` if the image is found, `None` to use the original URL.
    fn resolve_image_url(&self, url: &str) -> Option<String>;
}

impl ImageResolver for () {
    fn resolve_image_url(&self, _url: &str) -> Option<String> {
        None
    }
}

/// Concrete image resolver that maps image names to URLs.
///
/// Resolved image path type
#[derive(Clone, Debug)]
enum ResolvedImage {
    /// Data URL for immediate preview (still uploading)
    Pending(String),
    /// Draft image: `/image/{ident}/draft/{blob_rkey}/{name}`
    Draft {
        blob_rkey: Rkey<'static>,
        ident: AtIdentifier<'static>,
    },
    /// Published image: `/image/{ident}/{entry_rkey}/{name}`
    Published {
        entry_rkey: Rkey<'static>,
        ident: AtIdentifier<'static>,
    },
}

/// Resolves image paths in the editor.
///
/// Supports three states for images:
/// - Pending: uses data URL for immediate preview while upload is in progress
/// - Draft: uses path format `/image/{did}/draft/{blob_rkey}/{name}`
/// - Published: uses path format `/image/{did}/{entry_rkey}/{name}`
///
/// Image URLs in markdown use the format `/image/{name}`.
#[derive(Clone, Default)]
pub struct EditorImageResolver {
    /// All resolved images: name -> resolved path info
    images: std::collections::HashMap<String, ResolvedImage>,
}

impl EditorImageResolver {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a pending image with a data URL for immediate preview.
    pub fn add_pending(&mut self, name: String, data_url: String) {
        self.images.insert(name, ResolvedImage::Pending(data_url));
    }

    /// Promote a pending image to uploaded (draft) status.
    pub fn promote_to_uploaded(
        &mut self,
        name: &str,
        blob_rkey: Rkey<'static>,
        ident: AtIdentifier<'static>,
    ) {
        self.images
            .insert(name.to_string(), ResolvedImage::Draft { blob_rkey, ident });
    }

    /// Add an already-uploaded draft image.
    pub fn add_uploaded(
        &mut self,
        name: String,
        blob_rkey: Rkey<'static>,
        ident: AtIdentifier<'static>,
    ) {
        self.images
            .insert(name, ResolvedImage::Draft { blob_rkey, ident });
    }

    /// Add a published image.
    pub fn add_published(
        &mut self,
        name: String,
        entry_rkey: Rkey<'static>,
        ident: AtIdentifier<'static>,
    ) {
        self.images
            .insert(name, ResolvedImage::Published { entry_rkey, ident });
    }

    /// Check if an image is pending upload.
    pub fn is_pending(&self, name: &str) -> bool {
        matches!(self.images.get(name), Some(ResolvedImage::Pending(_)))
    }

    /// Build a resolver from editor images and user identifier.
    ///
    /// For draft mode (entry_rkey=None), only images with a `published_blob_uri` are included.
    /// For published mode (entry_rkey=Some), all images are included.
    pub fn from_images<'a>(
        images: impl IntoIterator<Item = &'a EditorImage>,
        ident: AtIdentifier<'static>,
        entry_rkey: Option<Rkey<'static>>,
    ) -> Self {
        use jacquard::IntoStatic;

        let mut resolver = Self::new();
        for editor_image in images {
            // Get the name from the Image (use alt text as fallback if name is empty)
            let name = editor_image
                .image
                .name
                .as_ref()
                .map(|n| n.to_string())
                .unwrap_or_else(|| editor_image.image.alt.to_string());

            if name.is_empty() {
                continue;
            }

            match &entry_rkey {
                // Published mode: use entry rkey for all images
                Some(rkey) => {
                    resolver.add_published(name, rkey.clone(), ident.clone());
                }
                // Draft mode: use published_blob_uri rkey
                None => {
                    let blob_rkey = match &editor_image.published_blob_uri {
                        Some(uri) => match uri.rkey() {
                            Some(rkey) => rkey.0.clone().into_static(),
                            None => continue,
                        },
                        None => continue,
                    };
                    resolver.add_uploaded(name, blob_rkey, ident.clone());
                }
            }
        }
        resolver
    }
}

impl ImageResolver for EditorImageResolver {
    fn resolve_image_url(&self, url: &str) -> Option<String> {
        // Extract image name from /image/{name} format
        let name = url.strip_prefix("/image/").unwrap_or(url);

        let resolved = self.images.get(name)?;
        match resolved {
            ResolvedImage::Pending(data_url) => Some(data_url.clone()),
            ResolvedImage::Draft { blob_rkey, ident } => {
                Some(format!("/image/{}/draft/{}/{}", ident, blob_rkey, name))
            }
            ResolvedImage::Published { entry_rkey, ident } => {
                Some(format!("/image/{}/{}/{}", ident, entry_rkey, name))
            }
        }
    }
}

impl ImageResolver for &EditorImageResolver {
    fn resolve_image_url(&self, url: &str) -> Option<String> {
        (*self).resolve_image_url(url)
    }
}

impl<'a, I: Iterator<Item = (Event<'a>, Range<usize>)>, E: EmbedContentProvider, R: ImageResolver>
    EditorWriter<'a, I, E, R>
{
    pub(crate) fn write_embed(
        &mut self,
        range: Range<usize>,
        embed_type: EmbedType,
        dest_url: CowStr<'_>,
        title: CowStr<'_>,
        id: CowStr<'_>,
        attrs: Option<markdown_weaver::WeaverAttributes<'_>>,
    ) -> Result<(), fmt::Error> {
        // Embed rendering: all syntax elements share one syn_id for visibility toggling
        // Structure: ![[  url-as-link  ]]  <embed-content>
        let raw_text = &self.source[range.clone()];
        let syn_id = self.gen_syn_id();
        let opening_char_start = self.last_char_offset;

        // Extract the URL from raw text (between ![[ and ]])
        let url_text = if raw_text.starts_with("![[") && raw_text.ends_with("]]") {
            &raw_text[3..raw_text.len() - 2]
        } else {
            dest_url.as_ref()
        };

        // Calculate char positions
        let url_char_len = url_text.chars().count();
        let opening_char_end = opening_char_start + 3; // "![["
        let url_char_start = opening_char_end;
        let url_char_end = url_char_start + url_char_len;
        let closing_char_start = url_char_end;
        let closing_char_end = closing_char_start + 2; // "]]"
        let formatted_range = opening_char_start..closing_char_end;

        // 1. Emit opening ![[ syntax span
        if raw_text.starts_with("![[") {
            write!(
                &mut self.writer,
                "<span class=\"md-syntax-inline\" data-syn-id=\"{}\" data-char-start=\"{}\" data-char-end=\"{}\" spellcheck=\"false\">![[</span>",
                syn_id, opening_char_start, opening_char_end
            )?;

            self.syntax_spans.push(SyntaxSpanInfo {
                syn_id: syn_id.clone(),
                char_range: opening_char_start..opening_char_end,
                syntax_type: SyntaxType::Inline,
                formatted_range: Some(formatted_range.clone()),
            });

            self.record_mapping(
                range.start..range.start + 3,
                opening_char_start..opening_char_end,
            );
        }

        // 2. Emit URL as a clickable link (same syn_id, shown/hidden with syntax)
        let url = dest_url.as_ref();
        let link_href = if url.starts_with("at://") {
            format!("https://alpha.weaver.sh/record/{}", url)
        } else {
            url.to_string()
        };

        write!(
            &mut self.writer,
            "<a class=\"image-alt embed-url\" href=\"{}\" data-syn-id=\"{}\" data-char-start=\"{}\" data-char-end=\"{}\" target=\"_blank\">",
            link_href, syn_id, url_char_start, url_char_end
        )?;
        escape_html(&mut self.writer, url_text)?;
        self.write("</a>")?;

        self.syntax_spans.push(SyntaxSpanInfo {
            syn_id: syn_id.clone(),
            char_range: url_char_start..url_char_end,
            syntax_type: SyntaxType::Inline,
            formatted_range: Some(formatted_range.clone()),
        });

        self.record_mapping(range.start + 3..range.end - 2, url_char_start..url_char_end);

        // 3. Emit closing ]] syntax span
        if raw_text.ends_with("]]") {
            write!(
                &mut self.writer,
                "<span class=\"md-syntax-inline\" data-syn-id=\"{}\" data-char-start=\"{}\" data-char-end=\"{}\" spellcheck=\"false\">]]</span>",
                syn_id, closing_char_start, closing_char_end
            )?;

            self.syntax_spans.push(SyntaxSpanInfo {
                syn_id: syn_id.clone(),
                char_range: closing_char_start..closing_char_end,
                syntax_type: SyntaxType::Inline,
                formatted_range: Some(formatted_range.clone()),
            });

            self.record_mapping(
                range.end - 2..range.end,
                closing_char_start..closing_char_end,
            );
        }

        // Collect AT URI for later resolution
        if url.starts_with("at://") || url.starts_with("did:") {
            self.ref_collector.add_at_embed(
                url,
                if title.is_empty() {
                    None
                } else {
                    Some(title.as_ref())
                },
            );
        }

        // 4. Emit the actual embed content
        // Try to get content from attributes first
        let content_from_attrs = if let Some(ref attrs) = attrs {
            attrs
                .attrs
                .iter()
                .find(|(k, _)| k.as_ref() == "content")
                .map(|(_, v)| v.as_ref().to_string())
        } else {
            None
        };

        // If no content in attrs, try provider
        let content = if let Some(content) = content_from_attrs {
            Some(content)
        } else if let Some(ref provider) = self.embed_provider {
            let tag = Tag::Embed {
                embed_type,
                dest_url: dest_url.clone(),
                title: title.clone(),
                id: id.clone(),
                attrs: attrs.clone(),
            };
            provider.get_embed_content(&tag)
        } else {
            None
        };

        if let Some(html_content) = content {
            // Write the pre-rendered content directly
            self.write(&html_content)?;
        } else {
            // Fallback: render as placeholder div (iframe doesn't make sense for at:// URIs)
            self.write("<div class=\"atproto-embed atproto-embed-placeholder\">")?;
            self.write("<span class=\"embed-loading\">Loading embed...</span>")?;
            self.write("</div>")?;
        }

        // Consume the text events for the URL (they're still in the iterator)
        // Use consume_until_end() since we already wrote the URL from source
        self.consume_until_end();

        // Update offsets
        self.last_char_offset = closing_char_end;
        self.last_byte_offset = range.end;

        Ok(())
    }
}
