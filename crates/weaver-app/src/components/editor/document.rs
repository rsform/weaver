//! Core data structures for the markdown editor.
//!
//! Uses Loro CRDT for text storage with built-in undo/redo support.
//! Mirrors the `sh.weaver.notebook.entry` schema for AT Protocol integration.
//!
//! # Reactive Architecture
//!
//! Individual fields are wrapped in Dioxus Signals for fine-grained reactivity:
//! - Cursor/selection changes don't trigger content re-renders
//! - Content changes bump `content_changed` Signal to trigger paragraph re-renders
//! - The document struct itself is NOT wrapped in a Signal - use `use_hook`

use dioxus::prelude::*;
use loro::{
    Frontiers, LoroDoc, LoroList, LoroMap, LoroResult, LoroText, LoroValue, ToJson, VersionVector,
    cursor::Cursor,
};

use jacquard::IntoStatic;
use jacquard::from_json_value;
use jacquard::smol_str::SmolStr;
use jacquard::types::string::AtUri;
use weaver_api::com_atproto::repo::strong_ref::StrongRef;
use weaver_api::sh_weaver::embed::images::Image;
use weaver_api::sh_weaver::embed::records::RecordEmbed;
use weaver_api::sh_weaver::notebook::entry::Entry;
use weaver_editor_core::EditorDocument;
use weaver_editor_core::TextBuffer;
use weaver_editor_core::UndoManager;
pub use weaver_editor_core::{
    Affinity, CompositionState, CursorState, EditInfo, EditorImage, Selection,
};
use weaver_editor_crdt::LoroTextBuffer;

/// Single source of truth for editor state.
///
/// Contains the document text (backed by Loro CRDT via LoroTextBuffer), cursor position,
/// selection, and IME composition state. Mirrors the `sh.weaver.notebook.entry`
/// schema with CRDT containers for each field.
///
/// # Reactive Architecture
///
/// The document itself is NOT wrapped in a Signal. Instead, individual fields
/// that need reactivity are wrapped in Signals:
/// - `cursor`, `selection`, `composition` - high-frequency, cursor-only updates
/// - `content_changed` - bumped to trigger paragraph re-renders when content changes
///
/// Use `use_hook(|| SignalEditorDocument::new(...))` in components, not `use_signal`.
///
/// # Cloning
///
/// SignalEditorDocument is cheap to clone - LoroTextBuffer and Loro types are Arc-backed,
/// and Signals are Copy. Closures can capture clones without overhead.
#[derive(Clone)]
pub struct SignalEditorDocument {
    /// The text buffer wrapping LoroDoc with undo/redo and cursor tracking.
    /// Access the underlying LoroDoc via `buffer.doc()`.
    buffer: LoroTextBuffer,

    // --- Entry schema containers (Loro handles interior mutability) ---
    // These are obtained from buffer.doc() but cached for convenience.
    /// Entry title (maps to entry.title)
    title: LoroText,

    /// URL path/slug (maps to entry.path)
    path: LoroText,

    /// ISO datetime string (maps to entry.createdAt)
    created_at: LoroText,

    /// Tags list (maps to entry.tags)
    tags: LoroList,

    /// Embeds container (maps to entry.embeds)
    /// Contains nested containers: images (LoroList), externals (LoroList), etc.
    embeds: LoroMap,

    // --- Entry tracking (reactive) ---
    /// StrongRef to the entry if editing an existing record.
    /// None for new entries that haven't been published yet.
    /// Signal so cloned docs share the same state after publish.
    pub entry_ref: Signal<Option<StrongRef<'static>>>,

    /// AT-URI of the notebook this draft belongs to (for re-publishing)
    pub notebook_uri: Signal<Option<SmolStr>>,

    // --- Edit sync state (for PDS sync) ---
    /// StrongRef to the sh.weaver.edit.root record for this edit session.
    /// None if we haven't synced to PDS yet.
    pub edit_root: Signal<Option<StrongRef<'static>>>,

    /// StrongRef to the most recent sh.weaver.edit.diff record.
    /// Used for the `prev` field when creating new diffs.
    /// None if no diffs have been created yet (only root exists).
    pub last_diff: Signal<Option<StrongRef<'static>>>,

    /// Version vector at the time of last sync to PDS.
    /// Used to export only changes since last sync.
    /// None if never synced.
    /// Signal so cloned docs share the same sync state.
    last_synced_version: Signal<Option<VersionVector>>,

    /// Last seen diff URI per collaborator root.
    /// Maps root URI -> last diff URI we've imported from that root.
    /// The diff rkey (TID) is time-sortable, so we skip diffs with rkey <= this.
    pub last_seen_diffs: Signal<std::collections::HashMap<AtUri<'static>, AtUri<'static>>>,

    // --- Reactive editor state (Signal-wrapped for fine-grained updates) ---
    /// Current cursor position. Signal so cursor changes don't dirty content memos.
    pub cursor: Signal<CursorState>,

