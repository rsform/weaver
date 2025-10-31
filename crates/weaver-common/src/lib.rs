//! Weaver common library - thin wrapper around jacquard with notebook-specific conveniences

pub mod error;

// Re-export jacquard for convenience
pub use jacquard;
use jacquard::error::ClientError;
use jacquard::types::ident::AtIdentifier;
use jacquard::{CowStr, IntoStatic, xrpc};

pub use error::WeaverError;
use jacquard::types::tid::{Ticker, Tid};

use jacquard::bytes::Bytes;
use jacquard::client::{Agent, AgentError, AgentErrorKind, AgentSession, AgentSessionExt};
use jacquard::prelude::*;
use jacquard::smol_str::SmolStr;
use jacquard::types::blob::{BlobRef, MimeType};
use jacquard::types::string::{AtUri, Cid, Did, Handle, RecordKey};
use jacquard::xrpc::Response;
use std::path::Path;
use std::sync::LazyLock;
use tokio::sync::Mutex;
use weaver_api::com_atproto::repo::get_record::GetRecordResponse;
use weaver_api::com_atproto::repo::strong_ref::StrongRef;
use weaver_api::sh_weaver::notebook::{book, chapter, entry};
use weaver_api::sh_weaver::publish::blob::Blob as PublishedBlob;

use crate::error::ParseError;

static W_TICKER: LazyLock<Mutex<Ticker>> = LazyLock::new(|| Mutex::new(Ticker::new()));

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
//#[trait_variant::make(Send)]
pub trait WeaverExt: AgentSessionExt {
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
    fn publish_notebook(
        &self,
        path: &Path,
    ) -> impl Future<Output = Result<PublishResult<'_>, WeaverError>>;

    /// Publish a blob to the user's PDS
    ///
    /// Multi-step workflow:
    /// 1. Upload blob to PDS
    /// 2. Create blob record with CID
    ///
    /// Returns the AT-URI of the published blob
    fn publish_blob<'a>(
        &self,
        blob: Bytes,
        url_path: &'a str,
        prev: Option<Tid>,
    ) -> impl Future<Output = Result<(StrongRef<'a>, PublishedBlob<'a>), WeaverError>>;

    fn confirm_record_ref(
        &self,
        uri: &AtUri<'_>,
    ) -> impl Future<Output = Result<StrongRef<'_>, WeaverError>>;
}

