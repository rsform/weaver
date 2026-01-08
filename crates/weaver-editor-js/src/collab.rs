//! JsCollabEditor - collaborative editor with Loro CRDT and iroh P2P.
//!
//! This wraps the core editor with a Loro-backed buffer and manages
//! the EditorReactor worker for off-main-thread collab networking.

use std::collections::HashMap;

use wasm_bindgen::prelude::*;
use web_sys::HtmlElement;

use weaver_editor_browser::{
    BrowserClipboard, BrowserCursor, ParagraphRender, update_paragraph_dom,
    update_syntax_visibility,
};
use weaver_editor_core::{
    CursorPlatform, EditorDocument, EditorImageResolver, PlainEditor, RenderCache, TextBuffer,
    apply_formatting, execute_action_with_clipboard, render_paragraphs_incremental,
};
use weaver_editor_crdt::{LoroTextBuffer, VersionVector};

use crate::actions::{ActionKind, parse_action};
use crate::types::{
    EntryEmbeds, EntryJson, FinalizedImage, JsParagraphRender, JsResolvedContent, PendingImage,
};

type InnerEditor = PlainEditor<LoroTextBuffer>;

/// Collaborative editor with Loro CRDT backend and iroh P2P networking.
///
/// The host app is responsible for:
/// - Creating/refreshing/deleting session records on PDS
/// - Discovering peers via index or backlinks
/// - Calling `addPeers` with discovered peer node IDs
///
/// The editor handles:
/// - Loro CRDT document sync
/// - iroh gossip networking (via web worker)
/// - Presence tracking
#[wasm_bindgen]
pub struct JsCollabEditor {
    doc: InnerEditor,
    cache: RenderCache,
    resolved_content: weaver_common::ResolvedContent,
    image_resolver: EditorImageResolver,
    entry_index: weaver_common::EntryIndex,
    paragraphs: Vec<ParagraphRender>,

    // Mount state
    editor_id: Option<String>,
    on_change: Option<js_sys::Function>,

    // Collab state
    resource_uri: String,
    collab_topic: Option<[u8; 32]>,

    // Callbacks for host to handle PDS operations
    on_session_needed: Option<js_sys::Function>,
    on_session_refresh: Option<js_sys::Function>,
    on_session_end: Option<js_sys::Function>,
    on_peers_needed: Option<js_sys::Function>,
    on_presence_changed: Option<js_sys::Function>,
    on_remote_update: Option<js_sys::Function>,

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
impl JsCollabEditor {
    /// Create a new empty collab editor.
    #[wasm_bindgen(constructor)]
    pub fn new(resource_uri: &str) -> Self {
        let buffer = LoroTextBuffer::new();
        let doc = PlainEditor::new(buffer);
        let topic = weaver_editor_crdt::compute_collab_topic(resource_uri);

        Self {
            doc,
            cache: RenderCache::default(),
            resolved_content: weaver_common::ResolvedContent::new(),
            image_resolver: EditorImageResolver::new(),
            entry_index: weaver_common::EntryIndex::new(),
            paragraphs: Vec::new(),
            editor_id: None,
            on_change: None,
            resource_uri: resource_uri.to_string(),
            collab_topic: Some(topic),
            on_session_needed: None,
            on_session_refresh: None,
            on_session_end: None,
            on_peers_needed: None,
            on_presence_changed: None,
            on_remote_update: None,
            title: String::new(),
            path: String::new(),
            tags: Vec::new(),
            created_at: now_iso(),
            pending_images: HashMap::new(),
            finalized_images: HashMap::new(),
        }
    }