    /// Active selection if any. Signal for same reason as cursor.
    pub selection: Signal<Option<Selection>>,

    /// IME composition state. Signal so composition updates are isolated.
    pub composition: Signal<Option<CompositionState>>,

    /// Timestamp when the last composition ended.
    /// Used for Safari workaround: ignore Enter keydown within 500ms of compositionend.
    pub composition_ended_at: Signal<Option<web_time::Instant>>,

    /// Bumped when content changes to trigger paragraph re-renders.
    /// Actual EditInfo is obtained from `buffer.last_edit()`.
    pub content_changed: Signal<()>,

    /// Pending snap direction for cursor restoration after edits.
    /// Set by input handlers, consumed by cursor restoration.
    pub pending_snap: Signal<Option<weaver_editor_core::SnapDirection>>,

    /// Collected refs (wikilinks, AT embeds) from the most recent render.
    /// Updated by the render pipeline, read by publish for populating records.
    pub collected_refs: Signal<Vec<weaver_common::ExtractedRef>>,
}

/// Pre-loaded document state that can be created outside of reactive context.
///
/// This struct holds the raw LoroDoc (which is safe outside reactive context)
/// along with sync state metadata. Use `SignalEditorDocument::from_loaded_state()`
/// inside a `use_hook` to convert this into a reactive SignalEditorDocument.
///
/// Note: Clone is a shallow/reference clone for LoroDoc (Arc-backed).
/// PartialEq always returns false since we can't meaningfully compare docs.
#[derive(Clone)]
pub struct LoadedDocState {
    /// The Loro document with all content already loaded/merged.
    pub doc: LoroDoc,
    /// StrongRef to the entry if editing an existing record.
    pub entry_ref: Option<StrongRef<'static>>,
    /// StrongRef to the sh.weaver.edit.root record (for PDS sync).
    pub edit_root: Option<StrongRef<'static>>,
    /// StrongRef to the most recent sh.weaver.edit.diff record.
    pub last_diff: Option<StrongRef<'static>>,
    /// Version vector of the last known PDS state.
    /// Used to determine what changes need to be synced.
    /// None if never synced to PDS.
    pub synced_version: Option<VersionVector>,
    /// Last seen diff URIs per collaborator root.
    /// Used for incremental sync on subsequent refreshes.
    pub last_seen_diffs: std::collections::HashMap<AtUri<'static>, AtUri<'static>>,
    /// Pre-resolved embed content fetched during load.
    /// Avoids embed pop-in on initial render.
    pub resolved_content: weaver_common::ResolvedContent,
    /// Notebook URI for re-publishing to the same notebook.
    pub notebook_uri: Option<SmolStr>,
}

impl PartialEq for LoadedDocState {
    fn eq(&self, _other: &Self) -> bool {
        // LoadedDocState contains LoroDoc which can't be meaningfully compared.
        // Return false to ensure components re-render when passed as props.
        false
    }
}

impl SignalEditorDocument {
    /// Create a new editor document with the given content.
    /// Sets `created_at` to current time.
    ///
    /// # Note
    /// This creates Dioxus Signals for reactive fields. Call from within
    /// a component using `use_hook(|| SignalEditorDocument::new(...))`.
    pub fn new(initial_content: String) -> Self {
        // Create the LoroTextBuffer which owns the LoroDoc
        let mut buffer = LoroTextBuffer::new();
        let doc = buffer.doc().clone();

        // Get other containers from the doc
        let title = doc.get_text("title");
        let path = doc.get_text("path");
        let created_at = doc.get_text("created_at");
        let tags = doc.get_list("tags");
        let embeds = doc.get_map("embeds");

        // Insert initial content if any
        if !initial_content.is_empty() {
            buffer.insert(0, &initial_content);
        }

        // Set created_at to current time (ISO 8601)
        let now = Self::current_datetime_string();
        created_at
            .insert(0, &now)
            .expect("failed to set created_at");

        Self {
            buffer,
            title,
            path,
            created_at,
            tags,
            embeds,
            entry_ref: Signal::new(None),
            notebook_uri: Signal::new(None),
            edit_root: Signal::new(None),
            last_diff: Signal::new(None),
            last_synced_version: Signal::new(None),
            last_seen_diffs: Signal::new(std::collections::HashMap::new()),
            cursor: Signal::new(CursorState {
                offset: 0,
                affinity: Affinity::Before,
            }),
            selection: Signal::new(None),
            composition: Signal::new(None),
            composition_ended_at: Signal::new(None),
            content_changed: Signal::new(()),
            pending_snap: Signal::new(None),
            collected_refs: Signal::new(Vec::new()),
        }
    }

