//! Platform abstraction traits for editor operations.
//!
//! These traits define the interface between the editor logic and platform-specific
//! implementations (browser DOM, native UI, etc.). This enables the same editor
//! logic to work across different platforms.

use crate::offset_map::SnapDirection;
use crate::types::{CursorRect, SelectionRect};
use crate::OffsetMapping;

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
    /// Given a character offset and the current offset map, positions the cursor
    /// in the rendered content. The snap direction is used when the offset falls
    /// on invisible content (formatting syntax).
    fn restore_cursor(
        &self,
        char_offset: usize,
        offset_map: &[OffsetMapping],
        snap_direction: Option<SnapDirection>,
    ) -> Result<(), PlatformError>;

    /// Get the screen coordinates for a cursor at the given offset.
    ///
    /// Returns None if the offset cannot be mapped to screen coordinates.
    fn get_cursor_rect(
        &self,
        char_offset: usize,
        offset_map: &[OffsetMapping],
    ) -> Option<CursorRect>;

    /// Get screen coordinates relative to the editor container.
    ///
    /// Same as `get_cursor_rect` but coordinates are relative to the editor
    /// element rather than the viewport.
    fn get_cursor_rect_relative(
        &self,
        char_offset: usize,
        offset_map: &[OffsetMapping],
    ) -> Option<CursorRect>;

    /// Get screen rectangles for a selection range.
    ///
    /// Returns multiple rects if the selection spans multiple lines.
    /// Coordinates are relative to the editor container.
    fn get_selection_rects_relative(
        &self,
        start: usize,
        end: usize,
        offset_map: &[OffsetMapping],
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
        offset_map: &[OffsetMapping],
        direction_hint: Option<SnapDirection>,
        on_cursor: F,
        on_selection: G,
    ) where
        F: FnOnce(usize),
        G: FnOnce(usize, usize);
}
