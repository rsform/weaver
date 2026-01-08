//! WASM bindings for the weaver markdown editor.
//!
//! Provides embeddable editor components for JavaScript/TypeScript apps.
//!
//! # Features
//!
//! - `collab`: Enable collaborative editing via Loro CRDT + iroh P2P
//! - `syntax-highlighting`: Enable syntax highlighting for code blocks

mod actions;
mod editor;
mod types;

#[cfg(feature = "collab")]
mod collab;

pub use actions::*;
pub use editor::*;
pub use types::*;

#[cfg(feature = "collab")]
pub use collab::*;

use wasm_bindgen::prelude::*;

/// Initialize panic hook for better error messages in console.
#[wasm_bindgen(start)]
pub fn init() {
    console_error_panic_hook::set_once();
}
