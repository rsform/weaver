//! JsEditor - the main editor wrapper for JavaScript.

use std::collections::HashMap;

use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use web_sys::HtmlElement;

use weaver_editor_browser::{
    BrowserClipboard, BrowserCursor, ParagraphRender, update_paragraph_dom,
    update_syntax_visibility,
};
use weaver_editor_core::{
    CursorPlatform, EditorDocument, EditorImageResolver, EditorRope, PlainEditor, RenderCache,
    UndoableBuffer, apply_formatting, execute_action_with_clipboard, render_paragraphs_incremental,
};

use crate::actions::{ActionKind, parse_action};
use crate::types::{
    EntryEmbeds, EntryJson, FinalizedImage, JsParagraphRender, JsResolvedContent, PendingImage,
};

type InnerEditor = PlainEditor<UndoableBuffer<EditorRope>>;

/// The main editor instance exposed to JavaScript.
///
/// Wraps the core editor with WASM bindings for browser use.
#[wasm_bindgen]
pub struct JsEditor {
    pub(crate) doc: InnerEditor,
    pub(crate) cache: RenderCache,
    pub(crate) resolved_content: weaver_common::ResolvedContent,
    pub(crate) image_resolver: EditorImageResolver,
    pub(crate) entry_index: weaver_common::EntryIndex,
    pub(crate) paragraphs: Vec<ParagraphRender>,

    // Mount state
    pub(crate) editor_id: Option<String>,
    pub(crate) on_change: Option<js_sys::Function>,

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
            image_resolver: EditorImageResolver::new(),
            entry_index: weaver_common::EntryIndex::new(),
            paragraphs: Vec::new(),
            editor_id: None,
            on_change: None,
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
            image_resolver: EditorImageResolver::new(),
            entry_index: weaver_common::EntryIndex::new(),
            paragraphs: Vec::new(),
            editor_id: None,
            on_change: None,
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
            image_resolver: EditorImageResolver::new(),
            entry_index: weaver_common::EntryIndex::new(),
            paragraphs: Vec::new(),
            editor_id: None,
            on_change: None,
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
    ///
    /// Automatically re-renders and updates the DOM after the action.
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

        // Update DOM and notify
        self.render_and_update_dom();
        self.notify_change();

