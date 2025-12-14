//! XRPC endpoint handlers for the appview.

use jacquard::CowStr;
use jacquard::IntoStatic;
use jacquard::cowstr::ToCowStr;
use jacquard::types::string::AtUri;
use smol_str::SmolStr;

use crate::server::AppState;

use self::actor::resolve_actor;
use self::repo::XrpcErrorResponse;

pub mod actor;
pub mod bsky;
pub mod collab;
pub mod edit;
pub mod identity;
pub mod notebook;
pub mod repo;

/// Resolved AT URI components with canonical DID-based URI.
pub struct ResolvedUri {
    /// The resolved DID (authority converted from handle if needed)
    pub did: SmolStr,
    /// The collection from the URI
    pub collection: SmolStr,
    /// The record key from the URI
    pub rkey: SmolStr,
    /// Canonical DID-based URI for database lookups
    pub canonical_uri: String,
}

/// Parse an AT URI and resolve its authority to a DID.
///
/// This handles the common case where a URI might use a handle as the authority
/// (e.g. `at://alice.bsky.social/...`) but the database stores URIs with DIDs
/// (e.g. `at://did:plc:abc123/...`).
pub async fn resolve_uri(
    state: &AppState,
    uri: &AtUri<'_>,
) -> Result<ResolvedUri, XrpcErrorResponse> {
    let authority = uri.authority();
    let collection = uri
        .collection()
        .ok_or_else(|| XrpcErrorResponse::invalid_request("URI must include collection"))?;
    let rkey = uri
        .rkey()
        .ok_or_else(|| XrpcErrorResponse::invalid_request("URI must include rkey"))?;

    // Resolve authority to DID (might be a handle)
    let did = resolve_actor(state, authority).await?;

    // Construct canonical DID-based URI for DB lookup
    let canonical_uri = format!("at://{}/{}/{}", did, collection, rkey.as_ref());

    Ok(ResolvedUri {
        did: SmolStr::new(&did),
        collection: SmolStr::new(collection.as_ref()),
        rkey: SmolStr::new(rkey.as_ref()),
        canonical_uri,
    })
}

/// Convert SmolStr to Option<CowStr> if non-empty
pub fn non_empty_str(s: &SmolStr) -> Option<CowStr<'static>> {
    if s.is_empty() {
        None
    } else {
        Some(s.to_cowstr().into_static())
    }
}
