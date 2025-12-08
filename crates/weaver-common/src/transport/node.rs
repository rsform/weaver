#![cfg(feature = "iroh")]

//! CollabNode - iroh endpoint with gossip router for real-time collaboration.

use iroh::Endpoint;
use iroh::EndpointId;
use iroh::SecretKey;
use iroh_gossip::net::{GOSSIP_ALPN, Gossip};
use miette::Diagnostic;
use std::sync::Arc;

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
            .alpns(vec![GOSSIP_ALPN.to_vec()])
            .bind()
            .await
            .map_err(|e| TransportError::Bind(Box::new(e)))?;

        // Build gossip protocol handler
        let gossip = Gossip::builder().spawn(endpoint.clone());

        // Build router to dispatch incoming connections by ALPN
        let router = iroh::protocol::Router::builder(endpoint.clone())
            .accept(GOSSIP_ALPN, gossip.clone())
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

    /// Get the relay URL this node is connected to (if any).
    ///
    /// This should be published in session records so other peers can connect
    /// via relay (essential for browser-to-browser connections).
    pub fn relay_url(&self) -> Option<String> {
        self.endpoint
            .addr()
            .relay_urls()
            .next()
            .map(|url| url.to_string())
    }

    /// Get the full node address including relay info.
    ///
    /// Use this when you need to connect to this node from another peer.
    pub fn node_addr(&self) -> iroh::EndpointAddr {
        self.endpoint.addr()
    }

    /// Wait for the endpoint to be online (relay connected).
    ///
    /// This should be called before publishing session records to ensure
    /// the relay URL is available for peer discovery. For browser clients,
    /// relay is required - we wait indefinitely since there's no fallback.
    pub async fn wait_online(&self) {
        self.endpoint.online().await;
    }

    /// Wait for relay connection and return the relay URL.
    ///
    /// Waits indefinitely for relay - browser clients require relay URLs
    /// for peer discovery. Returns the relay URL once connected.
    pub async fn wait_for_relay(&self) -> String {
        self.endpoint.online().await;
        // After online(), relay_url should always be Some for browser clients
        self.relay_url()
            .expect("relay URL should be available after online()")
    }

    /// Watch for address changes (including relay URL changes).
    ///
    /// Returns a stream that yields the address on each change.
    /// Use this to detect relay URL changes and update session records.
    pub fn watch_addr(&self) -> n0_future::boxed::BoxStream<iroh::EndpointAddr> {
        use iroh::Watcher;
        Box::pin(self.endpoint.watch_addr().stream())
    }
}
