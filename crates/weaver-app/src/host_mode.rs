//! Host mode context for subdomain and custom domain routing.

use crate::env::WEAVER_APP_HOST;
use jacquard::smol_str::{SmolStr, format_smolstr};
use jacquard::types::string::AtIdentifier;
use serde::{Deserialize, Serialize};

/// Context for subdomain routing - identifies the notebook being served.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound(deserialize = ""))]
pub struct SubdomainContext {
    /// DID of the notebook owner.
    #[serde(deserialize_with = "deserialize_static_ident")]
    pub owner: AtIdentifier<'static>,
    /// Notebook path (same as subdomain).
    pub notebook_path: SmolStr,

    /// Notebook title.
    pub notebook_title: SmolStr,
    /// Notebook rkey for direct lookups.
    pub notebook_rkey: SmolStr,
}

fn deserialize_static_ident<'de, D>(deserializer: D) -> Result<AtIdentifier<'static>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use jacquard::IntoStatic;
    let did: AtIdentifier<'de> = Deserialize::deserialize(deserializer)?;
    Ok(did.into_static())
}

impl SubdomainContext {
    /// Get the owner as an AtIdentifier for route parameters.
    pub fn owner_ident(&self) -> AtIdentifier<'static> {
        self.owner.clone()
    }
}

/// Link mode for generating appropriate URLs based on host context.
///
/// Components use this context to generate links that work on both
/// the main domain and subdomain hosting.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum LinkMode {
    /// Main domain - full paths with /:ident/:notebook/:entry
    #[default]
    MainDomain,
    /// Subdomain - simplified paths like /:entry
    Subdomain,
}

impl LinkMode {
    /// Check if we're in subdomain mode.
    pub fn is_subdomain(&self) -> bool {
        matches!(self, LinkMode::Subdomain)
    }

    /// Generate link to a notebook entry by title.
    pub fn entry_link(
        &self,
        ident: &AtIdentifier<'_>,
        book_title: &str,
        entry_title: &str,
    ) -> SmolStr {
        match self {
            LinkMode::MainDomain => format_smolstr!("/{}/{}/{}", ident, book_title, entry_title),
            LinkMode::Subdomain => format_smolstr!("/{}", entry_title),
        }
    }

    /// Generate link to a notebook entry by rkey.
    pub fn entry_rkey_link(
        &self,
        ident: &AtIdentifier<'_>,
        book_title: &str,
        rkey: &str,
    ) -> SmolStr {
        match self {
            LinkMode::MainDomain => format_smolstr!("/{}/{}/e/{}", ident, book_title, rkey),
            LinkMode::Subdomain => format_smolstr!("/e/{}", rkey),
        }
    }

    /// Generate link to edit a notebook entry by rkey.
    pub fn entry_edit_link(
        &self,
        ident: &AtIdentifier<'_>,
        book_title: &str,
        rkey: &str,
    ) -> SmolStr {
        match self {
            LinkMode::MainDomain => format_smolstr!("/{}/{}/e/{}/edit", ident, book_title, rkey),
            LinkMode::Subdomain => format_smolstr!("/e/{}/edit", rkey),
        }
    }

    /// Generate link to a notebook index.
    pub fn notebook_link(&self, ident: &AtIdentifier<'_>, book_title: &str) -> SmolStr {
        match self {
            LinkMode::MainDomain => format_smolstr!("/{}/{}", ident, book_title),
            LinkMode::Subdomain => SmolStr::new_static("/"),
        }
    }

    /// Generate link to a profile/repository.
    pub fn profile_link(&self, ident: &AtIdentifier<'_>) -> SmolStr {
        match self {
            LinkMode::MainDomain => format_smolstr!("/{}", ident),
            LinkMode::Subdomain => format_smolstr!("/u/{}", ident),
        }
    }

    /// Generate link to a standalone entry.
    pub fn standalone_entry_link(&self, ident: &AtIdentifier<'_>, rkey: &str) -> SmolStr {
        match self {
            LinkMode::MainDomain => format_smolstr!("/{}/e/{}", ident, rkey),
            // Standalone entries don't exist in subdomain mode - link to main domain
            LinkMode::Subdomain => format_smolstr!("{}/{}/e/{}", WEAVER_APP_HOST, ident, rkey),
        }
    }

    /// Generate link to edit a standalone entry.
    pub fn standalone_entry_edit_link(&self, ident: &AtIdentifier<'_>, rkey: &str) -> SmolStr {
        match self {
            LinkMode::MainDomain => format_smolstr!("/{}/e/{}/edit", ident, rkey),
            // Edit on main domain
            LinkMode::Subdomain => format_smolstr!("{}/{}/e/{}/edit", WEAVER_APP_HOST, ident, rkey),
        }
    }

    /// Generate link to create a new draft.
    pub fn new_draft_link(&self, ident: &AtIdentifier<'_>, notebook: Option<&str>) -> SmolStr {
        match (self, notebook) {
            (LinkMode::MainDomain, Some(nb)) => format_smolstr!("/{}/new?notebook={}", ident, nb),
            (LinkMode::MainDomain, None) => format_smolstr!("/{}/new", ident),
            // Drafts are on main domain
            (LinkMode::Subdomain, Some(nb)) => {
                format_smolstr!("{}/{}/new?notebook={}", WEAVER_APP_HOST, ident, nb)
            }
            (LinkMode::Subdomain, None) => format_smolstr!("{}/{}/new", WEAVER_APP_HOST, ident),
        }
    }

    /// Generate link to drafts list.
    pub fn drafts_link(&self, ident: &AtIdentifier<'_>) -> SmolStr {
        match self {
            LinkMode::MainDomain => format_smolstr!("/{}/drafts", ident),
            LinkMode::Subdomain => format_smolstr!("{}/{}/drafts", WEAVER_APP_HOST, ident),
        }
    }

    /// Generate link to invites page.
    pub fn invites_link(&self, ident: &AtIdentifier<'_>) -> SmolStr {
        match self {
            LinkMode::MainDomain => format_smolstr!("/{}/invites", ident),
            LinkMode::Subdomain => format_smolstr!("{}/{}/invites", WEAVER_APP_HOST, ident),
        }
    }
}
