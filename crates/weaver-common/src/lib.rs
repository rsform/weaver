pub mod agent;
pub mod config;
pub mod error;
/// This filestore is very much not production ready
#[cfg(all(feature = "native", feature = "dev"))]
pub mod filestore;
pub mod lexicons;
pub mod oauth;
pub mod resolver;
pub mod xrpc_server;
use std::sync::OnceLock;

pub use atrium_api::types::*;
pub use lexicons::*;
use regex::Regex;
use string::Did;

pub use crate::error::{Error, IoError, ParseError, SerDeError};

pub use merde::CowStr;
/// too many cows, so we have conversions
pub fn mcow_to_cow(cow: CowStr<'_>) -> std::borrow::Cow<'_, str> {
    match cow {
        CowStr::Borrowed(s) => std::borrow::Cow::Borrowed(s),
        CowStr::Owned(s) => std::borrow::Cow::Owned(s.into_string()),
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
        markdown_weaver::CowStr::Boxed(s) => std::borrow::Cow::Owned(s.into_string()),
        markdown_weaver::CowStr::Inlined(s) => std::borrow::Cow::Owned(s.as_ref().to_owned()),
    }
}

pub fn avatar_cdn_url(did: &Did, blob_ref: &BlobRef) -> String {
    let (cid, mime_type) = match blob_ref {
        BlobRef::Typed(TypedBlobRef::Blob(b)) => (
            atrium_api::types::string::Cid::new(b.r#ref.0)
                .as_ref()
                .to_string(),
            &b.mime_type,
        ),
        BlobRef::Untyped(r) => (r.cid.clone(), &r.mime_type),
    };
    format!(
        "https://cdn.bsky.app/img/avatar/plain/{}/{}@{}",
        did.as_str(),
        cid,
        mime_type.strip_prefix("image/").unwrap_or(mime_type)
    )
}

pub fn blob_url(did: &Did, pds: &str, blob_ref: &BlobRef) -> String {
    let cid = match blob_ref {
        BlobRef::Typed(TypedBlobRef::Blob(b)) => atrium_api::types::string::Cid::new(b.r#ref.0)
            .as_ref()
            .to_string(),

        BlobRef::Untyped(r) => r.cid.clone(),
    };
    format!(
        "https://{}/xrpc/com.atproto.repo.getBlob?did={}&cid={}",
        pds,
        did.as_str(),
        cid,
    )
}

pub fn match_identifier(maybe_identifier: &str) -> Option<&str> {
    static RE_HANDLE: OnceLock<Regex> = OnceLock::new();
    static RE_DID: OnceLock<Regex> = OnceLock::new();
    if maybe_identifier.len() > 253 {
         None
    } else if !RE_DID.get_or_init(|| Regex::new(r"^did:[a-z]+:[a-zA-Z0-9._:%-]*[a-zA-Z0-9._-]$").unwrap())
        .is_match(&maybe_identifier) && !RE_HANDLE
        .get_or_init(|| Regex::new(r"^([a-zA-Z0-9]([a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?\.)+[a-zA-Z]([a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?$").unwrap())
        .is_match(&maybe_identifier)
    {
        None
    } else {
        Some(maybe_identifier)
    }
}

pub fn match_nsid(maybe_nsid: &str) -> Option<&str> {
    static RE_NSID: OnceLock<Regex> = OnceLock::new();
    if maybe_nsid.len() > 317 {
        None
    } else if !RE_NSID
        .get_or_init(|| Regex::new(r"^[a-zA-Z]([a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?(\.[a-zA-Z0-9]([a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?)+(\.[a-zA-Z]([a-zA-Z0-9]{0,62}[a-zA-Z0-9])?)$").unwrap())
        .is_match(&maybe_nsid)
    {
        None
    } else {
        Some(maybe_nsid)
    }
}

/// Convert an ATURI to a HTTP URL
/// Currently has some failure modes and should restrict the NSIDs to a known subset
pub fn aturi_to_http<'s>(aturi: &'s str, appview: &'s str) -> Option<markdown_weaver::CowStr<'s>> {
    use markdown_weaver::CowStr;

    if aturi.starts_with("at://") {
        let rest = aturi.strip_prefix("at:://").unwrap();
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