    /// Create from markdown content.
    #[wasm_bindgen(js_name = fromMarkdown)]
    pub fn from_markdown(resource_uri: &str, content: &str) -> Self {
        let mut buffer = LoroTextBuffer::new();
        buffer.push(content);
        let doc = PlainEditor::new(buffer);
        let topic = weaver_editor_crdt::compute_collab_topic(resource_uri);

        Self {
            doc,
            cache: RenderCache::default(),
            resolved_content: weaver_common::ResolvedContent::new(),
            image_resolver: EditorImageResolver::new(),
            entry_index: weaver_common::EntryIndex::new(),
            paragraphs: Vec::new(),
            editor_id: None,
            on_change: None,
            resource_uri: resource_uri.to_string(),
            collab_topic: Some(topic),
            on_session_needed: None,
            on_session_refresh: None,
            on_session_end: None,
            on_peers_needed: None,
            on_presence_changed: None,
            on_remote_update: None,
            title: String::new(),
            path: String::new(),
            tags: Vec::new(),
            created_at: now_iso(),
            pending_images: HashMap::new(),
            finalized_images: HashMap::new(),
        }
    }

    /// Create from a Loro snapshot.
    #[wasm_bindgen(js_name = fromSnapshot)]
    pub fn from_snapshot(resource_uri: &str, snapshot: &[u8]) -> Result<JsCollabEditor, JsError> {
        let buffer = LoroTextBuffer::from_snapshot(snapshot)
            .map_err(|e| JsError::new(&format!("Invalid snapshot: {}", e)))?;
        let doc = PlainEditor::new(buffer);
        let topic = weaver_editor_crdt::compute_collab_topic(resource_uri);

        Ok(Self {
            doc,
            cache: RenderCache::default(),
            resolved_content: weaver_common::ResolvedContent::new(),
            image_resolver: EditorImageResolver::new(),
            entry_index: weaver_common::EntryIndex::new(),
            paragraphs: Vec::new(),
            editor_id: None,
            on_change: None,
            resource_uri: resource_uri.to_string(),
            collab_topic: Some(topic),
            on_session_needed: None,
            on_session_refresh: None,
            on_session_end: None,
            on_peers_needed: None,
            on_presence_changed: None,
            on_remote_update: None,
            title: String::new(),
            path: String::new(),
            tags: Vec::new(),
            created_at: now_iso(),
            pending_images: HashMap::new(),
            finalized_images: HashMap::new(),
        })
    }

    // === Callbacks ===

    /// Set callback for when a session record needs to be created.
    ///
    /// Called with: { nodeId: string, relayUrl: string | null }
    /// Should return: Promise<string> (the session record URI)
    #[wasm_bindgen(js_name = setOnSessionNeeded)]
    pub fn set_on_session_needed(&mut self, callback: js_sys::Function) {
        self.on_session_needed = Some(callback);
    }

    /// Set callback for periodic session refresh.
    ///
    /// Called with: { sessionUri: string }
    /// Should return: Promise<void>
    #[wasm_bindgen(js_name = setOnSessionRefresh)]
    pub fn set_on_session_refresh(&mut self, callback: js_sys::Function) {
        self.on_session_refresh = Some(callback);
    }

    /// Set callback for when the session ends.
    ///
    /// Called with: { sessionUri: string }
    /// Should return: Promise<void>
    #[wasm_bindgen(js_name = setOnSessionEnd)]
    pub fn set_on_session_end(&mut self, callback: js_sys::Function) {
        self.on_session_end = Some(callback);
    }

    /// Set callback for peer discovery.
    ///
    /// Called with: { resourceUri: string }
    /// Should return: Promise<string[]> (array of node IDs)
    #[wasm_bindgen(js_name = setOnPeersNeeded)]
    pub fn set_on_peers_needed(&mut self, callback: js_sys::Function) {
        self.on_peers_needed = Some(callback);
    }

    /// Set callback for presence changes.
    ///
    /// Called with: PresenceSnapshot
    #[wasm_bindgen(js_name = setOnPresenceChanged)]
    pub fn set_on_presence_changed(&mut self, callback: js_sys::Function) {
        self.on_presence_changed = Some(callback);
    }

    /// Set callback for remote updates (for debugging/logging).
    #[wasm_bindgen(js_name = setOnRemoteUpdate)]
    pub fn set_on_remote_update(&mut self, callback: js_sys::Function) {
        self.on_remote_update = Some(callback);
    }

