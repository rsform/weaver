#![cfg(feature = "iroh")]

//! Peer discovery via AT Protocol session records.
//!
//! Collaborators publish `sh.weaver.collab.session` records to their PDS
//! when joining a session. This module provides utilities for:
//! - Parsing NodeIds from session records
//! - Converting between EndpointId and z-base32 strings

use iroh::EndpointId;
use miette::Diagnostic;
use std::str::FromStr;

/// Error type for discovery operations
#[derive(Debug, thiserror::Error, Diagnostic)]
#[diagnostic(code(weaver::transport::discovery))]
pub enum DiscoveryError {
    #[error("invalid node ID: {0}")]
    InvalidNodeId(String),

    #[error("failed to fetch session records")]
    FetchError(#[source] Box<dyn std::error::Error + Send + Sync>),
}

/// Parse an EndpointId from a z-base32 encoded string.
///
/// Session records store NodeIds as z-base32 strings. This converts
/// them back to EndpointId for use with iroh.
pub fn parse_node_id(s: &str) -> Result<EndpointId, DiscoveryError> {
    EndpointId::from_str(s).map_err(|_| DiscoveryError::InvalidNodeId(s.to_string()))
}

/// Convert an EndpointId to a z-base32 string for storage in session records.
pub fn node_id_to_string(id: &EndpointId) -> String {
    id.to_string()
}

/// Information about a discovered peer from a session record.
#[derive(Debug, Clone)]
pub struct DiscoveredPeer {
    /// The peer's iroh EndpointId
    pub node_id: EndpointId,
    /// Optional relay URL for browser clients
    pub relay_url: Option<String>,
}

impl DiscoveredPeer {
    /// Parse a DiscoveredPeer from a session record's fields.
    pub fn from_session_fields(
        node_id: &str,
        relay_url: Option<&str>,
    ) -> Result<Self, DiscoveryError> {
        Ok(Self {
            node_id: parse_node_id(node_id)?,
            relay_url: relay_url.map(String::from),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_node_id_roundtrip() {
        // Generate a random key and get its EndpointId
        let secret = iroh::SecretKey::generate(&mut rand::rng());
        let endpoint_id = secret.public();

        // Convert to string and back
        let s = node_id_to_string(&endpoint_id);
        let parsed = parse_node_id(&s).unwrap();

        assert_eq!(endpoint_id, parsed);
    }

    #[test]
    fn test_invalid_node_id() {
        let result = parse_node_id("not-a-valid-node-id");
        assert!(result.is_err());
    }
}
