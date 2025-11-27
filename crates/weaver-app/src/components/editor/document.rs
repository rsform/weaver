//! Core data structures for the markdown editor.
//!
//! Uses Loro CRDT for text storage with built-in undo/redo support.
//! Mirrors the `sh.weaver.notebook.entry` schema for AT Protocol integration.

use loro::{
    ExportMode, LoroDoc, LoroList, LoroMap, LoroResult, LoroText, LoroValue, ToJson, UndoManager,
    cursor::{Cursor, Side},
};

use jacquard::IntoStatic;
use jacquard::from_json_value;
use jacquard::types::string::AtUri;
use weaver_api::sh_weaver::embed::images::Image;

/// Helper for working with editor images.
/// Constructed from LoroMap data, NOT serialized directly.
/// The Image lexicon type stores our `publishedBlobUri` in its `extra_data` field.
#[derive(Clone, Debug)]
pub struct EditorImage {
    /// The lexicon Image type (deserialized via from_json_value)
    pub image: Image<'static>,
    /// AT-URI of the PublishedBlob record (for cleanup on publish/delete)
    /// None for existing images that are already in an entry record.
    pub published_blob_uri: Option<AtUri<'static>>,
}

/// Single source of truth for editor state.
///
/// Contains the document text (backed by Loro CRDT), cursor position,
/// selection, and IME composition state. Mirrors the `sh.weaver.notebook.entry`
/// schema with CRDT containers for each field.
#[derive(Debug)]
pub struct EditorDocument {
    /// The Loro document containing all editor state.
    doc: LoroDoc,

    // --- Entry schema containers ---
    /// Markdown content (maps to entry.content)
    content: LoroText,

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

    // --- Editor state ---
    /// Undo manager for the document.
    undo_mgr: UndoManager,

    /// Current cursor position (char offset) - fast local cache.
    /// This is the authoritative position for immediate operations.
    pub cursor: CursorState,

    /// CRDT-aware cursor that tracks position through remote edits and undo/redo.
    /// Recreated after our own edits, queried after undo/redo/remote edits.
    loro_cursor: Option<Cursor>,

    /// Active selection if any
    pub selection: Option<Selection>,

    /// IME composition state (for Phase 3)
    pub composition: Option<CompositionState>,

    /// Timestamp when the last composition ended.
    /// Used for Safari workaround: ignore Enter keydown within 500ms of compositionend.
    pub composition_ended_at: Option<web_time::Instant>,

    /// Most recent edit info for incremental rendering optimization.
    /// Used to determine if we can skip full re-parsing.
    pub last_edit: Option<EditInfo>,
}

/// Cursor state including position and affinity.
#[derive(Clone, Debug, Copy)]
pub struct CursorState {
    /// Character offset in text (NOT byte offset!)
    pub offset: usize,

    /// Prefer left/right when at boundary (for vertical cursor movement)
    pub affinity: Affinity,
}

/// Cursor affinity for vertical movement.
#[derive(Clone, Debug, Copy, PartialEq, Eq)]
pub enum Affinity {
    Before,
    After,
}

/// Text selection with anchor and head positions.
#[derive(Clone, Debug, Copy)]
pub struct Selection {
    /// Where selection started
    pub anchor: usize,
    /// Where cursor is now
    pub head: usize,
}

/// IME composition state (for international text input).
#[derive(Clone, Debug)]
pub struct CompositionState {
    pub start_offset: usize,
    pub text: String,
}

/// Information about the most recent edit, used for incremental rendering optimization.
#[derive(Clone, Debug, Default)]
pub struct EditInfo {
    /// Character offset where the edit occurred
    pub edit_char_pos: usize,
    /// Number of characters inserted
    pub inserted_len: usize,
    /// Number of characters deleted
    pub deleted_len: usize,
    /// Whether the edit contains a newline (boundary-affecting)
    pub contains_newline: bool,
    /// Whether the edit is in the block-syntax zone of a line (first ~6 chars).
    /// Edits here could affect block-level syntax like headings, lists, code fences.
    pub in_block_syntax_zone: bool,
    /// Document length (in chars) after this edit was applied.
    /// Used to detect stale edit info - if current doc length doesn't match,
    /// the edit info is from a previous render cycle and shouldn't be used.
    pub doc_len_after: usize,
}

