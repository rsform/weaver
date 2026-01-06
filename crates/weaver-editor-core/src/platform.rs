//! Platform abstraction traits for editor operations.
//!
//! These traits define the interface between the editor logic and platform-specific
//! implementations (browser DOM, native UI, etc.). This enables the same editor
//! logic to work across different platforms.

use crate::offset_map::SnapDirection;
use crate::paragraph::ParagraphRender;
use crate::types::{CursorRect, SelectionRect};

/// Error type for platform operations.
#[derive(Debug, Clone)]
pub struct PlatformError(pub String);

impl std::fmt::Display for PlatformError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for PlatformError {}

impl From<&str> for PlatformError {
    fn from(s: &str) -> Self {
        PlatformError(s.to_string())
    }
}

impl From<String> for PlatformError {
    fn from(s: String) -> Self {
        PlatformError(s)
    }
}

/// Platform-specific cursor and selection operations.
///
/// Implementations handle the actual UI interaction for cursor positioning
/// and selection rendering. The browser implementation uses the DOM Selection API,
/// native implementations would use their respective UI frameworks.
pub trait CursorPlatform {
    /// Restore cursor position in the UI after content changes.
    ///
    /// Given a character offset and rendered paragraphs, positions the cursor
    /// in the rendered content. The snap direction is used when the offset falls
    /// on invisible content (formatting syntax).
    fn restore_cursor(
        &self,
        char_offset: usize,
        paragraphs: &[ParagraphRender],
        snap_direction: Option<SnapDirection>,
    ) -> Result<(), PlatformError>;

    /// Get the screen coordinates for a cursor at the given offset.
    ///
    /// Returns None if the offset cannot be mapped to screen coordinates.
    fn get_cursor_rect(
        &self,
        char_offset: usize,
        paragraphs: &[ParagraphRender],
    ) -> Option<CursorRect>;

    /// Get screen coordinates relative to the editor container.
    ///
    /// Same as `get_cursor_rect` but coordinates are relative to the editor
    /// element rather than the viewport.
    fn get_cursor_rect_relative(
        &self,
        char_offset: usize,
        paragraphs: &[ParagraphRender],
    ) -> Option<CursorRect>;

    /// Get screen rectangles for a selection range.
    ///
    /// Returns multiple rects if the selection spans multiple lines.
    /// Coordinates are relative to the editor container.
    fn get_selection_rects_relative(
        &self,
        start: usize,
        end: usize,
        paragraphs: &[ParagraphRender],
    ) -> Vec<SelectionRect>;
}

/// Platform-specific cursor state synchronization.
///
/// Handles reading the current cursor/selection state from the platform UI
/// back into the editor model. This is the inverse of `CursorPlatform`.
pub trait CursorSync {
    /// Sync cursor state from the platform UI into the provided callbacks.
    ///
    /// The implementation reads the current selection from the UI and calls
    /// the appropriate callback with the character offset(s).
    ///
    /// - For a collapsed cursor: calls `on_cursor(offset)`
    /// - For a selection: calls `on_selection(anchor, head)`
    fn sync_cursor_from_platform<F, G>(
        &self,
        paragraphs: &[ParagraphRender],
        direction_hint: Option<SnapDirection>,
        on_cursor: F,
        on_selection: G,
    ) where
        F: FnOnce(usize),
        G: FnOnce(usize, usize);
}

/// Platform-specific clipboard operations.
///
/// Implementations handle the low-level clipboard access (sync and async paths
/// as appropriate for the platform). Document-level operations (selection
/// extraction, cursor updates) are handled by the `clipboard_*` functions
/// in this module.
pub trait ClipboardPlatform {
    /// Write plain text to clipboard.
    ///
    /// For browsers, implementations should use both the sync DataTransfer API
    /// (for immediate fallback) and the async Clipboard API (for custom MIME types).
    fn write_text(&self, text: &str);

    /// Write markdown rendered as HTML to clipboard.
    ///
    /// The `plain_text` is the original markdown, `html` is the rendered output.
    /// Both should be written to clipboard with appropriate MIME types.
    fn write_html(&self, html: &str, plain_text: &str);

