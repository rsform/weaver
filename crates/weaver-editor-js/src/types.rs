//! Types exposed to JavaScript via wasm-bindgen.

use serde::{Deserialize, Serialize};
use tsify_next::Tsify;
use wasm_bindgen::prelude::*;

/// Pending image waiting for upload.
#[derive(Debug, Clone, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct PendingImage {
    pub local_id: String,
    #[tsify(type = "Uint8Array")]
    #[serde(with = "serde_bytes")]
    pub data: Vec<u8>,
    pub mime_type: String,
    pub name: String,
}

/// Finalized image with blob ref and staging URI.
#[derive(Debug, Clone, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct FinalizedImage {
    pub blob_ref: JsBlobRef,
    /// AT URI of the staging record (sh.weaver.publish.blob).
    pub staging_uri: String,
}

/// Blob reference matching AT Protocol blob format.
#[derive(Debug, Clone, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct JsBlobRef {
    #[serde(rename = "$type")]
    pub type_marker: String, // "blob"
    pub r#ref: BlobLink,
    pub mime_type: String,
    pub size: u64,
}

/// CID link for blob.
#[derive(Debug, Clone, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct BlobLink {
    #[serde(rename = "$link")]
    pub link: String,
}

/// Entry JSON matching sh.weaver.notebook.entry lexicon.
///
/// Used for snapshots (drafts) and final entry output.
#[derive(Debug, Clone, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct EntryJson {
    pub title: String,
    pub path: String,
    pub content: String,
    pub created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub embeds: Option<EntryEmbeds>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authors: Option<Vec<Author>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_warnings: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rating: Option<String>,
}

/// Entry embeds container.
#[derive(Debug, Clone, Default, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct EntryEmbeds {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub images: Option<ImagesEmbed>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub records: Option<RecordsEmbed>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub externals: Option<ExternalsEmbed>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub videos: Option<VideosEmbed>,
}

/// Image embed container.
#[derive(Debug, Clone, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct ImagesEmbed {
    pub images: Vec<ImageEmbed>,
}

/// Single image embed.
#[derive(Debug, Clone, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct ImageEmbed {
    pub image: JsBlobRef,
    pub alt: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aspect_ratio: Option<AspectRatio>,
}

/// Aspect ratio for images/videos.
#[derive(Debug, Clone, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct AspectRatio {
    pub width: u32,
    pub height: u32,
}

/// Record embed container.
#[derive(Debug, Clone, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct RecordsEmbed {
    pub records: Vec<RecordEmbed>,
}

/// Single record embed (strong ref).
#[derive(Debug, Clone, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct RecordEmbed {
    pub uri: String,
    pub cid: String,
}

/// External link embed container.
#[derive(Debug, Clone, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct ExternalsEmbed {
    pub externals: Vec<ExternalEmbed>,
}

/// Single external link embed.
#[derive(Debug, Clone, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct ExternalEmbed {
    pub uri: String,
    pub title: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thumb: Option<JsBlobRef>,
}

/// Video embed container.
#[derive(Debug, Clone, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct VideosEmbed {
    pub videos: Vec<VideoEmbed>,
}

/// Single video embed.
#[derive(Debug, Clone, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct VideoEmbed {
    pub video: JsBlobRef,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aspect_ratio: Option<AspectRatio>,
}

/// Author reference.
#[derive(Debug, Clone, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct Author {
    pub did: String,
}

/// Pre-rendered embed content for initial load.
#[wasm_bindgen]
pub struct JsResolvedContent {
    inner: weaver_common::ResolvedContent,
}

#[wasm_bindgen]
impl JsResolvedContent {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            inner: weaver_common::ResolvedContent::new(),
        }
    }

    /// Add pre-rendered HTML for an AT URI.
    #[wasm_bindgen(js_name = addEmbed)]
    pub fn add_embed(&mut self, at_uri: &str, html: &str) -> Result<(), JsError> {
        use weaver_common::jacquard::{CowStr, IntoStatic, types::string::AtUri};

        let uri = AtUri::new(at_uri)
            .map_err(|e| JsError::new(&format!("Invalid AT URI: {}", e)))?
            .into_static();

        self.inner
            .add_embed(uri, CowStr::from(html.to_string()), None);
        Ok(())
    }
}

impl Default for JsResolvedContent {
    fn default() -> Self {
        Self::new()
    }
}

impl JsResolvedContent {
    pub fn into_inner(self) -> weaver_common::ResolvedContent {
        self.inner
    }

    pub fn inner_ref(&self) -> &weaver_common::ResolvedContent {
        &self.inner
    }
}

/// Create an empty resolved content container.
#[wasm_bindgen]
pub fn create_resolved_content() -> JsResolvedContent {
    JsResolvedContent::new()
}
