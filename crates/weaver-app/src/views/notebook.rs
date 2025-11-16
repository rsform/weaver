use crate::{
    Route,
    components::{EntryCard, NotebookCover, NotebookCss},
    data,
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
pub fn Notebook(ident: ReadSignal<AtIdentifier<'static>>, book_title: SmolStr) -> Element {
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
    // Fetch full notebook metadata with SSR support
    let notebook_data = data::use_notebook(ident(), book_title()).ok();

    // Fetch entries with SSR support
    let entries_resource = data::use_notebook_entries(ident(), book_title()).ok();

    rsx! {
        document::Link { rel: "stylesheet", href: ENTRY_CARD_CSS }

        if let (Some(notebook_memo), Some(entries_memo)) = (&notebook_data, &entries_resource) {
            match (&*notebook_memo.read_unchecked(), &*entries_memo.read_unchecked()) {
                (Some(data), Some(entries)) => {
                    let (notebook_view, _) = data;
                    let author_count = notebook_view.authors.len();

                    rsx! {
                        div { class: "notebook-layout",
                            aside { class: "notebook-sidebar",
                                NotebookCover {
                                    notebook: notebook_view.clone(),
                                    title: book_title().to_string()
                                }
                            }

                            main { class: "notebook-main",
                                div { class: "entries-list",
                                    for entry in entries {
                                        EntryCard {
                                            entry: entry.clone(),
                                            book_title: book_title(),
                                            author_count
                                        }
                                    }
                                }
                            }
                        }
                    }
                },
                _ => rsx! { div { class: "loading", "Loading..." } }
            }
        } else {
            div { class: "loading", "Loading..." }
        }
    }
}
