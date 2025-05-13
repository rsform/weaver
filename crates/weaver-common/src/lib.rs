pub use merde::CowStr;

pub mod error;
pub mod lexicons;
pub use lexicons::*;

pub use crate::error::{Error, IoError, ParseError, SerDeError};

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