/// Max distance from line start where block syntax can appear.
/// Covers: `######` (6), ```` ``` ```` (3), `> ` (2), `- ` (2), `999. ` (5)
const BLOCK_SYNTAX_ZONE: usize = 6;

impl EditorDocument {
    /// Check if a character position is within the block-syntax zone of its line.
    fn is_in_block_syntax_zone(&self, pos: usize) -> bool {
        if pos == 0 {
            return true;
        }

        let content_str = self.content.to_string();
        let mut last_newline_pos: Option<usize> = None;

        for (i, c) in content_str.chars().take(pos).enumerate() {
            if c == '\n' {
                last_newline_pos = Some(i);
            }
        }

        let chars_from_line_start = match last_newline_pos {
            Some(nl_pos) => pos - nl_pos - 1,
            None => pos,
        };

        chars_from_line_start <= BLOCK_SYNTAX_ZONE
    }

    /// Create a new editor document with the given content.
    /// Sets `created_at` to current time.
    pub fn new(initial_content: String) -> Self {
        let doc = LoroDoc::new();

        // Get all containers
        let content = doc.get_text("content");
        let title = doc.get_text("title");
        let path = doc.get_text("path");
        let created_at = doc.get_text("created_at");
        let tags = doc.get_list("tags");
        let embeds = doc.get_map("embeds");

        // Insert initial content if any
        if !initial_content.is_empty() {
            content
                .insert(0, &initial_content)
                .expect("failed to insert initial content");
        }

        // Set created_at to current time (ISO 8601)
        let now = Self::current_datetime_string();
        created_at
            .insert(0, &now)
            .expect("failed to set created_at");

        // Set up undo manager with merge interval for batching keystrokes
        let mut undo_mgr = UndoManager::new(&doc);
        undo_mgr.set_merge_interval(300); // 300ms merge window
        undo_mgr.set_max_undo_steps(100);

        // Create initial Loro cursor at position 0
        let loro_cursor = content.get_cursor(0, Side::default());

        Self {
            doc,
            content,
            title,
            path,
            created_at,
            tags,
            embeds,
            undo_mgr,
            cursor: CursorState {
                offset: 0,
                affinity: Affinity::Before,
            },
            loro_cursor,
            selection: None,
            composition: None,
            composition_ended_at: None,
            last_edit: None,
        }
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
        &self.content
    }

    // --- Content accessors ---

    /// Get the markdown content as a string.
    pub fn content(&self) -> String {
        self.content.to_string()
    }

    /// Convert the document content to a string (alias for content()).
    pub fn to_string(&self) -> String {
        self.content.to_string()
    }

    /// Get the length of the content in characters.
    pub fn len_chars(&self) -> usize {
        self.content.len_unicode()
    }

    /// Get the length of the content in UTF-8 bytes.
    pub fn len_bytes(&self) -> usize {
        self.content.len_utf8()
    }

    /// Get the length of the content in UTF-16 code units.
    pub fn len_utf16(&self) -> usize {
        self.content.len_utf16()
    }

    /// Check if the content is empty.
    pub fn is_empty(&self) -> bool {
        self.content.len_unicode() == 0
    }

    // --- Entry metadata accessors ---

    /// Get the entry title.
    pub fn title(&self) -> String {
        self.title.to_string()
    }

