use crate::{Route, components::identity::NotebookCard, data};
use dioxus::prelude::*;
use jacquard::types::aturi::AtUri;

/// OpenGraph and Twitter Card meta tags for the homepage
#[component]
pub fn SiteOgMeta() -> Element {
    let base = if crate::env::WEAVER_APP_ENV == "dev" {
        format!("http://127.0.0.1:{}", crate::env::WEAVER_PORT)
    } else {
        crate::env::WEAVER_APP_HOST.to_string()
    };

    let title = "Weaver";
    let description = "Share your words, your way.";
    let image_url = format!("{}/og/site.png", base);
    let canonical_url = base;

    rsx! {
        document::Title { "{title}" }
        document::Meta { property: "og:title", content: "{title}" }
        document::Meta { property: "og:description", content: "{description}" }
        document::Meta { property: "og:image", content: "{image_url}" }
        document::Meta { property: "og:type", content: "website" }
        document::Meta { property: "og:url", content: "{canonical_url}" }
        document::Meta { property: "og:site_name", content: "Weaver" }
        document::Meta { name: "twitter:card", content: "summary_large_image" }
        document::Meta { name: "twitter:title", content: "{title}" }
        document::Meta { name: "twitter:description", content: "{description}" }
        document::Meta { name: "twitter:image", content: "{image_url}" }
    }
}

const NOTEBOOK_CARD_CSS: Asset = asset!("/assets/styling/notebook-card.css");

/// The Home page component that will be rendered when the current route is `[Route::Home]`
#[component]
pub fn Home() -> Element {
    // Fetch notebooks from UFOS with SSR support
    let (notebooks_result, notebooks) = data::use_notebooks_from_ufos();

    #[cfg(feature = "fullstack-server")]
    notebooks_result
        .as_ref()
        .ok()
        .map(|r| r.suspend())
        .transpose()?;

    rsx! {
        SiteOgMeta {}
        document::Link { rel: "stylesheet", href: NOTEBOOK_CARD_CSS }
        div {
            class: "record-view-container",

            div { class: "notebooks-list",
                match &*notebooks.read() {
                    Some(notebook_list) => rsx! {
                        for notebook in notebook_list.iter() {
                            {
                                let view = &notebook.0;
                                let entries = &notebook.1;
                                rsx! {
                                    div {
                                        key: "{view.cid}",
                                        NotebookCard {
                                            notebook: view.clone(),
                                            entry_refs: entries.clone()
                                        }
                                    }
                                }
                            }
                        }
                    },
                    _ => rsx! {
                        div { "Loading notebooks..." }
                    }
                }
            }
        }

    }
}