impl<A: AgentSession + IdentityResolver> WeaverExt for Agent<A> {
    async fn publish_notebook(&self, _path: &Path) -> Result<PublishResult<'_>, WeaverError> {
        // TODO: Implementation
        todo!("publish_notebook not yet implemented")
    }

    async fn publish_blob<'a>(
        &self,
        blob: Bytes,
        url_path: &'a str,
        prev: Option<Tid>,
    ) -> Result<(StrongRef<'a>, PublishedBlob<'a>), WeaverError> {
        let mime_type = MimeType::new_owned(tree_magic::from_u8(blob.as_ref()));

        let blob = self.upload_blob(blob, mime_type).await?;
        let publish_record = PublishedBlob::new()
            .path(url_path)
            .upload(BlobRef::Blob(blob))
            .build();
        let tid = W_TICKER.lock().await.next(prev);
        let record = self
            .create_record(publish_record.clone(), Some(RecordKey::any(tid.as_str())?))
            .await?;
        let strong_ref = StrongRef::new().uri(record.uri).cid(record.cid).build();

        Ok((strong_ref, publish_record))
    }

    async fn confirm_record_ref(&self, uri: &AtUri<'_>) -> Result<StrongRef<'_>, WeaverError> {
        let rkey = uri.rkey().ok_or_else(|| {
            AgentError::from(
                ClientError::invalid_request("AtUri missing rkey")
                    .with_help("ensure the URI includes a record key after the collection"),
            )
        })?;

        // Resolve authority (DID or handle) to get DID and PDS
        use jacquard::types::ident::AtIdentifier;
        let (repo_did, pds_url) = match uri.authority() {
            AtIdentifier::Did(did) => {
                let pds = self.pds_for_did(did).await.map_err(|e| {
                    AgentError::from(
                        ClientError::from(e)
                            .with_context("DID document resolution failed during record retrieval"),
                    )
                })?;
                (did.clone(), pds)
            }
            AtIdentifier::Handle(handle) => self.pds_for_handle(handle).await.map_err(|e| {
                AgentError::from(
                    ClientError::from(e)
                        .with_context("handle resolution failed during record retrieval"),
                )
            })?,
        };

        // Make stateless XRPC call to that PDS (no auth required for public records)
        use weaver_api::com_atproto::repo::get_record::GetRecord;
        let request = GetRecord::new()
            .repo(AtIdentifier::Did(repo_did))
            .collection(
                uri.collection()
                    .expect("collection should exist if rkey does")
                    .clone(),
            )
            .rkey(rkey.clone())
            .build();

        let response: Response<GetRecordResponse> = {
            let http_request = xrpc::build_http_request(&pds_url, &request, &self.opts().await)
                .map_err(|e| AgentError::from(ClientError::transport(e)))?;

            let http_response = self
                .send_http(http_request)
                .await
                .map_err(|e| AgentError::from(ClientError::transport(e)))?;

            xrpc::process_response(http_response)
        }
        .map_err(|e| AgentError::new(AgentErrorKind::Client, Some(e.into())))?;
        let record = response.parse().map_err(|e| AgentError::xrpc(e))?;
        let strong_ref = StrongRef::new()
            .uri(record.uri)
            .cid(record.cid.expect("when does this NOT have a CID?"))
            .build();
        Ok(strong_ref.into_static())
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
    pub entries: Vec<StrongRef<'a>>,
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

pub enum LinkUri<'a> {
    AtRecord(AtUri<'a>),
    AtIdent(Did<'a>, Handle<'a>),
    Web(jacquard::url::Url),
    Path(markdown_weaver::CowStr<'a>),
    Heading(markdown_weaver::CowStr<'a>),
    Footnote(markdown_weaver::CowStr<'a>),
}

impl<'a> LinkUri<'a> {
    pub async fn resolve<A>(dest_url: &'a str, agent: &Agent<A>) -> LinkUri<'a>
    where
        A: AgentSession + IdentityResolver,
    {
        if dest_url.starts_with('@') {
            if let Ok(handle) = Handle::new(dest_url) {
                if let Ok(did) = agent.resolve_handle(&handle).await {
                    return Self::AtIdent(did, handle);
                }
            }
        } else if dest_url.starts_with("did:") {
            if let Ok(did) = Did::new(dest_url) {
                if let Ok(doc) = agent.resolve_did_doc(&did).await {
                    if let Ok(doc) = doc.parse_validated() {
                        if let Some(handle) = doc.handles().first() {
                            return Self::AtIdent(did, handle.clone());
                        }
                    }
                }
            }
        } else if dest_url.starts_with('#') {
            // local fragment
            return Self::Heading(markdown_weaver::CowStr::Borrowed(dest_url));
        } else if dest_url.starts_with('^') {
            // footnote
            return Self::Footnote(markdown_weaver::CowStr::Borrowed(dest_url));
        }
        if let Ok(url) = jacquard::url::Url::parse(dest_url) {
            if let Some(uri) = jacquard::richtext::extract_at_uri_from_url(
                url.as_str(),
                jacquard::richtext::DEFAULT_EMBED_DOMAINS,
            ) {
                if let AtIdentifier::Handle(handle) = uri.authority() {
                    if let Ok(did) = agent.resolve_handle(handle).await {
                        let mut aturi = format!("at://{did}");
                        if let Some(collection) = uri.collection() {
                            aturi.push_str(&format!("/{}", collection));
                            if let Some(record) = uri.rkey() {
                                aturi.push_str(&format!("/{}", record.0));
                            }
                        }
                        if let Ok(aturi) = AtUri::new_owned(aturi) {
                            return Self::AtRecord(aturi);
                        }
                    }
                    return Self::AtRecord(uri);
                } else {
                    return Self::AtRecord(uri);
                }
            } else if url.scheme() == "http" || url.scheme() == "https" {
                return Self::Web(url);
            }
        }

        LinkUri::Path(markdown_weaver::CowStr::Borrowed(dest_url))
    }
}
