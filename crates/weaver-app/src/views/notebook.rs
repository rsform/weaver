use crate::{
    components::{EntryCard, NotebookCss},
    fetch, Route,
};
use dioxus::prelude::*;
use jacquard::{
    smol_str::{SmolStr, ToSmolStr},
    types::ident::AtIdentifier,
};

const ENTRY_CARD_CSS: Asset = asset!("/assets/styling/entry-card.css");

/// The Blog page component that will be rendered when the current route is `[Route::Blog]`
///
/// The component takes a `id` prop of type `i32` from the route enum. Whenever the id changes, the component function will be
/// re-run and the rendered HTML will be updated.
#[component]
pub fn Notebook(ident: AtIdentifier<'static>, book_title: SmolStr) -> Element {
    rsx! {
        NotebookCss { ident: ident.to_smolstr(), notebook: book_title }
        Outlet::<Route> {}
    }
}

#[component]
pub fn NotebookIndex(ident: AtIdentifier<'static>, book_title: SmolStr) -> Element {
    let fetcher = use_context::<fetch::CachedFetcher>();
    let book_title_clone = book_title.clone();

    let notebook_entries = use_resource(use_reactive!(|(ident, book_title)| {
        let fetcher = fetcher.clone();
        async move {
            fetcher.list_notebook_entries(ident, book_title).await.ok().flatten()
        }
    }));

    rsx! {
        document::Link { rel: "stylesheet", href: ENTRY_CARD_CSS }

        div { class: "entries-list",
            match &*notebook_entries.read_unchecked() {
                Some(Some(entries)) => rsx! {
                    for entry in entries {
                        EntryCard { entry: entry.clone(), book_title: book_title_clone.clone() }
                    }
                },
                Some(None) => rsx! { div { class: "error", "Notebook not found" } },
                None => rsx! { div { class: "loading", "Loading notebook..." } }
            }
        }
    }
}