    /// Set the entry title (replaces existing).
    pub fn set_title(&mut self, new_title: &str) {
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
    pub fn set_path(&mut self, new_path: &str) {
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
    pub fn set_created_at(&mut self, datetime: &str) {
        let current_len = self.created_at.len_unicode();
        if current_len > 0 {
            self.created_at.delete(0, current_len).ok();
        }
        self.created_at.insert(0, datetime).ok();
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
    pub fn add_tag(&mut self, tag: &str) {
        let existing = self.tags();
        if !existing.iter().any(|t| t == tag) {
            self.tags.push(LoroValue::String(tag.into())).ok();
        }
    }

    /// Remove a tag by value.
    pub fn remove_tag(&mut self, tag: &str) {
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
    pub fn clear_tags(&mut self) {
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

    /// Insert text into content and record edit info for incremental rendering.
    pub fn insert_tracked(&mut self, pos: usize, text: &str) -> LoroResult<()> {
        let in_block_syntax_zone = self.is_in_block_syntax_zone(pos);
        let len_before = self.content.len_unicode();
        let result = self.content.insert(pos, text);
        let len_after = self.content.len_unicode();
        self.last_edit = Some(EditInfo {
            edit_char_pos: pos,
            inserted_len: len_after.saturating_sub(len_before),
            deleted_len: 0,
            contains_newline: text.contains('\n'),
            in_block_syntax_zone,
            doc_len_after: len_after,
        });
        result
    }

    /// Push text to end of content. Faster than insert for appending.
    pub fn push_tracked(&mut self, text: &str) -> LoroResult<()> {
        let pos = self.content.len_unicode();
        let in_block_syntax_zone = self.is_in_block_syntax_zone(pos);
        let result = self.content.push_str(text);
        let len_after = self.content.len_unicode();
        self.last_edit = Some(EditInfo {
            edit_char_pos: pos,
            inserted_len: text.chars().count(),
            deleted_len: 0,
            contains_newline: text.contains('\n'),
            in_block_syntax_zone,
            doc_len_after: len_after,
        });
        result
    }

    /// Remove text range from content and record edit info for incremental rendering.
    pub fn remove_tracked(&mut self, start: usize, len: usize) -> LoroResult<()> {
        let content_str = self.content.to_string();
        let contains_newline = content_str.chars().skip(start).take(len).any(|c| c == '\n');
        let in_block_syntax_zone = self.is_in_block_syntax_zone(start);

        let result = self.content.delete(start, len);
        self.last_edit = Some(EditInfo {
            edit_char_pos: start,
            inserted_len: 0,
            deleted_len: len,
            contains_newline,
            in_block_syntax_zone,
            doc_len_after: self.content.len_unicode(),
        });
        result
    }

    /// Replace text in content (delete then insert) and record combined edit info.
    pub fn replace_tracked(&mut self, start: usize, len: usize, text: &str) -> LoroResult<()> {
        let content_str = self.content.to_string();
        let delete_has_newline = content_str.chars().skip(start).take(len).any(|c| c == '\n');
        let in_block_syntax_zone = self.is_in_block_syntax_zone(start);

        let len_before = self.content.len_unicode();
        // Use splice for atomic replace
        self.content.splice(start, len, text)?;
        let len_after = self.content.len_unicode();

        // inserted_len = (len_after - len_before) + deleted_len
        // because: len_after = len_before - deleted + inserted
        let inserted_len = (len_after + len).saturating_sub(len_before);

        self.last_edit = Some(EditInfo {
            edit_char_pos: start,
            inserted_len,
            deleted_len: len,
            contains_newline: delete_has_newline || text.contains('\n'),
            in_block_syntax_zone,
            doc_len_after: len_after,
        });
        Ok(())
    }

    /// Undo the last operation.
    /// Returns true if an undo was performed.
    /// Automatically updates cursor position from the Loro cursor.
    pub fn undo(&mut self) -> LoroResult<bool> {
        // Sync Loro cursor to current position BEFORE undo
        // so it tracks through the undo operation
        self.sync_loro_cursor();

        let result = self.undo_mgr.undo()?;
        if result {
            // After undo, query Loro cursor for new position
            self.sync_cursor_from_loro();
        }
        Ok(result)
    }

    /// Redo the last undone operation.
    /// Returns true if a redo was performed.
    /// Automatically updates cursor position from the Loro cursor.
    pub fn redo(&mut self) -> LoroResult<bool> {
        // Sync Loro cursor to current position BEFORE redo
        self.sync_loro_cursor();

        let result = self.undo_mgr.redo()?;
        if result {
            // After redo, query Loro cursor for new position
            self.sync_cursor_from_loro();
        }
        Ok(result)
    }

    /// Check if undo is available.
    pub fn can_undo(&self) -> bool {
        self.undo_mgr.can_undo()
    }

    /// Check if redo is available.
    pub fn can_redo(&self) -> bool {
        self.undo_mgr.can_redo()
    }

    /// Get a slice of the content text.
    /// Returns None if the range is invalid.
    pub fn slice(&self, start: usize, end: usize) -> Option<String> {
        self.content.slice(start, end).ok()
    }

    /// Sync the Loro cursor to the current cursor.offset position.
    /// Call this after OUR edits where we know the new cursor position.
    pub fn sync_loro_cursor(&mut self) {
        self.loro_cursor = self.content.get_cursor(self.cursor.offset, Side::default());
    }

    /// Update cursor.offset from the Loro cursor's tracked position.
    /// Call this after undo/redo or remote edits where the position may have shifted.
    /// Returns the new offset, or None if the cursor couldn't be resolved.
    pub fn sync_cursor_from_loro(&mut self) -> Option<usize> {
        let loro_cursor = self.loro_cursor.as_ref()?;
        let result = self.doc.get_cursor_pos(loro_cursor).ok()?;
        let new_offset = result.current.pos;
        self.cursor.offset = new_offset.min(self.len_chars());
        Some(self.cursor.offset)
    }

    /// Get the Loro cursor for serialization.
    pub fn loro_cursor(&self) -> Option<&Cursor> {
        self.loro_cursor.as_ref()
    }

    /// Set the Loro cursor (used when restoring from storage).
    pub fn set_loro_cursor(&mut self, cursor: Option<Cursor>) {
        self.loro_cursor = cursor;
        // Sync cursor.offset from the restored Loro cursor
        if self.loro_cursor.is_some() {
            self.sync_cursor_from_loro();
        }
    }

    /// Export the document as a binary snapshot.
    /// This captures all CRDT state including undo history.
    pub fn export_snapshot(&self) -> Vec<u8> {
        self.doc.export(ExportMode::Snapshot).unwrap_or_default()
    }

    /// Get the current state frontiers for change detection.
    /// Frontiers represent the "version" of the document state.
    pub fn state_frontiers(&self) -> loro::Frontiers {
        self.doc.state_frontiers()
    }

    /// Create a new EditorDocument from a binary snapshot.
    /// Falls back to empty document if import fails.
    ///
    /// If `loro_cursor` is provided, it will be used to restore the cursor position.
    /// Otherwise, falls back to `fallback_offset`.
    ///
    /// Note: Undo/redo is session-only. The UndoManager tracks operations as they
    /// happen in real-time; it cannot rebuild history from imported CRDT ops.
    /// For cross-session "undo", use time travel via `doc.checkout(frontiers)`.
    pub fn from_snapshot(
        snapshot: &[u8],
        loro_cursor: Option<Cursor>,
        fallback_offset: usize,
    ) -> Self {
        let doc = LoroDoc::new();

        if !snapshot.is_empty() {
            if let Err(e) = doc.import(snapshot) {
                tracing::warn!("Failed to import snapshot: {:?}, creating empty doc", e);
            }
        }

        // Get all containers (they will contain data from the snapshot if import succeeded)
        let content = doc.get_text("content");
        let title = doc.get_text("title");
        let path = doc.get_text("path");
        let created_at = doc.get_text("created_at");
        let tags = doc.get_list("tags");
        let embeds = doc.get_map("embeds");

        // Set up undo manager - tracks operations from this point forward only
        let mut undo_mgr = UndoManager::new(&doc);
        undo_mgr.set_merge_interval(300);
        undo_mgr.set_max_undo_steps(100);

        // Try to restore cursor from Loro cursor, fall back to offset
        let max_offset = content.len_unicode();
        let cursor_offset = if let Some(ref lc) = loro_cursor {
            doc.get_cursor_pos(lc)
                .map(|r| r.current.pos)
                .unwrap_or(fallback_offset)
        } else {
            fallback_offset
        };

        let cursor = CursorState {
            offset: cursor_offset.min(max_offset),
            affinity: Affinity::Before,
        };

        // If no Loro cursor provided, create one at the restored position
        let loro_cursor =
            loro_cursor.or_else(|| content.get_cursor(cursor.offset, Side::default()));

        Self {
            doc,
            content,
            title,
            path,
            created_at,
            tags,
            embeds,
            undo_mgr,
            cursor,
            loro_cursor,
            selection: None,
            composition: None,
            composition_ended_at: None,
            last_edit: None,
        }
    }
}

// EditorDocument can't derive Clone because LoroDoc/LoroText/UndoManager don't implement Clone.
// This is intentional - the document should be the single source of truth.

impl Clone for EditorDocument {
    fn clone(&self) -> Self {
        // Use snapshot export/import for a complete clone including all containers
        let snapshot = self.export_snapshot();
        let mut new_doc =
            Self::from_snapshot(&snapshot, self.loro_cursor.clone(), self.cursor.offset);

        // Copy non-CRDT state
        new_doc.cursor = self.cursor;
        new_doc.sync_loro_cursor();
        new_doc.selection = self.selection;
        new_doc.composition = self.composition.clone();
        new_doc.composition_ended_at = self.composition_ended_at;
        new_doc.last_edit = self.last_edit.clone();
        new_doc
    }
}