    // === Loro sync methods ===

    /// Export a full Loro snapshot.
    #[wasm_bindgen(js_name = exportSnapshot)]
    pub fn export_snapshot(&self) -> Vec<u8> {
        self.doc.buffer().export_snapshot()
    }

    /// Export updates since a given version.
    ///
    /// Returns null if no changes since that version.
    #[wasm_bindgen(js_name = exportUpdatesSince)]
    pub fn export_updates_since(&self, version: &[u8]) -> Option<Vec<u8>> {
        let vv = VersionVector::decode(version).ok()?;
        self.doc.buffer().export_updates_since(&vv)
    }

    /// Import remote Loro updates.
    #[wasm_bindgen(js_name = importUpdates)]
    pub fn import_updates(&mut self, data: &[u8]) -> Result<(), JsError> {
        self.doc
            .buffer_mut()
            .import(data)
            .map_err(|e| JsError::new(&format!("Import failed: {}", e)))?;

        // Re-render after importing remote changes
        self.render_and_update_dom();
        self.notify_change();

        Ok(())
    }

    /// Get the current version vector as bytes.
    #[wasm_bindgen(js_name = getVersion)]
    pub fn get_version(&self) -> Vec<u8> {
        self.doc.buffer().version().encode()
    }

    /// Get the collab topic (blake3 hash of resource URI).
    #[wasm_bindgen(js_name = getCollabTopic)]
    pub fn get_collab_topic(&self) -> Option<Vec<u8>> {
        self.collab_topic.map(|t| t.to_vec())
    }

    /// Get the resource URI.
    #[wasm_bindgen(js_name = getResourceUri)]
    pub fn get_resource_uri(&self) -> String {
        self.resource_uri.clone()
    }

    // === Content access (same as JsEditor) ===

    #[wasm_bindgen(js_name = getMarkdown)]
    pub fn get_markdown(&self) -> String {
        self.doc.content_string()
    }

    #[wasm_bindgen(js_name = getSnapshot)]
    pub fn get_entry_snapshot(&self) -> Result<JsValue, JsError> {
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

        self.get_entry_snapshot()
    }

    // === Metadata ===

    #[wasm_bindgen(js_name = getTitle)]
    pub fn get_title(&self) -> String {
        self.title.clone()
    }

    #[wasm_bindgen(js_name = setTitle)]
    pub fn set_title(&mut self, title: &str) {
        self.title = title.to_string();
    }

    #[wasm_bindgen(js_name = getPath)]
    pub fn get_path(&self) -> String {
        self.path.clone()
    }

    #[wasm_bindgen(js_name = setPath)]
    pub fn set_path(&mut self, path: &str) {
        self.path = path.to_string();
    }

    #[wasm_bindgen(js_name = getTags)]
    pub fn get_tags(&self) -> Vec<String> {
        self.tags.clone()
    }

    #[wasm_bindgen(js_name = setTags)]
    pub fn set_tags(&mut self, tags: Vec<String>) {
        self.tags = tags;
    }

    // === Actions ===

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

        self.render_and_update_dom();
        self.notify_change();

        Ok(())
    }

    // === Image handling ===

    #[wasm_bindgen(js_name = addPendingImage)]
    pub fn add_pending_image(&mut self, image: JsValue, data_url: &str) -> Result<(), JsError> {
        let pending: PendingImage = serde_wasm_bindgen::from_value(image)
            .map_err(|e| JsError::new(&format!("Invalid pending image: {}", e)))?;

        self.image_resolver
            .add_pending(&pending.local_id, data_url.to_string());

        self.pending_images
            .insert(pending.local_id.clone(), pending);
        Ok(())
    }

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

        self.image_resolver
            .promote_to_uploaded(local_id, rkey, identifier);

