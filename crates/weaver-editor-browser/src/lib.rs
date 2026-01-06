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
//! # Re-exports
//!
//! This crate re-exports `weaver-editor-core` for convenience, so consumers
//! only need to depend on `weaver-editor-browser`.

// Re-export core crate
pub use weaver_editor_core;
pub use weaver_editor_core::*;

pub mod cursor;
pub mod dom_sync;
pub mod events;
pub mod platform;
pub mod visibility;

// Browser cursor implementation
pub use cursor::{BrowserCursor, find_text_node_at_offset, restore_cursor_position};

// DOM sync types
pub use dom_sync::{
    BrowserCursorSync, CursorSyncResult, ParagraphDomData, dom_position_to_text_offset,
    sync_cursor_from_dom_impl, update_paragraph_dom,
};

// Event handling
pub use events::{
    BeforeInputContext, BeforeInputResult, StaticRange, copy_as_html, get_data_from_event,
    get_input_type_from_event, get_target_range_from_event, handle_beforeinput, is_composing,
    parse_browser_input_type, read_clipboard_text, write_clipboard_with_custom_type,
};

// Platform detection
pub use platform::{Platform, platform};

// Visibility updates
pub use visibility::update_syntax_visibility;
