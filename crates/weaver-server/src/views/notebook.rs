use crate::{fetch, Route};
use dioxus::prelude::*;
use jacquard::{
    client::BasicClient,
    smol_str::SmolStr,
    types::{ident::AtIdentifier, tid::Tid},
    CowStr,
};
use std::sync::Arc;

const BLOG_CSS: Asset = asset!("/assets/styling/blog.css");

/// The Blog page component that will be rendered when the current route is `[Route::Blog]`
///
/// The component takes a `id` prop of type `i32` from the route enum. Whenever the id changes, the component function will be
/// re-run and the rendered HTML will be updated.
#[component]
pub fn Notebook(ident: AtIdentifier<'static>, book_title: SmolStr) -> Element {
    let fetcher = use_context::<fetch::CachedFetcher>();
    rsx! {
        document::Link { rel: "stylesheet", href: BLOG_CSS }
        Outlet::<Route> {}
    }
}

#[component]
pub fn NotebookIndex(ident: AtIdentifier<'static>, book_title: SmolStr) -> Element {
    let fetcher = use_context::<fetch::CachedFetcher>();
    rsx! {}
}