        Ok(())
    }

    // === Image handling ===

    /// Add a pending image (called when user adds an image).
    ///
    /// The `data_url` is used for preview rendering until uploaded.
    #[wasm_bindgen(js_name = addPendingImage)]
    pub fn add_pending_image(&mut self, image: JsValue, data_url: &str) -> Result<(), JsError> {
        let pending: PendingImage = serde_wasm_bindgen::from_value(image)
            .map_err(|e| JsError::new(&format!("Invalid pending image: {}", e)))?;

        // Add to image resolver for preview rendering
        self.image_resolver
            .add_pending(&pending.local_id, data_url.to_string());

        self.pending_images
            .insert(pending.local_id.clone(), pending);
        Ok(())
    }

    /// Finalize an image after upload.
    ///
    /// Requires the blob rkey (from sh.weaver.publish.blob) and the user's identifier.
    #[wasm_bindgen(js_name = finalizeImage)]
    pub fn finalize_image(
        &mut self,
        local_id: &str,
        finalized: JsValue,
        blob_rkey: &str,
        ident: &str,
    ) -> Result<(), JsError> {
        use weaver_common::jacquard::IntoStatic;
        use weaver_common::jacquard::types::ident::AtIdentifier;
        use weaver_common::jacquard::types::string::Rkey;

        let finalized_data: FinalizedImage = serde_wasm_bindgen::from_value(finalized)
            .map_err(|e| JsError::new(&format!("Invalid finalized image: {}", e)))?;

        let rkey = Rkey::new(blob_rkey)
            .map_err(|e| JsError::new(&format!("Invalid rkey: {}", e)))?
            .into_static();
        let identifier = AtIdentifier::new(ident)
            .map_err(|e| JsError::new(&format!("Invalid identifier: {}", e)))?
            .into_static();

        // Promote pending to uploaded in image resolver
        self.image_resolver
            .promote_to_uploaded(local_id, rkey, identifier);

        self.pending_images.remove(local_id);
        self.finalized_images
            .insert(local_id.to_string(), finalized_data);
        Ok(())
    }

    /// Remove an image from tracking.
    ///
    /// Note: The image resolver does not support removal, so images remain
    /// until the editor is destroyed.
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

    // === Entry index (for wikilinks) ===

    /// Add an entry to the wikilink index.
    ///
    /// This allows wikilinks to resolve correctly in the editor.
    /// - `title`: The entry title (matched case-insensitively)
    /// - `path`: The entry path slug (matched case-insensitively)
    /// - `canonical_url`: The URL to link to (e.g., "/my-notebook/my-entry")
    #[wasm_bindgen(js_name = addEntryToIndex)]
    pub fn add_entry_to_index(&mut self, title: &str, path: &str, canonical_url: &str) {
        self.entry_index
            .add_entry(title, path, canonical_url.to_string());
    }

    /// Clear the entry index.
    #[wasm_bindgen(js_name = clearEntryIndex)]
    pub fn clear_entry_index(&mut self) {
        self.entry_index = weaver_common::EntryIndex::new();
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

    // === Mounting ===

    /// Mount the editor into a container element.
    ///
    /// Creates a contenteditable div inside the container and sets up event handlers.
    /// The onChange callback is called after each edit.
    #[wasm_bindgen]
    pub fn mount(
        &mut self,
        container: &HtmlElement,
        on_change: Option<js_sys::Function>,
    ) -> Result<(), JsError> {
        let window = web_sys::window().ok_or_else(|| JsError::new("No window"))?;
        let document = window
            .document()
            .ok_or_else(|| JsError::new("No document"))?;

        // Generate unique ID for the editor element
        let editor_id = format!("weaver-editor-{}", js_sys::Math::random().to_bits());

        // Create the contenteditable element
        let editor_el = document
            .create_element("div")
            .map_err(|e| JsError::new(&format!("Failed to create element: {:?}", e)))?;

        editor_el
            .set_attribute("id", &editor_id)
            .map_err(|e| JsError::new(&format!("Failed to set id: {:?}", e)))?;
        editor_el
            .set_attribute("contenteditable", "true")
            .map_err(|e| JsError::new(&format!("Failed to set contenteditable: {:?}", e)))?;
        editor_el
            .set_attribute("class", "weaver-editor-content")
            .map_err(|e| JsError::new(&format!("Failed to set class: {:?}", e)))?;

        container
            .append_child(&editor_el)
            .map_err(|e| JsError::new(&format!("Failed to append child: {:?}", e)))?;

        self.editor_id = Some(editor_id);
        self.on_change = on_change;

        // Initial render
        self.render_and_update_dom();

        Ok(())
    }

    /// Check if the editor is mounted.
    #[wasm_bindgen(js_name = isMounted)]
    pub fn is_mounted(&self) -> bool {
        self.editor_id.is_some()
    }

    /// Unmount the editor and clean up.
    #[wasm_bindgen]
    pub fn unmount(&mut self) {
        if let Some(ref editor_id) = self.editor_id {
            if let Some(window) = web_sys::window() {
                if let Some(document) = window.document() {
                    if let Some(element) = document.get_element_by_id(editor_id) {
                        let _ = element.remove();
                    }
                }
            }
        }
        self.editor_id = None;
        self.on_change = None;
    }

    /// Focus the editor.
    #[wasm_bindgen]
    pub fn focus(&self) {
        if let Some(ref editor_id) = self.editor_id {
            if let Some(window) = web_sys::window() {
                if let Some(document) = window.document() {
                    if let Some(element) = document.get_element_by_id(editor_id) {
                        if let Ok(html_el) = element.dyn_into::<HtmlElement>() {
                            let _ = html_el.focus();
                        }
                    }
                }
            }
        }
    }

    /// Blur the editor.
    #[wasm_bindgen]
    pub fn blur(&self) {
        if let Some(ref editor_id) = self.editor_id {
            if let Some(window) = web_sys::window() {
                if let Some(document) = window.document() {
                    if let Some(element) = document.get_element_by_id(editor_id) {
                        if let Ok(html_el) = element.dyn_into::<HtmlElement>() {
                            let _ = html_el.blur();
                        }
                    }
                }
            }
        }
    }

    // === Rendering ===

    /// Get rendered paragraphs as JS objects.
    ///
    /// For use when host needs to inspect render state.
    #[wasm_bindgen(js_name = getParagraphs)]
    pub fn get_paragraphs(&self) -> Result<JsValue, JsError> {
        let js_paras: Vec<JsParagraphRender> = self
            .paragraphs
            .iter()
            .map(JsParagraphRender::from)
            .collect();
        serde_wasm_bindgen::to_value(&js_paras)
            .map_err(|e| JsError::new(&format!("Serialization error: {}", e)))
    }
}

impl Default for JsEditor {
    fn default() -> Self {
        Self::new()
    }
}

// Internal methods (not exposed to JS)
impl JsEditor {
    /// Render the document and update the DOM.
    pub fn render_and_update_dom(&mut self) {
        let Some(ref editor_id) = self.editor_id else {
            return;
        };

        let cursor_offset = self.doc.cursor_offset();
        let last_edit = self.doc.last_edit();

        // Render with incremental caching
        let result = render_paragraphs_incremental(
            self.doc.buffer(),
            Some(&self.cache),
            cursor_offset,
            last_edit.as_ref(),
            Some(&self.image_resolver),
            Some(&self.entry_index),
            &self.resolved_content,
        );

        let old_paragraphs = std::mem::replace(&mut self.paragraphs, result.paragraphs);
        self.cache = result.cache;
        self.doc.set_last_edit(None); // Clear after using

        // Update DOM
        let cursor_para_updated = update_paragraph_dom(
            editor_id,
            &old_paragraphs,
            &self.paragraphs,
            cursor_offset,
            false,
        );

        // Update syntax visibility
        let syntax_spans: Vec<_> = self
            .paragraphs
            .iter()
            .flat_map(|p| p.syntax_spans.iter().cloned())
            .collect();
        update_syntax_visibility(cursor_offset, None, &syntax_spans, &self.paragraphs);

        // Restore cursor position after DOM update
        if cursor_para_updated {
            let cursor = BrowserCursor::new(editor_id);
            let snap_direction = self.doc.pending_snap();
            let _ = cursor.restore_cursor(cursor_offset, &self.paragraphs, snap_direction);
        }
    }

    /// Notify the onChange callback.
    pub(crate) fn notify_change(&self) {
        if let Some(ref callback) = self.on_change {
            let this = JsValue::null();
            let _ = callback.call0(&this);
        }
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
