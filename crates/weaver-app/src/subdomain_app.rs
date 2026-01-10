//! Subdomain Dioxus application.
//!
//! Separate router for subdomain hosting with simpler route structure.

use dioxus::prelude::*;
use jacquard::oauth::client::OAuthClient;
use jacquard::oauth::session::ClientData;
use jacquard::smol_str::{SmolStr, ToSmolStr};
use jacquard::types::string::AtIdentifier;

use crate::auth;
use crate::auth::{AuthState, AuthStore};
use crate::components::identity::RepositoryIndex;
use crate::components::{EntryPage, NotebookCss};
use crate::host_mode::{LinkMode, SubdomainContext};
use crate::views::{NotebookEntryByRkey, NotebookEntryEdit, NotebookIndex, SubdomainNavbar};
use crate::{CONFIG, fetch};

/// Subdomain route enum - simpler paths without /:ident/:notebook prefix.
#[derive(Debug, Clone, Routable, PartialEq)]
#[rustfmt::skip]
pub enum SubdomainRoute {
    #[layout(SubdomainNavbar)]
        /// Landing page - custom entry or notebook index.
        #[route("/")]
        SubdomainLanding {},
        /// Explicit notebook index.
        #[route("/index")]
        SubdomainIndexPage {},
        /// Entry by title.
        #[route("/:title")]
        SubdomainEntry { title: SmolStr },
        /// Entry by rkey.
        #[route("/e/:rkey")]
        SubdomainEntryByRkey { rkey: SmolStr },
        /// Entry edit by rkey.
        #[route("/e/:rkey/edit")]
        SubdomainEntryEdit { rkey: SmolStr },
        /// Profile/repository view.
        #[route("/u/:ident")]
        SubdomainProfile { ident: AtIdentifier<'static> },
}

/// Look up notebook by global path and build SubdomainContext.
pub async fn lookup_subdomain_context(
    fetcher: &crate::fetch::Fetcher,
    path: &str,
) -> Option<SubdomainContext> {
    use jacquard::IntoStatic;
    use jacquard::smol_str::SmolStr;
    use jacquard::xrpc::XrpcClient;
    use weaver_api::sh_weaver::notebook::resolve_global_notebook::ResolveGlobalNotebook;

    let request = ResolveGlobalNotebook::new().path(path).build();

    match fetcher.send(request).await {
        Ok(response) => {
            let output = response.into_output().ok()?;
            let notebook = output.notebook;

            let owner = notebook.uri.authority().clone().into_static();
            let rkey = notebook.uri.rkey()?.0.to_smolstr();
            let notebook_path = notebook
                .path
                .map(|p| SmolStr::new(p.as_ref()))
                .unwrap_or_else(|| SmolStr::new(path));

            Some(SubdomainContext {
                owner,
                notebook_path,
                notebook_rkey: rkey,
                notebook_title: notebook.title.clone().unwrap_or_default().to_smolstr(),
            })
        }
        Err(e) => {
            tracing::debug!(path = path, error = %e, "Global notebook lookup failed");
            None
        }
    }
}

/// Extract subdomain from host if it matches base domain pattern.
pub fn extract_subdomain(host: &str, base: &str) -> Option<String> {
    let suffix = format!(".{}", base);
    if host.ends_with(&suffix) && host.len() > suffix.len() {
        Some(host[..host.len() - suffix.len()].to_string())
    } else {
        None
    }
}

const ENTRY_CSS: Asset = asset!("/assets/styling/entry.css");
const LAYOUTS_CSS: Asset = asset!("/assets/styling/layouts.css");

/// Root component for subdomain app.
#[component]
pub fn SubdomainApp() -> Element {
    rsx! {
        document::Link { rel: "icon", href: crate::FAVICON }
        document::Link { rel: "preconnect", href: "https://fonts.googleapis.com" }
        document::Link { rel: "preconnect", href: "https://fonts.gstatic.com" }
        document::Link { rel: "stylesheet", href: crate::THEME_DEFAULTS_CSS }
        document::Link { rel: "stylesheet", href: "https://fonts.googleapis.com/css2?family=IBM+Plex+Mono:ital,wght@0,200;0,300;0,400;0,500;0,600;0,700;1,200;1,300;1,400;1,500;1,600;1,700&family=IBM+Plex+Sans:ital,wght@0,100..700;1,100..700&family=IBM+Plex+Serif:ital,wght@0,200;0,300;0,400;0,500;0,600;0,700;1,200;1,300;1,400;1,500;1,600;1,700&display=swap" }
        document::Link { rel: "stylesheet", href: crate::MAIN_CSS }
        document::Link { rel: "stylesheet", href: LAYOUTS_CSS }
        document::Link { rel: "stylesheet", href: ENTRY_CSS }
        crate::components::toast::ToastProvider {
            Router::<SubdomainRoute> {}
        }
    }
}

/// Landing page - check for custom "/" entry, else show notebook index.
#[component]
fn SubdomainLanding() -> Element {
    let ctx = use_context::<SubdomainContext>();

    // TODO: Check for entry with custom path "/" for this notebook.
    // For now, just render the notebook index.
    rsx! {

        NotebookCss { ident: ctx.owner_ident().to_smolstr(), notebook:  ctx.notebook_path.clone() }
        NotebookIndex {
            ident: ctx.owner_ident(),
            book_title: ctx.notebook_title.clone(),
        }
    }
}

/// Explicit notebook index route.
#[component]
fn SubdomainIndexPage() -> Element {
    let ctx = use_context::<SubdomainContext>();

    rsx! {

        NotebookCss { ident: ctx.owner_ident().to_smolstr(), notebook:  ctx.notebook_path.clone() }
        NotebookIndex {
            ident: ctx.owner_ident(),
            book_title: ctx.notebook_title.clone(),
        }
    }
}

/// Entry by title.
#[component]
fn SubdomainEntry(title: SmolStr) -> Element {
    let ctx = use_context::<SubdomainContext>();

    rsx! {

        NotebookCss { ident: ctx.owner_ident().to_smolstr(), notebook:  ctx.notebook_path.clone() }
        EntryPage {
            ident: ctx.owner_ident(),
            book_title: ctx.notebook_title.clone(),
            title: title,
        }
    }
}

/// Entry by rkey.
#[component]
fn SubdomainEntryByRkey(rkey: SmolStr) -> Element {
    let ctx = use_context::<SubdomainContext>();

    rsx! {

        NotebookCss { ident: ctx.owner_ident().to_smolstr(), notebook:  ctx.notebook_path.clone() }
        NotebookEntryByRkey {
            ident: ctx.owner_ident(),
            book_title: ctx.notebook_title.clone(),
            rkey: rkey,
        }
    }
}

/// Entry edit by rkey.
#[component]
fn SubdomainEntryEdit(rkey: SmolStr) -> Element {
    let ctx = use_context::<SubdomainContext>();

    rsx! {

        NotebookCss { ident: ctx.owner_ident().to_smolstr(), notebook:  ctx.notebook_path.clone() }
        NotebookEntryEdit {
            ident: ctx.owner_ident(),
            book_title: ctx.notebook_title.clone(),
            rkey: rkey,
        }
    }
}

/// Profile/repository view for an identity.
#[component]
fn SubdomainProfile(ident: AtIdentifier<'static>) -> Element {
    rsx! {
        RepositoryIndex { ident }
    }
}
