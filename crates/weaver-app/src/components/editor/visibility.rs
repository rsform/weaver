//! Conditional syntax visibility based on cursor position.
//!
//! Re-exports core visibility logic and browser DOM updates.

// Core visibility calculation.
pub use weaver_editor_core::VisibilityState;

// Browser DOM updates.
pub use weaver_editor_browser::update_syntax_visibility;
