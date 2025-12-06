//! CollabNode - iroh endpoint with gossip router for real-time collaboration.

use iroh::Endpoint;
use iroh::EndpointId;
use iroh::SecretKey;
use iroh_gossip::net::Gossip;
use miette::Diagnostic;
use std::sync::Arc;

use super::WEAVER_GOSSIP_ALPN;

/// Error type for transport operations
#[derive(Debug, thiserror::Error, Diagnostic)]
#[diagnostic(code(weaver::transport))]
pub enum TransportError {
    #[error("failed to bind endpoint")]
    Bind(#[source] Box<dyn std::error::Error + Send + Sync>),

    #[error("gossip error")]
    Gossip(#[source] Box<dyn std::error::Error + Send + Sync>),
}

/// A collaboration node wrapping an iroh endpoint and gossip router.
///
/// There should be one CollabNode per application instance. It manages:
/// - The iroh QUIC endpoint (with automatic relay fallback for browsers)
/// - The gossip protocol handler
/// - The protocol router for ALPN dispatch
pub struct CollabNode {
    endpoint: Endpoint,
    gossip: Gossip,
    #[allow(dead_code)]
    router: iroh::protocol::Router,
    secret_key: SecretKey,
}

impl CollabNode {
    /// Spawn a new collaboration node.
    ///
    /// If no secret key is provided, a new one is generated. For browsers,
    /// this means each session gets a fresh identity (published to PDS via
    /// session records for peer discovery).
    pub async fn spawn(secret_key: Option<SecretKey>) -> Result<Arc<Self>, TransportError> {
        let secret_key = secret_key.unwrap_or_else(|| SecretKey::generate(&mut rand::rng()));

        // Build endpoint with gossip ALPN
        // In WASM, this automatically uses relay-only mode
        // In native, this can do direct P2P with relay fallback
        let endpoint = Endpoint::builder()
            .secret_key(secret_key.clone())
            .alpns(vec![WEAVER_GOSSIP_ALPN.to_vec()])
            .bind()
            .await
            .map_err(|e| TransportError::Bind(Box::new(e)))?;

        // Build gossip protocol handler
        let gossip = Gossip::builder().spawn(endpoint.clone());

        // Build router to dispatch incoming connections by ALPN
        let router = iroh::protocol::Router::builder(endpoint.clone())
            .accept(WEAVER_GOSSIP_ALPN, gossip.clone())
            .spawn();

        tracing::info!(node_id = %endpoint.id(), "CollabNode started");

        Ok(Arc::new(Self {
            endpoint,
            gossip,
            router,
            secret_key,
        }))
    }

    /// Get this node's public identifier.
    ///
    /// This should be published to the user's PDS in a session record
    /// so other collaborators can discover and connect to this node.
    pub fn node_id(&self) -> EndpointId {
        self.endpoint.id()
    }

    /// Get the node ID as a z-base32 string for storage in AT Protocol records.
    pub fn node_id_string(&self) -> String {
        self.endpoint.id().to_string()
    }

    /// Get a reference to the gossip handler for joining topics.
    pub fn gossip(&self) -> &Gossip {
        &self.gossip
    }

    /// Get a reference to the underlying endpoint.
    pub fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }

    /// Get a clone of the secret key (for session persistence if needed).
    pub fn secret_key(&self) -> SecretKey {
        self.secret_key.clone()
    }
}
