//! Types exposed to JavaScript via wasm-bindgen.

use jacquard::IntoStatic;
use wasm_bindgen::prelude::*;
use weaver_common::ResolvedContent;

/// Result from rendering LaTeX math.
#[wasm_bindgen]
pub struct JsMathResult {
    pub success: bool,
    #[wasm_bindgen(getter_with_clone)]
    pub html: String,
    #[wasm_bindgen(getter_with_clone)]
    pub error: Option<String>,
}

/// Pre-rendered embed content for synchronous rendering.
///
/// Build this by calling `create_resolved_content()` and adding embeds
/// with `resolved_content_add_embed()`.
#[wasm_bindgen]
pub struct JsResolvedContent {
    inner: ResolvedContent,
}

#[wasm_bindgen]
impl JsResolvedContent {
    /// Create an empty resolved content container.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            inner: ResolvedContent::new(),
        }
    }

    /// Add pre-rendered embed HTML for an AT URI.
    ///
    /// # Arguments
    /// * `at_uri` - The AT Protocol URI (e.g., "at://did:plc:.../app.bsky.feed.post/...")
    /// * `html` - The pre-rendered HTML for this embed
    #[wasm_bindgen(js_name = addEmbed)]
    pub fn add_embed(&mut self, at_uri: &str, html: &str) -> Result<(), JsError> {
        use jacquard::types::string::AtUri;
        use jacquard::CowStr;

        let uri = AtUri::new(at_uri)
            .map_err(|e| JsError::new(&format!("Invalid AT URI: {}", e)))?
            .into_static();

        self.inner.add_embed(uri, CowStr::from(html.to_string()), None);
        Ok(())
    }
}

impl JsResolvedContent {
    pub fn into_inner(self) -> ResolvedContent {
        self.inner
    }
}

impl Default for JsResolvedContent {
    fn default() -> Self {
        Self::new()
    }
}

/// Create an empty resolved content container.
///
/// Use this to pre-render embeds before calling render functions.
#[wasm_bindgen]
pub fn create_resolved_content() -> JsResolvedContent {
    JsResolvedContent::new()
}
