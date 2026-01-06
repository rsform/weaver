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
//! - `events`: beforeinput, keydown, paste event handlers
//! - `contenteditable`: Editor element setup and management
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

// Browser cursor implementation
pub use cursor::BrowserCursor;

// Platform detection
pub use platform::{Platform, platform};

// TODO: contenteditable module
// TODO: embed worker module
