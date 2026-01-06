//! Browser DOM layer for the weaver markdown editor.
//!
//! This crate provides DOM manipulation and browser event handling,
//! generic over any `EditorDocument` implementation. It assumes a
//! `wasm32-unknown-unknown` target environment.
//!
//! # Architecture
//!
//! - `cursor`: Selection API handling and cursor restoration
//! - `dom_sync`: DOM ↔ document state synchronization
//! - `events`: beforeinput event handling and clipboard helpers
//! - `platform`: Browser/OS detection for platform-specific behavior
//!
//! # DOM Update Strategy
//!
//! The [`FORCE_INNERHTML_UPDATE`] constant controls how DOM updates are handled:
//!
//! - `true`: Editor always owns DOM updates. `handle_beforeinput` returns `Handled`,
//!   and `update_paragraph_dom` always replaces innerHTML. This is more predictable
//!   but can interfere with IME and cause flickering.
//!
//! - `false`: For simple edits (plain text insertion, single char deletion),
//!   `handle_beforeinput` can return `PassThrough` to let the browser update the DOM
//!   directly while we just track changes in the model. `update_paragraph_dom` will
//!   skip innerHTML replacement for the cursor paragraph if syntax is unchanged.
//!   This is smoother but requires careful coordination.
//!
//! # Re-exports
//!
//! This crate re-exports `weaver-editor-core` for convenience, so consumers
//! only need to depend on `weaver-editor-browser`.

// Re-export core crate
pub use weaver_editor_core;
pub use weaver_editor_core::*;

/// Controls DOM update strategy.
///
/// When `true`, the editor always owns DOM updates:
/// - `handle_beforeinput` returns `Handled` (preventDefault)
/// - `update_paragraph_dom` always replaces innerHTML
///
/// When `false`, simple edits can be handled by the browser:
/// - `handle_beforeinput` returns `PassThrough` for plain text inserts/deletes
/// - `update_paragraph_dom` skips innerHTML for cursor paragraph if syntax unchanged
///
/// Set to `true` for maximum control, `false` for smoother typing experience.
pub const FORCE_INNERHTML_UPDATE: bool = true;

pub mod color;
pub mod cursor;
pub mod dom_sync;
pub mod events;
pub mod platform;
pub mod visibility;

// Browser cursor implementation
pub use cursor::{
    BrowserCursor, find_text_node_at_offset, get_cursor_rect, get_cursor_rect_relative,
    get_selection_rects_relative, restore_cursor_position,
};

// DOM sync types
pub use dom_sync::{
    BrowserCursorSync, CursorSyncResult, dom_position_to_text_offset, sync_cursor_from_dom_impl,
    update_paragraph_dom,
};

// Event handling
pub use events::{
    BeforeInputContext, BeforeInputResult, StaticRange, copy_as_html, get_current_range,
    get_data_from_event, get_input_type_from_event, get_target_range_from_event,
    handle_beforeinput, is_composing, parse_browser_input_type, read_clipboard_text,
    write_clipboard_with_custom_type,
};

// Platform detection
pub use platform::{Platform, platform};

// Visibility updates
pub use visibility::update_syntax_visibility;

// Color utilities
pub use color::{rgba_u32_to_css, rgba_u32_to_css_alpha};
