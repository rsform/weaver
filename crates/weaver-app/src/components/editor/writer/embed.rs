//! App-specific image resolver for the editor.
//!
//! Provides EditorImageResolver which maps image names to URLs based on
//! image state (pending upload, draft, published).

use jacquard::types::{ident::AtIdentifier, string::Rkey};
use weaver_editor_core::ImageResolver;

use crate::components::editor::document::EditorImage;

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

