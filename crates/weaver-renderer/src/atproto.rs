//! AT Protocol renderer for weaver notebooks
//!
//! Two-stage pipeline: markdown→markdown preprocessing (CLI),
//! then client-side markdown→HTML rendering (WASM).

mod error;
mod types;
mod markdown_writer;
mod preprocess;
mod client;
mod embed_renderer;
mod writer;

pub use error::{AtProtoPreprocessError, ClientRenderError};
pub use types::{BlobName, BlobInfo};
pub use preprocess::AtProtoPreprocessContext;
pub use client::{ClientContext, EmbedResolver, DefaultEmbedResolver};
pub use markdown_writer::MarkdownWriter;
pub use embed_renderer::{fetch_and_render_profile, fetch_and_render_post, fetch_and_render_generic};
pub use writer::{ClientWriter, EmbedContentProvider};
