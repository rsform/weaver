//! JsEditor - the main editor wrapper for JavaScript.

use std::collections::HashMap;

use wasm_bindgen::prelude::*;
use web_sys::HtmlElement;

use weaver_editor_browser::BrowserClipboard;
use weaver_editor_core::{
    EditorDocument, EditorRope, PlainEditor, RenderCache, UndoableBuffer, apply_formatting,
    execute_action_with_clipboard,
};

use crate::actions::{ActionKind, parse_action};
use crate::types::{EntryEmbeds, EntryJson, FinalizedImage, JsResolvedContent, PendingImage};

type InnerEditor = PlainEditor<UndoableBuffer<EditorRope>>;

/// The main editor instance exposed to JavaScript.
///
/// Wraps the core editor with WASM bindings for browser use.
#[wasm_bindgen]
pub struct JsEditor {
    doc: InnerEditor,
    cache: RenderCache,
    resolved_content: weaver_common::ResolvedContent,

    // Metadata
    title: String,
    path: String,
    tags: Vec<String>,
    created_at: String,

    // Image tracking
    pending_images: HashMap<String, PendingImage>,
    finalized_images: HashMap<String, FinalizedImage>,
}

#[wasm_bindgen]
impl JsEditor {
    /// Create a new empty editor.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        let rope = EditorRope::new();
        let buffer = UndoableBuffer::new(rope, 100);
        let doc = PlainEditor::new(buffer);

