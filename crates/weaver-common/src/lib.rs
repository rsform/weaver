pub mod agent;
pub mod config;
pub mod error;
pub mod lexicons;
pub mod oauth;
pub mod resolver;
pub mod xrpc_server;
use atrium_api::types::{BlobRef, TypedBlobRef, string::Did};
pub use lexicons::*;

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
