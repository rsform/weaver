use miette::Diagnostic;
use thiserror::Error;

#[derive(Debug, Error, Diagnostic)]
pub enum AtProtoPreprocessError {
    #[error("blob upload failed: {0}")]
    #[diagnostic(code(atproto::preprocess::blob_upload))]
    BlobUpload(String, #[source] Box<dyn std::error::Error + Send + Sync>),

    #[error("failed to resolve handle {handle} to DID")]
    #[diagnostic(code(atproto::preprocess::handle_resolution))]
    HandleResolution {
        handle: String,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    #[error("entry not found in vault: {0}")]
    #[diagnostic(code(atproto::preprocess::entry_not_found))]
    EntryNotFound(String),

    #[error("invalid image path: {0}")]
    #[diagnostic(code(atproto::preprocess::invalid_image))]
    InvalidImage(String),

    #[error("io error: {0}")]
    #[diagnostic(code(atproto::preprocess::io))]
    Io(#[from] std::io::Error),

    #[error("invalid AT URI: {0}")]
    #[diagnostic(code(atproto::preprocess::invalid_uri))]
    InvalidUri(String),

    #[error("failed to fetch record: {0}")]
    #[diagnostic(code(atproto::preprocess::fetch_failed))]
    FetchFailed(String),

    #[error("failed to parse record: {0}")]
    #[diagnostic(code(atproto::preprocess::parse_failed))]
    ParseFailed(String),
}

#[derive(Debug, Error, Diagnostic)]
pub enum ClientRenderError {
    #[error("failed to fetch embedded entry: {uri}")]
    #[diagnostic(code(atproto::client::entry_fetch))]
    EntryFetch {
        uri: String,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    #[error("blob not found in entry embeds: {name}")]
    #[diagnostic(code(atproto::client::blob_not_found))]
    BlobNotFound { name: String },
}
