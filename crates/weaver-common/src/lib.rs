//! Weaver common library - thin wrapper around jacquard with notebook-specific conveniences

pub mod error;

// Re-export jacquard for convenience
pub use jacquard;
use jacquard::CowStr;
pub use jacquard_api;

pub use error::WeaverError;

use jacquard::client::{Agent, AgentSession};
use jacquard::types::blob::BlobRef;
use jacquard::types::string::{AtUri, Cid, Did};
use jacquard_api::sh_weaver::notebook::{book, chapter, entry};
use std::path::Path;

/// Extension trait providing weaver-specific multi-step operations on Agent
///
/// This trait extends jacquard's Agent with notebook-specific workflows that
/// involve multiple atproto operations (uploading blobs, creating records, etc.)
///
/// For single-step operations, use jacquard's built-in methods directly:
/// - `agent.create_record()` - Create a single record
/// - `agent.get_record()` - Get a single record
/// - `agent.upload_blob()` - Upload a single blob
///
/// This trait is for multi-step workflows that coordinate between multiple operations.
#[trait_variant::make(Send)]
pub trait WeaverExt {
    /// Publish a notebook directory to the user's PDS
    ///
    /// Multi-step workflow:
    /// 1. Parse markdown files in directory
    /// 2. Extract and upload images/assets → BlobRefs
    /// 3. Transform markdown refs to point at uploaded blobs
    /// 4. Create entry records for each file
    /// 5. Create book record with entry refs
    ///
    /// Returns the AT-URI of the published book
    async fn publish_notebook(&self, path: &Path) -> Result<PublishResult<'_>, WeaverError>;

    /// Upload assets from markdown content
    ///
    /// Multi-step workflow:
    /// 1. Parse markdown for image/asset refs
    /// 2. Upload each asset → BlobRef
    /// 3. Return mapping of original path → BlobRef
    ///
    /// Used by renderer to transform local refs to atproto refs
    async fn upload_assets(
        &self,
        markdown: &str,
    ) -> Result<Vec<(String, BlobRef<'_>)>, WeaverError>;
}

impl<A: AgentSession> WeaverExt for Agent<A> {
    async fn publish_notebook(&self, _path: &Path) -> Result<PublishResult<'_>, WeaverError> {
        // TODO: Implementation
        todo!("publish_notebook not yet implemented")
    }

    async fn upload_assets(
        &self,
        _markdown: &str,
    ) -> Result<Vec<(String, BlobRef<'_>)>, WeaverError> {
        // TODO: Implementation
        todo!("upload_assets not yet implemented")
    }
}

/// Result of publishing a notebook
#[derive(Debug, Clone)]
pub struct PublishResult<'a> {
    /// AT-URI of the published book
    pub uri: AtUri<'a>,
    /// CID of the book record
    pub cid: Cid<'a>,
    /// URIs of published entries
    pub entries: Vec<AtUri<'a>>,
}

/// too many cows, so we have conversions
pub fn mcow_to_cow(cow: CowStr<'_>) -> std::borrow::Cow<'_, str> {
    match cow {
        CowStr::Borrowed(s) => std::borrow::Cow::Borrowed(s),
        CowStr::Owned(s) => std::borrow::Cow::Owned(s.to_string()),
    }
}

/// too many cows, so we have conversions
pub fn cow_to_mcow(cow: std::borrow::Cow<'_, str>) -> CowStr<'_> {
    match cow {
        std::borrow::Cow::Borrowed(s) => CowStr::Borrowed(s),
        std::borrow::Cow::Owned(s) => CowStr::Owned(s.into()),
    }
}

/// too many cows, so we have conversions
pub fn mdcow_to_cow(cow: markdown_weaver::CowStr<'_>) -> std::borrow::Cow<'_, str> {
    match cow {
        markdown_weaver::CowStr::Borrowed(s) => std::borrow::Cow::Borrowed(s),
        markdown_weaver::CowStr::Boxed(s) => std::borrow::Cow::Owned(s.to_string()),
        markdown_weaver::CowStr::Inlined(s) => std::borrow::Cow::Owned(s.as_ref().to_owned()),
    }
}

/// Utility: Generate CDN URL for avatar blob
pub fn avatar_cdn_url(did: &Did, cid: &Cid) -> String {
    format!(
        "https://cdn.bsky.app/img/avatar/plain/{}/{}",
        did.as_str(),
        cid
    )
}

/// Utility: Generate PDS URL for blob retrieval
pub fn blob_url(did: &Did, pds: &str, cid: &Cid) -> String {
    format!(
        "https://{}/xrpc/com.atproto.repo.getBlob?did={}&cid={}",
        pds,
        did.as_str(),
        cid
    )
}

pub fn match_identifier(maybe_identifier: &str) -> Option<&str> {
    if jacquard::types::string::AtIdentifier::new(maybe_identifier).is_ok() {
        Some(maybe_identifier)
    } else {
        None
    }
}

pub fn match_nsid(maybe_nsid: &str) -> Option<&str> {
    if jacquard::types::string::Nsid::new(maybe_nsid).is_ok() {
        Some(maybe_nsid)
    } else {
        None
    }
}

/// Convert an ATURI to a HTTP URL
/// Currently has some failure modes and should restrict the NSIDs to a known subset
pub fn aturi_to_http<'s>(aturi: &'s str, appview: &'s str) -> Option<markdown_weaver::CowStr<'s>> {
    use markdown_weaver::CowStr;

    if aturi.starts_with("at://") {
        let rest = aturi.strip_prefix("at://").unwrap();
        let mut split = rest.splitn(2, '/');
        let maybe_identifier = split.next()?;
        let maybe_nsid = split.next()?;
        let maybe_rkey = split.next()?;

        // https://atproto.com/specs/handle#handle-identifier-syntax
        let identifier = match_identifier(maybe_identifier)?;

        let nsid = if let Some(nsid) = match_nsid(maybe_nsid) {
            // Last part of the nsid is generally the middle component of the URL
            // TODO: check for bsky ones specifically, because those are the ones where this is valid
            nsid.rsplitn(1, '.').next()?
        } else {
            return None;
        };
        Some(CowStr::Boxed(
            format!(
                "https://{}/profile/{}/{}/{}",
                appview, identifier, nsid, maybe_rkey
            )
            .into_boxed_str(),
        ))
    } else {
        Some(CowStr::Borrowed(aturi))
    }
}
