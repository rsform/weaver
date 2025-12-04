use crate::{
    Route,
    auth::AuthState,
    components::{EntryCard, NotebookCover, NotebookCss},
    components::button::{Button, ButtonVariant},
    data,
};
use dioxus::prelude::*;
use jacquard::{
    smol_str::{SmolStr, ToSmolStr},
    types::ident::AtIdentifier,
};

/// OpenGraph and Twitter Card meta tags for notebook index pages
#[component]
pub fn NotebookOgMeta(
    title: String,
    description: String,
    image_url: String,
    canonical_url: String,
    author_handle: String,
    entry_count: usize,
) -> Element {
    let page_title = format!("{} | @{} | Weaver", title, author_handle);
    let full_description = if entry_count > 0 {
        format!("{} entries · {}", entry_count, description)
    } else {
        description.clone()
    };

    rsx! {
        document::Title { "{page_title}" }
        document::Meta { property: "og:title", content: "{title}" }
        document::Meta { property: "og:description", content: "{full_description}" }
        document::Meta { property: "og:image", content: "{image_url}" }
        document::Meta { property: "og:type", content: "website" }
        document::Meta { property: "og:url", content: "{canonical_url}" }
        document::Meta { property: "og:site_name", content: "Weaver" }
        document::Meta { name: "twitter:card", content: "summary_large_image" }
        document::Meta { name: "twitter:title", content: "{title}" }
        document::Meta { name: "twitter:description", content: "{full_description}" }
        document::Meta { name: "twitter:image", content: "{image_url}" }
        document::Meta { name: "twitter:creator", content: "@{author_handle}" }
    }
}

// Card styles loaded at navbar level
const LAYOUTS_CSS: Asset = asset!("/assets/styling/layouts.css");

/// The Blog page component that will be rendered when the current route is `[Route::Blog]`
///
/// The component takes a `id` prop of type `i32` from the route enum. Whenever the id changes, the component function will be
/// re-run and the rendered HTML will be updated.
#[component]
pub fn Notebook(ident: ReadSignal<AtIdentifier<'static>>, book_title: SmolStr) -> Element {
    tracing::debug!(
        "Notebook component rendering for ident: {:?}, book: {}",
        ident(),
        book_title
    );
    rsx! {
        NotebookCss { ident: ident.to_smolstr(), notebook: book_title }
        Outlet::<Route> {}
    }
}

#[component]
pub fn NotebookIndex(
    ident: ReadSignal<AtIdentifier<'static>>,
    book_title: ReadSignal<SmolStr>,
) -> Element {
    tracing::debug!(
        "NotebookIndex component rendering for ident: {:?}, book: {}",
        ident(),
        book_title()
    );
    // Fetch full notebook metadata with SSR support
    // IMPORTANT: Call ALL hooks before any ? early returns to maintain hook order
    let (notebook_result, notebook_data) = data::use_notebook(ident, book_title);
    let (entries_result, entries_resource) = data::use_notebook_entries(ident, book_title);
    tracing::debug!("NotebookIndex got notebook data and entries");

    #[cfg(feature = "fullstack-server")]
    notebook_result?;

    #[cfg(feature = "fullstack-server")]
    entries_result?;

    // Check ownership for "Add Entry" button
    let auth_state = use_context::<Signal<AuthState>>();
    let is_owner = {
        let current_did = auth_state.read().did.clone();
        match (&current_did, ident()) {
            (Some(did), AtIdentifier::Did(ref ident_did)) => *did == *ident_did,
            _ => false,
        }
    };

    rsx! {
        document::Link { rel: "stylesheet", href: LAYOUTS_CSS }

        match (&*notebook_data.read(), &*entries_resource.read()) {
            (Some(data), Some(entries)) => {
                let (notebook_view, _) = data;
                let author_count = notebook_view.authors.len();

                // Build OG metadata
                let og_title = notebook_view.title
                    .as_ref()
                    .map(|t| t.as_ref().to_string())
                    .unwrap_or_else(|| "Untitled Notebook".to_string());

                let og_author = {
                    use weaver_api::sh_weaver::actor::ProfileDataViewInner;
                    notebook_view.authors.first()
                        .map(|a| match &a.record.inner {
                            ProfileDataViewInner::ProfileView(p) => p.handle.as_ref().to_string(),
                            ProfileDataViewInner::ProfileViewDetailed(p) => p.handle.as_ref().to_string(),
                            ProfileDataViewInner::TangledProfileView(p) => p.handle.as_ref().to_string(),
                            _ => "unknown".to_string(),
                        })
                        .unwrap_or_else(|| "unknown".to_string())
                };

                // NotebookView doesn't expose description directly, use empty for now
                let og_description = String::new();

                let base = if crate::env::WEAVER_APP_ENV == "dev" {
                    format!("http://127.0.0.1:{}", crate::env::WEAVER_PORT)
                } else {
                    crate::env::WEAVER_APP_HOST.to_string()
                };
                let og_image_url = format!("{}/og/notebook/{}/{}.png", base, ident(), book_title());
                let canonical_url = format!("{}/{}/{}", base, ident(), book_title());

                rsx! {
                    NotebookOgMeta {
                        title: og_title,
                        description: og_description,
                        image_url: og_image_url,
                        canonical_url,
                        author_handle: og_author,
                        entry_count: entries.len(),
                    }
                    div { class: "notebook-layout",
                        aside { class: "notebook-sidebar",
                            NotebookCover {
                                notebook: notebook_view.clone(),
                                title: book_title().to_string(),
                                is_owner,
                                ident: Some(ident())
                            }
                        }

                        main { class: "notebook-main",
                            div { class: "entries-list",
                                for entry in entries {
                                    EntryCard {
                                        entry: entry.clone(),
                                        book_title: book_title(),
                                        author_count,
                                        ident: ident(),
                                    }
                                }
                            }
                        }
                    }
                }
            },
            _ => rsx! { div { class: "loading", "Loading..." } }
        }
    }
}