        Self {
            doc,
            cache: RenderCache::default(),
            resolved_content: weaver_common::ResolvedContent::new(),
            title: String::new(),
            path: String::new(),
            tags: Vec::new(),
            created_at: now_iso(),
            pending_images: HashMap::new(),
            finalized_images: HashMap::new(),
        }
    }

    /// Create an editor from markdown content.
    #[wasm_bindgen(js_name = fromMarkdown)]
    pub fn from_markdown(content: &str) -> Self {
        let rope = EditorRope::from_str(content);
        let buffer = UndoableBuffer::new(rope, 100);
        let doc = PlainEditor::new(buffer);

        Self {
            doc,
            cache: RenderCache::default(),
            resolved_content: weaver_common::ResolvedContent::new(),
            title: String::new(),
            path: String::new(),
            tags: Vec::new(),
            created_at: now_iso(),
            pending_images: HashMap::new(),
            finalized_images: HashMap::new(),
        }
    }

    /// Create an editor from a snapshot (EntryJson).
    #[wasm_bindgen(js_name = fromSnapshot)]
    pub fn from_snapshot(snapshot: JsValue) -> Result<JsEditor, JsError> {
        let entry: EntryJson = serde_wasm_bindgen::from_value(snapshot)
            .map_err(|e| JsError::new(&format!("Invalid snapshot: {}", e)))?;

        let rope = EditorRope::from_str(&entry.content);
        let buffer = UndoableBuffer::new(rope, 100);
        let doc = PlainEditor::new(buffer);

        Ok(Self {
            doc,
            cache: RenderCache::default(),
            resolved_content: weaver_common::ResolvedContent::new(),
            title: entry.title,
            path: entry.path,
            tags: entry.tags.unwrap_or_default(),
            created_at: entry.created_at,
            pending_images: HashMap::new(),
            finalized_images: HashMap::new(),
        })
    }

    /// Set pre-resolved embed content.
    #[wasm_bindgen(js_name = setResolvedContent)]
    pub fn set_resolved_content(&mut self, content: JsResolvedContent) {
        self.resolved_content = content.into_inner();
    }

    // === Content access ===

    /// Get the markdown content.
    #[wasm_bindgen(js_name = getMarkdown)]
    pub fn get_markdown(&self) -> String {
        self.doc.content_string()
    }

    /// Get the current state as a snapshot (EntryJson).
    #[wasm_bindgen(js_name = getSnapshot)]
    pub fn get_snapshot(&self) -> Result<JsValue, JsError> {
        let entry = EntryJson {
            title: self.title.clone(),
            path: self.path.clone(),
            content: self.doc.content_string(),
            created_at: self.created_at.clone(),
            updated_at: Some(now_iso()),
            tags: if self.tags.is_empty() {
                None
            } else {
                Some(self.tags.clone())
            },
            embeds: self.build_embeds(),
            authors: None,
            content_warnings: None,
            rating: None,
        };

        serde_wasm_bindgen::to_value(&entry)
            .map_err(|e| JsError::new(&format!("Serialization error: {}", e)))
    }

    /// Get the entry JSON, validating required fields.
    ///
    /// Throws if title or path is empty, or if there are pending images.
    #[wasm_bindgen(js_name = toEntry)]
    pub fn to_entry(&self) -> Result<JsValue, JsError> {
        if self.title.is_empty() {
            return Err(JsError::new("Title is required"));
        }
        if self.path.is_empty() {
            return Err(JsError::new("Path is required"));
        }
        if !self.pending_images.is_empty() {
            return Err(JsError::new(
                "Pending images must be finalized before publishing",
            ));
        }

        self.get_snapshot()
    }

    // === Metadata ===

    /// Get the title.
    #[wasm_bindgen(js_name = getTitle)]
    pub fn get_title(&self) -> String {
        self.title.clone()
    }

    /// Set the title.
    #[wasm_bindgen(js_name = setTitle)]
    pub fn set_title(&mut self, title: &str) {
        self.title = title.to_string();
    }

    /// Get the path.
    #[wasm_bindgen(js_name = getPath)]
    pub fn get_path(&self) -> String {
        self.path.clone()
    }

    /// Set the path.
    #[wasm_bindgen(js_name = setPath)]
    pub fn set_path(&mut self, path: &str) {
        self.path = path.to_string();
    }

    /// Get the tags.
    #[wasm_bindgen(js_name = getTags)]
    pub fn get_tags(&self) -> Vec<String> {
        self.tags.clone()
    }

    /// Set the tags.
    #[wasm_bindgen(js_name = setTags)]
    pub fn set_tags(&mut self, tags: Vec<String>) {
        self.tags = tags;
    }

    // === Actions ===

    /// Execute an editor action.
    #[wasm_bindgen(js_name = executeAction)]
    pub fn execute_action(&mut self, action: JsValue) -> Result<(), JsError> {
        let js_action = parse_action(action)?;
        let kind = js_action.to_action_kind();

        let clipboard = BrowserClipboard::empty();
        match kind {
            ActionKind::Editor(editor_action) => {
                execute_action_with_clipboard(&mut self.doc, &editor_action, &clipboard);
            }
            ActionKind::Format(format_action) => {
                apply_formatting(&mut self.doc, format_action);
            }
        }

        Ok(())
    }

    // === Image handling ===

    /// Add a pending image (called when user adds an image).
    #[wasm_bindgen(js_name = addPendingImage)]
    pub fn add_pending_image(&mut self, image: JsValue) -> Result<(), JsError> {
        let pending: PendingImage = serde_wasm_bindgen::from_value(image)
            .map_err(|e| JsError::new(&format!("Invalid pending image: {}", e)))?;

        self.pending_images
            .insert(pending.local_id.clone(), pending);
        Ok(())
    }

    /// Finalize an image after upload.
    #[wasm_bindgen(js_name = finalizeImage)]
    pub fn finalize_image(&mut self, local_id: &str, finalized: JsValue) -> Result<(), JsError> {
        let finalized: FinalizedImage = serde_wasm_bindgen::from_value(finalized)
            .map_err(|e| JsError::new(&format!("Invalid finalized image: {}", e)))?;

        self.pending_images.remove(local_id);
        self.finalized_images
            .insert(local_id.to_string(), finalized);
        Ok(())
    }

    /// Remove a pending image.
    #[wasm_bindgen(js_name = removeImage)]
    pub fn remove_image(&mut self, local_id: &str) {
        self.pending_images.remove(local_id);
        self.finalized_images.remove(local_id);
    }

    /// Get pending images that need upload.
    #[wasm_bindgen(js_name = getPendingImages)]
    pub fn get_pending_images(&self) -> Result<JsValue, JsError> {
        let pending: Vec<_> = self.pending_images.values().cloned().collect();
        serde_wasm_bindgen::to_value(&pending)
            .map_err(|e| JsError::new(&format!("Serialization error: {}", e)))
    }

    /// Get staging URIs for cleanup after publish.
    #[wasm_bindgen(js_name = getStagingUris)]
    pub fn get_staging_uris(&self) -> Vec<String> {
        self.finalized_images
            .values()
            .map(|f| f.staging_uri.clone())
            .collect()
    }

    // === Cursor/selection ===

    /// Get the current cursor offset.
    #[wasm_bindgen(js_name = getCursorOffset)]
    pub fn get_cursor_offset(&self) -> usize {
        self.doc.cursor_offset()
    }

    /// Set the cursor offset.
    #[wasm_bindgen(js_name = setCursorOffset)]
    pub fn set_cursor_offset(&mut self, offset: usize) {
        self.doc.set_cursor_offset(offset);
    }

    /// Get the document length in characters.
    #[wasm_bindgen(js_name = getLength)]
    pub fn get_length(&self) -> usize {
        self.doc.len_chars()
    }

    // === Undo/redo ===

    /// Check if undo is available.
    #[wasm_bindgen(js_name = canUndo)]
    pub fn can_undo(&self) -> bool {
        self.doc.can_undo()
    }

    /// Check if redo is available.
    #[wasm_bindgen(js_name = canRedo)]
    pub fn can_redo(&self) -> bool {
        self.doc.can_redo()
    }
}

impl Default for JsEditor {
    fn default() -> Self {
        Self::new()
    }
}

impl JsEditor {
    /// Build embeds from finalized images.
    fn build_embeds(&self) -> Option<EntryEmbeds> {
        if self.finalized_images.is_empty() {
            return None;
        }

        use crate::types::{ImageEmbed, ImagesEmbed};

        let images: Vec<ImageEmbed> = self
            .finalized_images
            .values()
            .map(|f| ImageEmbed {
                image: f.blob_ref.clone(),
                alt: String::new(), // TODO: track alt text
                aspect_ratio: None,
            })
            .collect();

        Some(EntryEmbeds {
            images: Some(ImagesEmbed { images }),
            records: None,
            externals: None,
            videos: None,
        })
    }
}

/// Get current time as ISO string.
fn now_iso() -> String {
    // Use js_sys::Date for browser-compatible time
    let date = js_sys::Date::new_0();
    date.to_iso_string().into()
}
