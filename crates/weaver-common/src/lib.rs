//! Weaver common library - thin wrapper around jacquard with notebook-specific conveniences

pub mod agent;
pub mod constellation;
pub mod error;
pub mod worker_rt;

// Re-export jacquard for convenience
pub use agent::WeaverExt;
pub use error::WeaverError;
pub use jacquard;
use jacquard::CowStr;
use jacquard::client::{Agent, AgentSession};
use jacquard::prelude::*;
use jacquard::types::ident::AtIdentifier;
use jacquard::types::string::{AtUri, Cid, Did, Handle};
use jacquard::types::tid::Ticker;
use std::sync::LazyLock;
use tokio::sync::Mutex;
use weaver_api::com_atproto::repo::strong_ref::StrongRef;

static W_TICKER: LazyLock<Mutex<Ticker>> = LazyLock::new(|| Mutex::new(Ticker::new()));

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

pub fn mcow_to_cow(cow: CowStr<'_>) -> std::borrow::Cow<'_, str> {
    match cow {
        CowStr::Borrowed(s) => std::borrow::Cow::Borrowed(s),
        CowStr::Owned(s) => std::borrow::Cow::Owned(s.to_string()),
    }
}

pub fn cow_to_mcow(cow: std::borrow::Cow<'_, str>) -> CowStr<'_> {
    match cow {
        std::borrow::Cow::Borrowed(s) => CowStr::Borrowed(s),
        std::borrow::Cow::Owned(s) => CowStr::Owned(s.into()),
    }
}

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

pub fn normalize_title_path(title: &str) -> String {
    title.replace(' ', "_").to_lowercase()
}