        self.pending_images.remove(local_id);
        self.finalized_images
            .insert(local_id.to_string(), finalized_data);
        Ok(())
    }

    #[wasm_bindgen(js_name = removeImage)]
    pub fn remove_image(&mut self, local_id: &str) {
        self.pending_images.remove(local_id);
        self.finalized_images.remove(local_id);
    }

    #[wasm_bindgen(js_name = getPendingImages)]
    pub fn get_pending_images(&self) -> Result<JsValue, JsError> {
        let pending: Vec<_> = self.pending_images.values().cloned().collect();
        serde_wasm_bindgen::to_value(&pending)
            .map_err(|e| JsError::new(&format!("Serialization error: {}", e)))
    }

    #[wasm_bindgen(js_name = getStagingUris)]
    pub fn get_staging_uris(&self) -> Vec<String> {
        self.finalized_images
            .values()
            .map(|f| f.staging_uri.clone())
            .collect()
    }

    // === Entry index ===

    #[wasm_bindgen(js_name = addEntryToIndex)]
    pub fn add_entry_to_index(&mut self, title: &str, path: &str, canonical_url: &str) {
        self.entry_index
            .add_entry(title, path, canonical_url.to_string());
    }

    #[wasm_bindgen(js_name = clearEntryIndex)]
    pub fn clear_entry_index(&mut self) {
        self.entry_index = weaver_common::EntryIndex::new();
    }

    // === Cursor/selection ===

    #[wasm_bindgen(js_name = getCursorOffset)]
    pub fn get_cursor_offset(&self) -> usize {
        self.doc.cursor_offset()
    }

    /// Get the current selection, or null if no selection.
    #[wasm_bindgen(js_name = getSelection)]
    pub fn get_selection(&self) -> JsValue {
        match self.doc.selection() {
            Some(s) => {
                #[derive(serde::Serialize)]
                struct JsSelection {
                    anchor: usize,
                    head: usize,
                }
                serde_wasm_bindgen::to_value(&JsSelection {
                    anchor: s.anchor,
                    head: s.head,
                })
                .unwrap_or(JsValue::NULL)
            }
            None => JsValue::NULL,
        }
    }

    #[wasm_bindgen(js_name = setCursorOffset)]
    pub fn set_cursor_offset(&mut self, offset: usize) {
        self.doc.set_cursor_offset(offset);
        // Sync Loro cursor for CRDT-aware tracking
        self.doc.buffer().sync_cursor(offset);
    }

    #[wasm_bindgen(js_name = getLength)]
    pub fn get_length(&self) -> usize {
        self.doc.len_chars()
    }

    // === Undo/redo ===

    #[wasm_bindgen(js_name = canUndo)]
    pub fn can_undo(&self) -> bool {
        self.doc.can_undo()
    }

    #[wasm_bindgen(js_name = canRedo)]
    pub fn can_redo(&self) -> bool {
        self.doc.can_redo()
    }

    // === Mounting ===

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

        let editor_id = format!("weaver-collab-editor-{}", js_sys::Math::random().to_bits());

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

        self.render_and_update_dom();

        Ok(())
    }

    #[wasm_bindgen(js_name = isMounted)]
    pub fn is_mounted(&self) -> bool {
        self.editor_id.is_some()
    }

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

    #[wasm_bindgen]
    pub fn focus(&self) {
        use wasm_bindgen::JsCast;
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

    #[wasm_bindgen]
    pub fn blur(&self) {
        use wasm_bindgen::JsCast;
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

    #[wasm_bindgen(js_name = setResolvedContent)]
    pub fn set_resolved_content(&mut self, content: JsResolvedContent) {
        self.resolved_content = content.into_inner();
    }

    #[wasm_bindgen(js_name = renderAndUpdateDom)]
    pub fn render_and_update_dom_js(&mut self) {
        self.render_and_update_dom();
    }

    // === Remote cursor positioning ===

    /// Get cursor rect relative to editor for a given character position.
    ///
    /// Returns { x, y, height } or null if position can't be mapped.
    #[wasm_bindgen(js_name = getCursorRectRelative)]
    pub fn get_cursor_rect_relative(&self, position: usize) -> JsValue {
        let Some(ref editor_id) = self.editor_id else {
            return JsValue::NULL;
        };

        // Flatten offset maps from all paragraphs.
        let offset_map: Vec<_> = self
            .paragraphs
            .iter()
            .flat_map(|p| p.offset_map.iter().cloned())
            .collect();

        let Some(rect) =
            weaver_editor_browser::get_cursor_rect_relative(position, &offset_map, editor_id)
        else {
            return JsValue::NULL;
        };

        #[derive(serde::Serialize)]
        struct JsCursorRect {
            x: f64,
            y: f64,
            height: f64,
        }

        serde_wasm_bindgen::to_value(&JsCursorRect {
            x: rect.x,
            y: rect.y,
            height: rect.height,
        })
        .unwrap_or(JsValue::NULL)
    }

    /// Get selection rects relative to editor for a given range.
    ///
    /// Returns array of { x, y, width, height } for each line of selection.
    #[wasm_bindgen(js_name = getSelectionRectsRelative)]
    pub fn get_selection_rects_relative(&self, start: usize, end: usize) -> JsValue {
        let Some(ref editor_id) = self.editor_id else {
            return JsValue::from(js_sys::Array::new());
        };

        // Flatten offset maps from all paragraphs.
        let offset_map: Vec<_> = self
            .paragraphs
            .iter()
            .flat_map(|p| p.offset_map.iter().cloned())
            .collect();

        let rects =
            weaver_editor_browser::get_selection_rects_relative(start, end, &offset_map, editor_id);

        #[derive(serde::Serialize)]
        struct JsSelectionRect {
            x: f64,
            y: f64,
            width: f64,
            height: f64,
        }

        let js_rects: Vec<JsSelectionRect> = rects
            .into_iter()
            .map(|r| JsSelectionRect {
                x: r.x,
                y: r.y,
                width: r.width,
                height: r.height,
            })
            .collect();

        serde_wasm_bindgen::to_value(&js_rects).unwrap_or(JsValue::from(js_sys::Array::new()))
    }

    /// Convert RGBA u32 color (0xRRGGBBAA) to CSS rgba() string.
    #[wasm_bindgen(js_name = rgbaToCss)]
    pub fn rgba_to_css(color: u32) -> String {
        weaver_editor_browser::rgba_u32_to_css(color)
    }

    /// Convert RGBA u32 color to CSS rgba() string with custom alpha.
    #[wasm_bindgen(js_name = rgbaToCssAlpha)]
    pub fn rgba_to_css_alpha(color: u32, alpha: f32) -> String {
        weaver_editor_browser::rgba_u32_to_css_alpha(color, alpha)
    }
}

impl JsCollabEditor {
    pub(crate) fn render_and_update_dom(&mut self) {
        let Some(ref editor_id) = self.editor_id else {
            return;
        };

        let cursor_offset = self.doc.cursor_offset();
        let last_edit = self.doc.last_edit();

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
        self.doc.set_last_edit(None);

        let cursor_para_updated = update_paragraph_dom(
            editor_id,
            &old_paragraphs,
            &self.paragraphs,
            cursor_offset,
            false,
        );

        let syntax_spans: Vec<_> = self
            .paragraphs
            .iter()
            .flat_map(|p| p.syntax_spans.iter().cloned())
            .collect();
        update_syntax_visibility(cursor_offset, None, &syntax_spans, &self.paragraphs);

        if cursor_para_updated {
            let cursor = BrowserCursor::new(editor_id);
            let snap_direction = self.doc.pending_snap();
            let _ = cursor.restore_cursor(cursor_offset, &self.paragraphs, snap_direction);
        }
    }

    pub(crate) fn notify_change(&self) {
        if let Some(ref callback) = self.on_change {
            let this = JsValue::null();
            let _ = callback.call0(&this);
        }
    }

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
                alt: String::new(),
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

fn now_iso() -> String {
    let date = js_sys::Date::new_0();
    date.to_iso_string().into()
}
