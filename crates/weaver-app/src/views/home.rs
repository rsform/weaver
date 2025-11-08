use crate::{components::identity::NotebookCard, fetch};
use dioxus::prelude::*;

const NOTEBOOK_CARD_CSS: Asset = asset!("/assets/styling/notebook-card.css");

/// The Home page component that will be rendered when the current route is `[Route::Home]`
#[component]
pub fn Home() -> Element {
    let fetcher = use_context::<fetch::CachedFetcher>();

    // Fetch notebooks from UFOS
    let notebooks = use_resource(move || {
        let fetcher = fetcher.clone();
        async move { fetcher.fetch_notebooks_from_ufos().await }
    });

    rsx! {
        document::Link { rel: "stylesheet", href: NOTEBOOK_CARD_CSS }

        div { class: "notebooks-list",
            match notebooks() {
                Some(Ok(notebook_list)) => rsx! {
                    for notebook in notebook_list.iter() {
                        {
                            let view = &notebook.0;
                            rsx! {
                                div {
                                    key: "{view.cid}",
                                    NotebookCard { notebook: view.clone() }
                                }
                            }
                        }
                    }
                },
                Some(Err(_)) => rsx! {
                    div { "Error loading notebooks" }
                },
                None => rsx! {
                    div { "Loading notebooks..." }
                }
            }
        }
    }
}
