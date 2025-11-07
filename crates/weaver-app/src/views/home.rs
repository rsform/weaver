use crate::{components::identity::NotebookCard, fetch};
use dioxus::prelude::*;

const NOTEBOOK_CARD_CSS: Asset = asset!("/assets/styling/notebook-card.css");

/// The Home page component that will be rendered when the current route is `[Route::Home]`
#[component]
pub fn Home() -> Element {
    let fetcher = use_context::<fetch::CachedFetcher>();
    let notebooks = use_signal(|| fetcher.list_recent_notebooks());
    rsx! {
        document::Link { rel: "stylesheet", href: NOTEBOOK_CARD_CSS }

        div { class: "notebooks-list",
            for notebook in notebooks.iter() {
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
        }
    }
}
