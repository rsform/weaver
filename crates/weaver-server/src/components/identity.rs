use crate::{fetch, Route};
use dioxus::prelude::*;
use jacquard::{
    client::BasicClient,
    types::{ident::AtIdentifier, tid::Tid},
    CowStr,
};
use weaver_api::sh_weaver::notebook::NotebookView;
#[component]
pub fn Repository(ident: AtIdentifier<'static>) -> Element {
    rsx! {
        // We can create elements inside the rsx macro with the element name followed by a block of attributes and children.
        div {
            Outlet::<Route> {}
        }
    }
}

#[component]
pub fn RepositoryIndex(ident: AtIdentifier<'static>) -> Element {
    let fetcher = use_context::<fetch::CachedFetcher>();
    let notebooks = use_signal(|| fetcher.list_recent_notebooks());
    rsx! {
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

#[component]
pub fn NotebookCard(notebook: NotebookView<'static>) -> Element {
    rsx! {}
}
