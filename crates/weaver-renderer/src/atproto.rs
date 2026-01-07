//! AT Protocol renderer for weaver notebooks
//!
//! Two-stage pipeline: markdown→markdown preprocessing (CLI),
//! then client-side markdown→HTML rendering (WASM).

mod client;
mod embed_renderer;
mod error;
mod markdown_writer;
#[cfg(not(target_family = "wasm"))]
mod preprocess;
mod types;
mod writer;

pub use client::{ClientContext, DefaultEmbedResolver, EmbedResolver};
pub use embed_renderer::{
    // Async fetch-and-render functions (require agent/network)
    fetch_and_render, fetch_and_render_generic, fetch_and_render_post, fetch_and_render_profile,
    // Pure sync render functions (pre-fetched data, no network)
    render_generic_record, render_post_view, render_profile_data_view, render_record,
};
pub use error::{AtProtoPreprocessError, ClientRenderError};
pub use markdown_writer::MarkdownWriter;
#[cfg(not(target_family = "wasm"))]
pub use preprocess::AtProtoPreprocessContext;
pub use types::{BlobInfo, BlobName};
pub use writer::{ClientWriter, EmbedContentProvider};

#[cfg(test)]
mod tests;
