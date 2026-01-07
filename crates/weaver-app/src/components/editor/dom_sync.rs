//! DOM synchronization for the markdown editor.
//!
//! Most DOM sync logic is in `weaver_editor_browser`. This module re-exports
//! the `update_paragraph_dom` function for use in the editor component.

// Re-export from browser crate.
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub use weaver_editor_browser::update_paragraph_dom;