    /// Create a SignalEditorDocument from a fetched Entry.
    ///
    /// MUST be called from within a reactive context (e.g., `use_hook`) to
    /// properly initialize Dioxus Signals.
    pub fn from_entry(entry: &Entry<'_>, entry_ref: StrongRef<'static>) -> Self {
        let mut doc = Self::new(entry.content.to_string());

        // Set metadata
        doc.set_title(&entry.title);
        doc.set_path(&entry.path);
        doc.set_created_at(&entry.created_at.to_string());

        // Add tags
        if let Some(ref tags) = entry.tags {
            for tag in tags.iter() {
                doc.add_tag(tag.as_ref());
            }
        }

        // Add existing images (no published_blob_uri needed - they're already in the entry)
        if let Some(ref embeds) = entry.embeds {
            if let Some(ref images) = embeds.images {
                for img in &images.images {
                    doc.add_image(&img.clone().into_static(), None);
                }
            }

            if let Some(ref records) = embeds.records {
                for record in &records.records {
                    doc.add_record(&record.clone().into_static());
                }
            }
        }

        // Set the entry_ref so subsequent publishes update this record
        doc.set_entry_ref(Some(entry_ref));

        doc
    }

    /// Generate current datetime as ISO 8601 string.
    #[cfg(target_family = "wasm")]
    fn current_datetime_string() -> String {
        js_sys::Date::new_0()
            .to_iso_string()
            .as_string()
            .unwrap_or_default()
    }

    #[cfg(not(target_family = "wasm"))]
    fn current_datetime_string() -> String {
        // Fallback for non-wasm (tests, etc.)
        chrono::Utc::now().to_rfc3339()
    }

    /// Get the underlying LoroText for read operations on content.
    pub fn loro_text(&self) -> &LoroText {
        self.buffer.content()
    }

    /// Get the underlying LoroDoc for subscriptions and advanced operations.
    pub fn loro_doc(&self) -> &LoroDoc {
        self.buffer.doc()
    }

    /// Get direct access to the LoroTextBuffer.
    pub fn buffer(&self) -> &LoroTextBuffer {
        &self.buffer
    }

    /// Get mutable access to the LoroTextBuffer.
    pub fn buffer_mut(&mut self) -> &mut LoroTextBuffer {
        &mut self.buffer
    }

    // --- Content accessors ---

    /// Get the markdown content as a string.
    pub fn content(&self) -> String {
        weaver_editor_core::TextBuffer::to_string(&self.buffer)
    }

    /// Convert the document content to a string (alias for content()).
    pub fn to_string(&self) -> String {
        weaver_editor_core::TextBuffer::to_string(&self.buffer)
    }

    /// Get the length of the content in characters.
    pub fn len_chars(&self) -> usize {
        weaver_editor_core::TextBuffer::len_chars(&self.buffer)
    }

    /// Get the length of the content in UTF-8 bytes.
    pub fn len_bytes(&self) -> usize {
        weaver_editor_core::TextBuffer::len_bytes(&self.buffer)
    }

    /// Get the length of the content in UTF-16 code units.
    pub fn len_utf16(&self) -> usize {
        self.buffer.content().len_utf16()
    }

    /// Check if the content is empty.
    pub fn is_empty(&self) -> bool {
        weaver_editor_core::TextBuffer::len_chars(&self.buffer) == 0
    }

    // --- Entry metadata accessors ---

    /// Get the entry title.
    pub fn title(&self) -> String {
        self.title.to_string()
    }

    /// Set the entry title (replaces existing).
    /// Takes &self because Loro has interior mutability.
    pub fn set_title(&self, new_title: &str) {
        let current_len = self.title.len_unicode();
        if current_len > 0 {
            self.title.delete(0, current_len).ok();
        }
        self.title.insert(0, new_title).ok();
    }

    /// Get the URL path/slug.
    pub fn path(&self) -> String {
        self.path.to_string()
    }

    /// Set the URL path/slug (replaces existing).
    /// Takes &self because Loro has interior mutability.
    pub fn set_path(&self, new_path: &str) {
        let current_len = self.path.len_unicode();
        if current_len > 0 {
            self.path.delete(0, current_len).ok();
        }
        self.path.insert(0, new_path).ok();
    }

    /// Get the created_at timestamp (ISO 8601 string).
    pub fn created_at(&self) -> String {
        self.created_at.to_string()
    }

    /// Set the created_at timestamp (usually only called once on creation or when loading).
    /// Takes &self because Loro has interior mutability.
    pub fn set_created_at(&self, datetime: &str) {
        let current_len = self.created_at.len_unicode();
        if current_len > 0 {
            self.created_at.delete(0, current_len).ok();
        }
        self.created_at.insert(0, datetime).ok();
    }

    // --- Entry ref accessors ---

