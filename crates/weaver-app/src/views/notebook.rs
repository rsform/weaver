use crate::{
    components::{EntryCard, NotebookCover, NotebookCss},
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

    // Fetch full notebook to get author count
    let ident_for_notebook = ident.clone();
    let book_title_for_notebook = book_title.clone();
    let data_fetcher = fetcher.clone();
    let notebook_data = use_resource(use_reactive!(|(
        ident_for_notebook,
        book_title_for_notebook,
    )| {
        let fetcher = data_fetcher.clone();
        async move {
            fetcher
                .get_notebook(ident_for_notebook, book_title_for_notebook)
                .await
                .ok()
                .flatten()
        }
    }));

    // Also fetch entries
    let entry_fetcher = fetcher.clone();
    let entries_resource = use_resource(use_reactive!(|(ident, book_title)| {
        let fetcher = entry_fetcher.clone();
        async move {
            fetcher
                .list_notebook_entries(ident, book_title)
                .await
                .ok()
                .flatten()
        }
    }));

    rsx! {
        document::Link { rel: "stylesheet", href: ENTRY_CARD_CSS }

        match (&*notebook_data.read_unchecked(), &*entries_resource.read_unchecked()) {
            (Some(Some(data)), Some(Some(entries))) => {
                let (notebook_view, _) = data.as_ref();
                let author_count = notebook_view.authors.len();

                rsx! {
                    div { class: "notebook-layout",
                        aside { class: "notebook-sidebar",
                            NotebookCover {
                                notebook: notebook_view.clone(),
                                title: book_title_clone.to_string()
                            }
                        }

                        main { class: "notebook-main",
                            div { class: "entries-list",
                                for entry in entries {
                                    EntryCard {
                                        entry: entry.clone(),
                                        book_title: book_title_clone.clone(),
                                        author_count
                                    }
                                }
                            }
                        }
                    }
                }
            },
            (Some(None), _) | (_, Some(None)) => rsx! { div { class: "error", "Notebook or entries not found" } },
            _ => rsx! { div { class: "loading", "Loading..." } }
        }
    }
}
