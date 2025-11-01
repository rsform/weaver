use crate::{components::EntryCard, fetch};
use dioxus::prelude::*;

/// The Home page component that will be rendered when the current route is `[Route::Home]`
#[component]
pub fn Home() -> Element {
    let fetcher = use_context::<fetch::CachedFetcher>();
    let entries = use_signal(|| fetcher.list_recent_entries());
    rsx! {
        for entry in entries.iter() {
            {
                let view = &entry.0;
                rsx! {
                    div {
                        key: "{view.entry.cid}",
                        EntryCard { entry: view.clone() }
                    }
                }
            }
        }
    }
}
