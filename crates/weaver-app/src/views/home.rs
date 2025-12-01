use crate::{Route, components::identity::NotebookCard, data};
use dioxus::prelude::*;
use jacquard::types::aturi::AtUri;

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