    /// Get the StrongRef to the entry if editing an existing record.
    pub fn entry_ref(&self) -> Option<StrongRef<'static>> {
        self.entry_ref.read().clone()
    }

    /// Set the StrongRef when editing an existing entry.
    pub fn set_entry_ref(&mut self, entry: Option<StrongRef<'static>>) {
        self.entry_ref.set(entry);
    }

    /// Get the notebook URI if this draft belongs to a notebook.
    pub fn notebook_uri(&self) -> Option<SmolStr> {
        self.notebook_uri.read().clone()
    }

    /// Set the notebook URI for re-publishing to the same notebook.
    pub fn set_notebook_uri(&mut self, uri: Option<SmolStr>) {
        self.notebook_uri.set(uri);
    }

    // --- Tags accessors ---

    /// Get all tags as a vector of strings.
    pub fn tags(&self) -> Vec<String> {
        let len = self.tags.len();
        (0..len)
            .filter_map(|i| match self.tags.get(i)? {
                loro::ValueOrContainer::Value(LoroValue::String(s)) => Some(s.to_string()),
                _ => None,
            })
            .collect()
    }

    /// Add a tag (if not already present).
    /// Takes &self because Loro has interior mutability.
    pub fn add_tag(&self, tag: &str) {
        let existing = self.tags();
        if !existing.iter().any(|t| t == tag) {
            self.tags.push(LoroValue::String(tag.into())).ok();
        }
    }

    /// Remove a tag by value.
    /// Takes &self because Loro has interior mutability.
    pub fn remove_tag(&self, tag: &str) {
        let len = self.tags.len();
        for i in (0..len).rev() {
            if let Some(loro::ValueOrContainer::Value(LoroValue::String(s))) = self.tags.get(i) {
                if s.as_str() == tag {
                    self.tags.delete(i, 1).ok();
                    break;
                }
            }
        }
    }

    /// Clear all tags.
    /// Takes &self because Loro has interior mutability.
    pub fn clear_tags(&self) {
        let len = self.tags.len();
        if len > 0 {
            self.tags.delete(0, len).ok();
        }
    }

    // --- Images accessors ---

    /// Get the images LoroList from embeds, creating it if needed.
    fn get_images_list(&self) -> LoroList {
        self.embeds
            .get_or_create_container("images", LoroList::new())
            .unwrap()
    }

    /// Get all images as a Vec.
    pub fn images(&self) -> Vec<EditorImage> {
        let images_list = self.get_images_list();
        let mut result = Vec::new();

        for i in 0..images_list.len() {
            if let Some(editor_image) = self.loro_value_to_editor_image(&images_list, i) {
                result.push(editor_image);
            }
        }

        result
    }

    /// Convert a LoroValue at the given index to an EditorImage.
    fn loro_value_to_editor_image(&self, list: &LoroList, index: usize) -> Option<EditorImage> {
        let value = list.get(index)?;

        // Extract LoroValue from ValueOrContainer
        let loro_value = value.as_value()?;

        // Convert LoroValue to serde_json::Value
        let json = loro_value.to_json_value();

        // Deserialize using Jacquard's from_json_value - publishedBlobUri ends up in extra_data
        let image: Image<'static> = from_json_value::<Image>(json).ok()?;

        // Extract our tracking field from extra_data
        let published_blob_uri = image
            .extra_data
            .as_ref()
            .and_then(|m| m.get("publishedBlobUri"))
            .and_then(|d| d.as_str())
            .and_then(|s| AtUri::new(s).ok())
            .map(|uri| uri.into_static());

        Some(EditorImage {
            image,
            published_blob_uri,
        })
    }

    /// Add an image to the embeds.
    /// The Image is serialized to JSON with our publishedBlobUri added.
    pub fn add_image(&mut self, image: &Image<'_>, published_blob_uri: Option<&AtUri<'_>>) {
        // Serialize the Image to serde_json::Value
        let mut json = serde_json::to_value(image).expect("Image serializes");

        // Add our tracking field (not part of lexicon, stored in extra_data on deserialize)
        if let Some(uri) = published_blob_uri {
            json.as_object_mut()
                .unwrap()
                .insert("publishedBlobUri".into(), uri.as_str().into());
        }

        // Insert into the images list
        let images_list = self.get_images_list();
        images_list.push(json).ok();
    }

    pub fn add_record(&mut self, record: &RecordEmbed<'_>) {
        // Serialize the Record embed to serde_json::Value
        let json = serde_json::to_value(record).expect("Record serializes");

        // Insert into the record list
        let record_list = self.get_records_list();
        record_list.push(json).ok();
    }

    pub fn remove_record(&mut self, index: usize) {
        let record_list = self.get_records_list();
        if index < record_list.len() {
            record_list.delete(index, 1).ok();
        }
    }

    /// Remove an image by index.
    pub fn remove_image(&mut self, index: usize) {
        let images_list = self.get_images_list();
        if index < images_list.len() {
            images_list.delete(index, 1).ok();
        }
    }

    /// Get a single image by index.
    pub fn get_image(&self, index: usize) -> Option<EditorImage> {
        let images_list = self.get_images_list();
        self.loro_value_to_editor_image(&images_list, index)
    }

    /// Get the number of images.
    pub fn images_len(&self) -> usize {
        self.get_images_list().len()
    }

    /// Update the alt text of an image at the given index.
    pub fn update_image_alt(&mut self, index: usize, alt: &str) {
        let images_list = self.get_images_list();
        if let Some(value) = images_list.get(index) {
            if let Some(loro_value) = value.as_value() {
                let mut json = loro_value.to_json_value();
                if let Some(obj) = json.as_object_mut() {
                    obj.insert("alt".into(), alt.into());
                    // Replace the entire value at this index
                    images_list.delete(index, 1).ok();
                    images_list.insert(index, json).ok();
                }
            }
        }
    }

    // --- Record embed methods ---

    /// Get the records LoroList from embeds, creating it if needed.
    fn get_records_list(&self) -> LoroList {
        self.embeds
            .get_or_create_container("records", LoroList::new())
            .unwrap()
    }

    /// Get all record embeds as a Vec.
    pub fn record_embeds(&self) -> Vec<RecordEmbed<'static>> {
        let records_list = self.get_records_list();
        let mut result = Vec::new();

        for i in 0..records_list.len() {
            if let Some(record_embed) = self.loro_value_to_record_embed(&records_list, i) {
                result.push(record_embed);
            }
        }

        result
    }

    /// Convert a LoroValue at the given index to a RecordEmbed.
    fn loro_value_to_record_embed(
        &self,
        list: &LoroList,
        index: usize,
    ) -> Option<RecordEmbed<'static>> {
        let value = list.get(index)?;
        let loro_value = value.as_value()?;
        let json = loro_value.to_json_value();
        from_json_value::<RecordEmbed>(json)
            .ok()
            .map(|r| r.into_static())
    }

    /// Insert text into content and bump content_changed for re-rendering.
    /// Edit info is tracked automatically by the buffer.
    pub fn insert_tracked(&mut self, pos: usize, text: &str) -> LoroResult<()> {
        self.buffer.insert(pos, text);
        self.content_changed.set(());
        Ok(())
    }

    /// Push text to end of content. Faster than insert for appending.
    pub fn push_tracked(&mut self, text: &str) -> LoroResult<()> {
        let pos = weaver_editor_core::TextBuffer::len_chars(&self.buffer);
        self.buffer.insert(pos, text);
        self.content_changed.set(());
        Ok(())
    }

    /// Remove text range from content and bump content_changed for re-rendering.
    /// Edit info is tracked automatically by the buffer.
    pub fn remove_tracked(&mut self, start: usize, len: usize) -> LoroResult<()> {
        self.buffer.delete(start..start + len);
        self.content_changed.set(());
        Ok(())
    }

    /// Replace text in content (atomic splice) and bump content_changed.
    /// Edit info is tracked automatically by the buffer.
    pub fn replace_tracked(&mut self, start: usize, len: usize, text: &str) -> LoroResult<()> {
        self.buffer.replace(start..start + len, text);
        self.content_changed.set(());
        Ok(())
    }

    /// Undo the last operation. Automatically updates cursor position.
    pub fn undo(&mut self) -> LoroResult<bool> {
        // Sync Loro cursor to current position BEFORE undo
        // so it tracks through the undo operation
        self.sync_loro_cursor();

        let result = self.buffer.undo();
        if result {
            // After undo, query Loro cursor for new position
            self.sync_cursor_from_loro();
            // Signal content change for re-render
            self.content_changed.set(());
        }
        Ok(result)
    }

    /// Redo the last undone operation. Automatically updates cursor position.
    pub fn redo(&mut self) -> LoroResult<bool> {
        // Sync Loro cursor to current position BEFORE redo
        self.sync_loro_cursor();

        let result = self.buffer.redo();
        if result {
            // After redo, query Loro cursor for new position
            self.sync_cursor_from_loro();
            // Signal content change for re-render
            self.content_changed.set(());
        }
        Ok(result)
    }

    /// Check if undo is available.
    pub fn can_undo(&self) -> bool {
        UndoManager::can_undo(&self.buffer)
    }

    /// Check if redo is available.
    pub fn can_redo(&self) -> bool {
        UndoManager::can_redo(&self.buffer)
    }

    /// Get a slice of the content text.
    /// Returns None if the range is invalid.
    pub fn slice(&self, start: usize, end: usize) -> Option<SmolStr> {
        self.buffer.slice(start..end)
    }

    /// Sync the Loro cursor to the current cursor.offset position.
    /// Call this after OUR edits where we know the new cursor position.
    pub fn sync_loro_cursor(&mut self) {
        let offset = self.cursor.read().offset;
        tracing::debug!(offset, "sync_loro_cursor: saving cursor position to Loro");
        self.buffer.sync_cursor(offset);
    }

    /// Update cursor.offset from the Loro cursor's tracked position.
    /// Call this after undo/redo or remote edits where the position may have shifted.
    /// Returns the new offset, or None if the cursor couldn't be resolved.
    pub fn sync_cursor_from_loro(&mut self) -> Option<usize> {
        let old_offset = self.cursor.read().offset;
        let new_offset = self.buffer.resolve_cursor()?;
        let jump = if new_offset > old_offset {
            new_offset - old_offset
        } else {
            old_offset - new_offset
        };
        if jump > 100 {
            tracing::warn!(
                old_offset,
                new_offset,
                jump,
                "sync_cursor_from_loro: LARGE CURSOR JUMP detected"
            );
        }
        tracing::debug!(
            old_offset,
            new_offset,
            "sync_cursor_from_loro: updating cursor from Loro"
        );
        self.cursor.with_mut(|c| c.offset = new_offset);
        Some(new_offset)
    }

    /// Get the Loro cursor for serialization.
    pub fn loro_cursor(&self) -> Option<Cursor> {
        self.buffer.loro_cursor()
    }

    /// Set the Loro cursor (used when restoring from storage).
    pub fn set_loro_cursor(&mut self, cursor: Option<Cursor>) {
        tracing::debug!(has_cursor = cursor.is_some(), "set_loro_cursor called");
        self.buffer.set_loro_cursor(cursor);
        // Sync cursor.offset from the restored Loro cursor
        if self.buffer.loro_cursor().is_some() {
            self.sync_cursor_from_loro();
        }
    }

    /// Export the document as a binary snapshot.
    /// This captures all CRDT state including undo history.
    pub fn export_snapshot(&self) -> Vec<u8> {
        self.buffer.export_snapshot()
    }

    /// Get the current state frontiers for change detection.
    /// Frontiers represent the "version" of the document state.
    pub fn state_frontiers(&self) -> Frontiers {
        self.buffer.doc().state_frontiers()
    }

    /// Get the current version vector.
    pub fn version_vector(&self) -> VersionVector {
        self.buffer.version()
    }

    /// Get the last edit info for incremental rendering.
    /// This comes from the buffer's internal tracking.
    pub fn last_edit(&self) -> Option<EditInfo> {
        self.buffer.last_edit()
    }

    /// Bump the content_changed signal to trigger re-renders.
    /// Call this after remote imports or other external content changes.
    pub fn notify_content_changed(&mut self) {
        self.content_changed.set(());
    }

    // --- Collected refs accessors ---

    /// Update collected refs from the render pipeline.
    pub fn set_collected_refs(&mut self, refs: Vec<weaver_common::ExtractedRef>) {
        self.collected_refs.set(refs);
    }

    /// Get AT URIs from collected embeds for populating entry.embeds.records.
    ///
    /// Filters for AtEmbed refs and parses to AtUri. Invalid URIs are skipped.
    pub fn at_embed_uris(&self) -> Vec<AtUri<'static>> {
        self.collected_refs
            .read()
            .iter()
            .filter_map(|r| match r {
                weaver_common::ExtractedRef::AtEmbed { uri, .. } => {
                    AtUri::new(uri).ok().map(|u| u.into_static())
                }
                _ => None,
            })
            .collect()
    }

    // --- Edit sync methods ---

    /// Get the edit root StrongRef if set.
    pub fn edit_root(&self) -> Option<StrongRef<'static>> {
        self.edit_root.read().clone()
    }

    /// Set the edit root after creating or finding the root record.
    pub fn set_edit_root(&mut self, root: Option<StrongRef<'static>>) {
        self.edit_root.set(root);
    }

    /// Get the last diff StrongRef if set.
    pub fn last_diff(&self) -> Option<StrongRef<'static>> {
        self.last_diff.read().clone()
    }

    /// Set the last diff after creating a new diff record.
    pub fn set_last_diff(&mut self, diff: Option<StrongRef<'static>>) {
        self.last_diff.set(diff);
    }

    /// Get the last seen diff URI for a collaborator root.
    pub fn last_seen_diff_for_root(&self, root_uri: &AtUri<'_>) -> Option<AtUri<'static>> {
        self.last_seen_diffs
            .read()
            .get(&root_uri.clone().into_static())
            .cloned()
    }

    /// Update the last seen diff for a collaborator root.
    pub fn set_last_seen_diff(&mut self, root_uri: AtUri<'static>, diff_uri: AtUri<'static>) {
        self.last_seen_diffs.write().insert(root_uri, diff_uri);
    }

    /// Check if there are unsynced changes since the last PDS sync.
    pub fn has_unsynced_changes(&self) -> bool {
        match &*self.last_synced_version.read() {
            Some(synced_vv) => self.buffer.version() != *synced_vv,
            None => true, // Never synced, so there are changes
        }
    }

    /// Export updates since the last sync.
    /// Returns None if there are no changes to export.
    /// After successful upload, call `mark_synced()` to update the sync marker.
    pub fn export_updates_since_sync(&self) -> Option<Vec<u8>> {
        let from_vv = self.last_synced_version.read().clone().unwrap_or_default();
        self.buffer.export_updates_since(&from_vv)
    }

    /// Mark the current state as synced.
    /// Call this after successfully uploading a diff to the PDS.
    pub fn mark_synced(&mut self) {
        self.last_synced_version.set(Some(self.buffer.version()));
    }

    /// Import updates from a PDS diff blob.
    /// Used when loading edit history from the PDS.
    pub fn import_updates(&mut self, updates: &[u8]) -> LoroResult<()> {
        let len_before = weaver_editor_core::TextBuffer::len_chars(&self.buffer);
        let vv_before = self.buffer.version();

        self.buffer
            .import(updates)
            .map_err(|e| loro::LoroError::DecodeError(e.to_string().into()))?;

        let len_after = weaver_editor_core::TextBuffer::len_chars(&self.buffer);
        let vv_after = self.buffer.version();
        let vv_changed = vv_before != vv_after;
        let len_changed = len_before != len_after;

        tracing::debug!(
            len_before,
            len_after,
            len_changed,
            vv_changed,
            "import_updates: merge result"
        );

        // Only trigger re-render if something actually changed
        if vv_changed {
            self.content_changed.set(());
        }
        Ok(())
    }

    /// Export updates since the given version vector.
    /// Used for real-time P2P sync where we track broadcast version separately from PDS sync.
    pub fn export_updates_from(&self, from_vv: &VersionVector) -> Option<Vec<u8>> {
        self.buffer.export_updates_since(from_vv)
    }

    /// Set the sync state when loading from PDS.
    /// This sets the version marker to the current state so we don't
    /// re-upload what we just downloaded.
    pub fn set_synced_from_pds(
        &mut self,
        edit_root: StrongRef<'static>,
        last_diff: Option<StrongRef<'static>>,
    ) {
        self.edit_root.set(Some(edit_root));
        self.last_diff.set(last_diff);
        self.last_synced_version.set(Some(self.buffer.version()));
    }

    /// Create a new SignalEditorDocument from a binary snapshot.
    /// Falls back to empty document if import fails.
    ///
    /// If `loro_cursor` is provided, it will be used to restore the cursor position.
    /// Otherwise, falls back to `fallback_offset`.
    ///
    /// Note: Undo/redo is session-only. The UndoManager tracks operations as they
    /// happen in real-time; it cannot rebuild history from imported CRDT ops.
    /// For cross-session "undo", use time travel via `doc.checkout(frontiers)`.
    ///
    /// # Note
    /// This creates Dioxus Signals for reactive fields. Call from within
    /// a component using `use_hook`.
    pub fn from_snapshot(
        snapshot: &[u8],
        loro_cursor: Option<Cursor>,
        fallback_offset: usize,
    ) -> Self {
        // Create buffer from snapshot
        let buffer = if snapshot.is_empty() {
            LoroTextBuffer::new()
        } else {
            match LoroTextBuffer::from_snapshot(snapshot) {
                Ok(buf) => buf,
                Err(e) => {
                    tracing::warn!("Failed to import snapshot: {:?}, creating empty doc", e);
                    LoroTextBuffer::new()
                }
            }
        };

        let doc = buffer.doc().clone();

        // Get other containers from the doc
        let title = doc.get_text("title");
        let path = doc.get_text("path");
        let created_at = doc.get_text("created_at");
        let tags = doc.get_list("tags");
        let embeds = doc.get_map("embeds");

        // Try to restore cursor from Loro cursor, fall back to offset
        let max_offset = weaver_editor_core::TextBuffer::len_chars(&buffer);
        let cursor_offset = if let Some(ref lc) = loro_cursor {
            doc.get_cursor_pos(lc)
                .map(|r| r.current.pos)
                .unwrap_or(fallback_offset)
        } else {
            fallback_offset
        };

        let cursor_state = CursorState {
            offset: cursor_offset.min(max_offset),
            affinity: Affinity::Before,
        };

        // Set up the Loro cursor
        let buffer = buffer;
        if let Some(lc) = loro_cursor {
            buffer.set_loro_cursor(Some(lc));
        } else {
            buffer.sync_cursor(cursor_state.offset);
        }

        Self {
            buffer,
            title,
            path,
            created_at,
            tags,
            embeds,
            entry_ref: Signal::new(None),
            notebook_uri: Signal::new(None),
            edit_root: Signal::new(None),
            last_diff: Signal::new(None),
            last_synced_version: Signal::new(None),
            last_seen_diffs: Signal::new(std::collections::HashMap::new()),
            cursor: Signal::new(cursor_state),
            selection: Signal::new(None),
            composition: Signal::new(None),
            composition_ended_at: Signal::new(None),
            content_changed: Signal::new(()),
            pending_snap: Signal::new(None),
            collected_refs: Signal::new(Vec::new()),
        }
    }

    /// Create a SignalEditorDocument from pre-loaded state.
    ///
    /// Use this when loading from PDS/localStorage merge outside reactive context.
    /// The `LoadedDocState` contains a pre-merged LoroDoc; this method wraps it
    /// with the reactive Signals needed for the editor UI.
    ///
    /// # Note
    /// This creates Dioxus Signals. Call from within a component using `use_hook`.
    pub fn from_loaded_state(state: LoadedDocState) -> Self {
        // Create buffer from the loaded doc
        let buffer = LoroTextBuffer::from_doc(state.doc.clone(), "content");
        let doc = buffer.doc().clone();

        // Get other containers from the doc
        let title = doc.get_text("title");
        let path = doc.get_text("path");
        let created_at = doc.get_text("created_at");
        let tags = doc.get_list("tags");
        let embeds = doc.get_map("embeds");

        // Position cursor at end of content
        let cursor_offset = weaver_editor_core::TextBuffer::len_chars(&buffer);
        let cursor_state = CursorState {
            offset: cursor_offset,
            affinity: Affinity::Before,
        };

        // Set up the Loro cursor
        let buffer = buffer;
        buffer.sync_cursor(cursor_offset);

        Self {
            buffer,
            title,
            path,
            created_at,
            tags,
            embeds,
            entry_ref: Signal::new(state.entry_ref),
            notebook_uri: Signal::new(state.notebook_uri),
            edit_root: Signal::new(state.edit_root),
            last_diff: Signal::new(state.last_diff),
            // Use the synced version from state (tracks the PDS version vector)
            last_synced_version: Signal::new(state.synced_version),
            last_seen_diffs: Signal::new(state.last_seen_diffs),
            cursor: Signal::new(cursor_state),
            selection: Signal::new(None),
            composition: Signal::new(None),
            composition_ended_at: Signal::new(None),
            content_changed: Signal::new(()),
            pending_snap: Signal::new(None),
            collected_refs: Signal::new(Vec::new()),
        }
    }
}

