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

const ENTRY_CARD_CSS: Asset = asset!("/assets/styling/entry-card.css");

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
        document::Link { rel: "stylesheet", href: ENTRY_CARD_CSS }

        match (&*notebook_data.read(), &*entries_resource.read()) {
            (Some(data), Some(entries)) => {
                let (notebook_view, _) = data;
                let author_count = notebook_view.authors.len();

                rsx! {
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
