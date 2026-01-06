//! Error types for CRDT operations.

use thiserror::Error;

/// Errors that can occur during CRDT operations.
#[derive(Error, Debug)]
#[non_exhaustive]
pub enum CrdtError {
    /// Failed to import CRDT data.
    #[error("failed to import CRDT data: {0}")]
    Import(String),

    /// Failed to export CRDT data.
    #[error("failed to export CRDT data: {0}")]
    Export(String),

    /// Sync operation failed.
    #[error("sync failed: {0}")]
    Sync(String),

    /// Not authenticated.
    #[error("not authenticated")]
    NotAuthenticated,

    /// Invalid AT-URI.
    #[error("invalid AT-URI: {0}")]
    InvalidUri(String),

    /// XRPC call failed.
    #[error("XRPC error: {0}")]
    Xrpc(String),

    /// Serialization error.
    #[error("serialization error: {0}")]
    Serialization(String),

    /// Loro CRDT error.
    #[error("loro error: {0}")]
    Loro(String),
}

impl From<loro::LoroError> for CrdtError {
    fn from(e: loro::LoroError) -> Self {
        CrdtError::Import(e.to_string())
    }
}

impl From<jacquard::client::AgentError> for CrdtError {
    fn from(e: jacquard::client::AgentError) -> Self {
        CrdtError::Xrpc(e.to_string())
    }
}