impl PartialEq for SignalEditorDocument {
    fn eq(&self, _other: &Self) -> bool {
        // SignalEditorDocument uses interior mutability, so we can't meaningfully compare.
        // Return false to ensure components re-render when passed as props.
        false
    }
}

impl weaver_editor_crdt::CrdtDocument for SignalEditorDocument {
    fn export_snapshot(&self) -> Vec<u8> {
        self.export_snapshot()
    }

    fn export_updates_since_sync(&self) -> Option<Vec<u8>> {
        self.export_updates_since_sync()
    }

    fn import(&mut self, data: &[u8]) -> Result<(), weaver_editor_crdt::CrdtError> {
        self.import_updates(data)
            .map_err(|e| weaver_editor_crdt::CrdtError::Import(e.to_string()))
    }

    fn version(&self) -> VersionVector {
        self.version_vector()
    }

    fn edit_root(&self) -> Option<StrongRef<'static>> {
        SignalEditorDocument::edit_root(self)
    }

    fn set_edit_root(&mut self, root: Option<StrongRef<'static>>) {
        SignalEditorDocument::set_edit_root(self, root);
    }

    fn last_diff(&self) -> Option<StrongRef<'static>> {
        SignalEditorDocument::last_diff(self)
    }

    fn set_last_diff(&mut self, diff: Option<StrongRef<'static>>) {
        SignalEditorDocument::set_last_diff(self, diff);
    }

    fn mark_synced(&mut self) {
        SignalEditorDocument::mark_synced(self);
    }

    fn has_unsynced_changes(&self) -> bool {
        SignalEditorDocument::has_unsynced_changes(self)
    }
}

impl EditorDocument for SignalEditorDocument {
    type Buffer = LoroTextBuffer;

    fn buffer(&self) -> &Self::Buffer {
        &self.buffer
    }

    fn buffer_mut(&mut self) -> &mut Self::Buffer {
        &mut self.buffer
    }

    fn cursor(&self) -> CursorState {
        *self.cursor.read()
    }

    fn set_cursor(&mut self, cursor: CursorState) {
        self.cursor.set(cursor);
    }

    fn selection(&self) -> Option<Selection> {
        self.selection.read().clone()
    }

    fn set_selection(&mut self, selection: Option<Selection>) {
        self.selection.set(selection);
    }

    fn last_edit(&self) -> Option<EditInfo> {
        self.buffer.last_edit()
    }

    fn set_last_edit(&mut self, _edit: Option<EditInfo>) {
        // Buffer tracks edit info internally. We use this hook to
        // bump content_changed for reactive re-rendering.
        self.content_changed.set(());
    }

    fn composition(&self) -> Option<CompositionState> {
        self.composition.read().clone()
    }

    fn set_composition(&mut self, composition: Option<CompositionState>) {
        self.composition.set(composition);
    }
}
