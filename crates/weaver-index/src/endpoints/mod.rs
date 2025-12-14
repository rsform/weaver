//! XRPC endpoint handlers for the appview.

use jacquard::CowStr;
use jacquard::IntoStatic;
use jacquard::cowstr::ToCowStr;
use smol_str::SmolStr;

pub mod actor;
pub mod collab;
pub mod edit;
pub mod notebook;
pub mod repo;

/// Convert SmolStr to Option<CowStr> if non-empty
pub fn non_empty_str(s: &SmolStr) -> Option<CowStr<'static>> {
    if s.is_empty() {
        None
    } else {
        Some(s.to_cowstr().into_static())
    }
}
