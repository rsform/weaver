use crate::{
    components::{FeedEntryCard, NotebookCard, css::DefaultNotebookCss},
    data,
};
use dioxus::prelude::*;
use jacquard::smol_str::{SmolStr, format_smolstr};
use jacquard::types::ident::AtIdentifier;
use jacquard::types::string::Did;

/// Pinned content items - can be notebooks or entries
#[derive(Clone, PartialEq)]
pub enum PinnedItem {
    Notebook {
        ident: AtIdentifier<'static>,
        title: SmolStr,
    },
    #[allow(dead_code)]
    Entry {
        ident: AtIdentifier<'static>,
        rkey: SmolStr,
    },
}

/// Hardcoded pinned items
fn pinned_items() -> Vec<PinnedItem> {
    vec![
        // Add pinned items here, e.g.:
        // PinnedItem::Notebook {
        //     ident: AtIdentifier::Did(Did::new_static("did:plc:yfvwmnlztr4dwkb7hwz55r2g").unwrap()),
        //     title: SmolStr::new_static("Weaver"),
        // },
        PinnedItem::Entry {
            ident: AtIdentifier::Did(Did::new_static("did:plc:yfvwmnlztr4dwkb7hwz55r2g").unwrap()),
            rkey: SmolStr::new_static("3m4rbphjzt62b"),
        },
    ]
}

/// OpenGraph and Twitter Card meta tags for the homepage
#[component]
pub fn SiteOgMeta() -> Element {
    let base = if crate::env::WEAVER_APP_ENV == "dev" {
        format_smolstr!("http://127.0.0.1:{}", crate::env::WEAVER_PORT)
    } else {
        SmolStr::new_static(crate::env::WEAVER_APP_HOST)
    };

    let title = "Weaver";
    let description = "Share your words, your way.";
    let image_url = format_smolstr!("{}/og/site.png", base);
    let canonical_url = base;

    rsx! {
        document::Title { "{title}" }
        document::Meta { property: "og:title", content: "{title}" }
        document::Meta { property: "og:description", content: "{description}" }
        document::Meta { property: "og:image", content: "{image_url}" }
        document::Meta { property: "og:type", content: "website" }
        document::Meta { property: "og:url", content: "{canonical_url}" }
        document::Meta { property: "og:site_name", content: "Weaver" }
        document::Meta { name: "twitter:card", content: "summary_large_image" }
        document::Meta { name: "twitter:title", content: "{title}" }
        document::Meta { name: "twitter:description", content: "{description}" }
        document::Meta { name: "twitter:image", content: "{image_url}" }
    }
}

// Card styles (entry-card, notebook-card) loaded at navbar level
const ENTRY_CSS: Asset = asset!("/assets/styling/entry.css");
const HOME_CSS: Asset = asset!("/assets/styling/home.css");

/// The Home page component that will be rendered when the current route is `[Route::Home]`
#[component]
pub fn Home() -> Element {
    // Fetch entries from UFOS with SSR support
    let (entries_result, entries) = data::use_entries_from_ufos();

    let pinned = pinned_items();
    let has_pinned = !pinned.is_empty();

    #[cfg(feature = "fullstack-server")]
    let _entries_res = entries_result?;

    rsx! {
        SiteOgMeta {}

        document::Link { rel: "stylesheet", href: HOME_CSS }
        document::Link { rel: "stylesheet", href: ENTRY_CSS }
        DefaultNotebookCss {  }
        div {
            class: "home-container",

            // Pinned section
            if has_pinned {
                section { class: "pinned-section",
                    h2 { class: "section-header", "Featured" }
                    div { class: "pinned-items",
                        for item in pinned.into_iter() {
                            PinnedItemCard { item }
                        }
                    }
                }
            }

            // Main feed
            section { class: "feed-section",
                h2 { class: "section-header", "Recent" }
                div { class: "entries-feed",
                    match &*entries.read() {
                        Some(entry_list) => rsx! {
                            for (entry_view, entry, _time_us) in entry_list.iter() {
                                div {
                                    key: "{entry_view.cid}",
                                    FeedEntryCard {
                                        entry_view: entry_view.clone(),
                                        entry: entry.clone()
                                    }
                                }
                            }
                        },
                        _ => rsx! {
                            div { class: "loading", "Loading entries..." }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn PinnedItemCard(item: PinnedItem) -> Element {
    match item {
        PinnedItem::Notebook { ident, title } => rsx! {
            PinnedNotebookCard { ident, title }
        },
        PinnedItem::Entry { ident, rkey } => rsx! {
            PinnedEntryCard { ident, rkey }
        },
    }
}

#[component]
fn PinnedNotebookCard(ident: AtIdentifier<'static>, title: SmolStr) -> Element {
    let ident_memo = use_memo(move || ident.clone());
    let title_memo = use_memo(move || title.clone());
    let (note_res, notebook) = data::use_notebook(ident_memo.into(), title_memo.into());

    #[cfg(feature = "fullstack-server")]
    let _note_res = note_res?;

    match &*notebook.read() {
        Some((view, entries)) => rsx! {
            NotebookCard {
                notebook: view.clone(),
                entry_refs: entries.clone(),
                show_author: Some(true)
            }
        },
        None => rsx! {
            div { class: "pinned-item-loading", "Loading notebook..." }
        },
    }
}

#[component]
fn PinnedEntryCard(ident: AtIdentifier<'static>, rkey: SmolStr) -> Element {
    let ident_memo = use_memo(move || ident.clone());
    let rkey_memo = use_memo(move || rkey.clone());
    let (entry_res, entry_data) =
        data::use_standalone_entry_data(ident_memo.into(), rkey_memo.into());

    #[cfg(feature = "fullstack-server")]
    let _entry_res = entry_res?;

    match &*entry_data.read() {
        Some(data) => rsx! {
            FeedEntryCard {
                entry_view: data.entry_view.clone(),
                entry: data.entry.clone()
            }
        },
        None => rsx! {
            div { class: "pinned-item-loading", "Loading entry..." }
        },
    }
}