    /// Read text from clipboard.
    ///
    /// For browsers, this reads from the paste event's DataTransfer.
    /// Returns None if no text is available.
    fn read_text(&self) -> Option<String>;
}

/// Strip zero-width characters used for formatting gaps.
///
/// The editor uses ZWNJ (U+200C) and ZWSP (U+200B) to create cursor positions
/// within invisible formatting syntax. These should be stripped when copying
/// text to the clipboard.
pub fn strip_zero_width(text: &str) -> String {
    text.replace('\u{200C}', "").replace('\u{200B}', "")
}

/// Copy selected text from document to clipboard.
///
/// Returns true if text was copied, false if no selection.
pub fn clipboard_copy<D: crate::EditorDocument, P: ClipboardPlatform>(
    doc: &D,
    platform: &P,
) -> bool {
    let Some(sel) = doc.selection() else {
        return false;
    };

    let (start, end) = (sel.start().min(sel.end()), sel.start().max(sel.end()));
    if start == end {
        return false;
    }

    let Some(text) = doc.slice(start..end) else {
        return false;
    };

    let clean_text = strip_zero_width(&text);
    platform.write_text(&clean_text);
    true
}

/// Cut selected text from document to clipboard.
///
/// Copies the selection to clipboard, then deletes it from the document.
/// Returns true if text was cut, false if no selection.
pub fn clipboard_cut<D: crate::EditorDocument, P: ClipboardPlatform>(
    doc: &mut D,
    platform: &P,
) -> bool {
    let Some(sel) = doc.selection() else {
        return false;
    };

    let (start, end) = (sel.start().min(sel.end()), sel.start().max(sel.end()));
    if start == end {
        return false;
    }

    let Some(text) = doc.slice(start..end) else {
        return false;
    };

    let clean_text = strip_zero_width(&text);
    platform.write_text(&clean_text);

    // Delete selection.
    doc.delete(start..end);
    doc.set_selection(None);

    true
}

/// Paste text from clipboard into document.
///
/// Replaces any selection with the pasted text, or inserts at cursor.
/// Returns true if text was pasted, false if clipboard was empty.
pub fn clipboard_paste<D: crate::EditorDocument, P: ClipboardPlatform>(
    doc: &mut D,
    platform: &P,
) -> bool {
    let Some(text) = platform.read_text() else {
        return false;
    };

    if text.is_empty() {
        return false;
    }

    // Delete selection if present.
    if let Some(sel) = doc.selection() {
        let (start, end) = (sel.start().min(sel.end()), sel.start().max(sel.end()));
        if start != end {
            doc.delete(start..end);
            doc.set_cursor_offset(start);
        }
    }
    doc.set_selection(None);

    // Insert at cursor.
    let cursor = doc.cursor_offset();
    doc.insert(cursor, &text);

    true
}

/// Render markdown to HTML using the ClientWriter.
///
/// Uses a minimal context with no embed resolution, suitable for clipboard operations.
pub fn render_markdown_to_html(markdown: &str) -> Option<String> {
    use crate::markdown_weaver::Parser;
    use crate::weaver_renderer::atproto::ClientWriter;

    let parser = Parser::new(markdown).into_offset_iter();
    let mut html = String::new();
    ClientWriter::<_, _, ()>::new(parser, &mut html, markdown)
        .run()
        .ok()?;
    Some(html)
}

/// Copy selected text as rendered HTML to clipboard.
///
/// Renders the selected markdown to HTML and writes both representations
/// to the clipboard. Returns true if text was copied, false if no selection.
pub fn clipboard_copy_as_html<D: crate::EditorDocument, P: ClipboardPlatform>(
    doc: &D,
    platform: &P,
) -> bool {
    let Some(sel) = doc.selection() else {
        return false;
    };

    let (start, end) = (sel.start().min(sel.end()), sel.start().max(sel.end()));
    if start == end {
        return false;
    }

    let Some(text) = doc.slice(start..end) else {
        return false;
    };

    let clean_text = strip_zero_width(&text);

    let Some(html) = render_markdown_to_html(&clean_text) else {
        return false;
    };

    platform.write_html(&html, &clean_text);
    true
}
