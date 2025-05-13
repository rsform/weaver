pub use merde::CowStr;

pub mod error;

pub use crate::error::{Error, IoError, ParseError, SerDeError};

/// too many cows, so we have conversions
pub fn mcow_to_cow<'a>(cow: CowStr<'a>) -> std::borrow::Cow<'a, str> {
    match cow {
        CowStr::Borrowed(s) => std::borrow::Cow::Borrowed(s),
        CowStr::Owned(s) => std::borrow::Cow::Owned(s.into_string()),
    }
}

/// too many cows, so we have conversions
pub fn cow_to_mcow<'a>(cow: std::borrow::Cow<'a, str>) -> CowStr<'a> {
    match cow {
        std::borrow::Cow::Borrowed(s) => CowStr::Borrowed(s),
        std::borrow::Cow::Owned(s) => CowStr::Owned(s.into()),
    }
}

/// too many cows, so we have conversions
pub fn mdcow_to_cow<'a>(cow: markdown_weaver::CowStr<'a>) -> std::borrow::Cow<'a, str> {
    match cow {
        markdown_weaver::CowStr::Borrowed(s) => std::borrow::Cow::Borrowed(s),
        markdown_weaver::CowStr::Boxed(s) => std::borrow::Cow::Owned(s.into_string()),
        markdown_weaver::CowStr::Inlined(s) => std::borrow::Cow::Owned(s.as_ref().to_owned()),
    }
}
