use crate::{components::NotebookCss, fetch, Route};
use dioxus::prelude::*;
use jacquard::{
    smol_str::{SmolStr, ToSmolStr},
    types::ident::AtIdentifier,
};

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
    rsx! {}
}
