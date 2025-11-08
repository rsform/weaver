#![allow(non_snake_case)]

#[cfg(feature = "server")]
use crate::blobcache::BlobCache;
use crate::{
    components::avatar::{Avatar, AvatarFallback, AvatarImage},
    data::use_handle,
    fetch,
};

use crate::Route;
use dioxus::prelude::*;

const ENTRY_CSS: Asset = asset!("/assets/styling/entry.css");

use jacquard::prelude::*;
#[allow(unused_imports)]
use jacquard::smol_str::ToSmolStr;
use jacquard::{from_data, types::string::Datetime};
#[allow(unused_imports)]
use jacquard::{
    smol_str::SmolStr,
    types::{cid::Cid, string::AtIdentifier},
};
#[allow(unused_imports)]
use std::sync::Arc;
use weaver_api::sh_weaver::notebook::{entry, BookEntryView};

#[component]
pub fn Entry(ident: AtIdentifier<'static>, book_title: SmolStr, title: SmolStr) -> Element {
    let ident_clone = ident.clone();
    let book_title_clone = book_title.clone();

    // Use feature-gated hook for SSR support
    let entry = crate::data::use_entry_data(ident.clone(), book_title.clone(), title.clone())?;

    // Handle blob caching when entry data is available
    use_effect(move || {
        if let Some((_book_entry_view, entry_record)) = &*entry.read() {
            if let Some(embeds) = &entry_record.embeds {
                if let Some(images) = &embeds.images {
                    // Register blob mappings with service worker (client-side only)
                    #[cfg(all(
                        target_family = "wasm",
                        target_os = "unknown",
                        not(feature = "fullstack-server")
                    ))]
                    {
                        let ident = ident.clone();
                        let book_title = book_title.clone();
                        let images = images.clone();
                        spawn(async move {
                            let fetcher = use_context::<fetch::CachedFetcher>();
                            let _ = crate::service_worker::register_entry_blobs(
                                &ident,
                                book_title.as_str(),
                                &images,
                                &fetcher,
                            )
                            .await;
                        });
                    }
                    #[cfg(feature = "fullstack-server")]
                    {
                        let ident = ident.clone();
                        let images = images.clone();
                        spawn(async move {
                            for image in &images.images {
                                use crate::data::cache_blob;

                                let cid = image.image.blob().cid();
                                cache_blob(
                                    ident.to_smolstr(),
                                    cid.to_smolstr(),
                                    image.name.as_ref().map(|n| n.to_smolstr()),
                                )
                                .await
                                .ok();
                            }
                        });
                    }
                }
            }
        }
    });

    match &*entry.read_unchecked() {
        Some((book_entry_view, entry_record)) => {
            rsx! { EntryPage {
                book_entry_view: book_entry_view.clone(),
                entry_record: entry_record.clone(),
                ident: use_handle(ident_clone)?(),
                book_title: book_title_clone
            } }
        }
        _ => rsx! { p { "Loading..." } },
    }
}

/// Full entry page with metadata, content, and navigation
#[component]
fn EntryPage(
    book_entry_view: BookEntryView<'static>,
    entry_record: entry::Entry<'static>,
    ident: AtIdentifier<'static>,
    book_title: SmolStr,
) -> Element {
    // Extract metadata
    let entry_view = &book_entry_view.entry;
    let title = entry_view
        .title
        .as_ref()
        .map(|t| t.as_ref())
        .unwrap_or("Untitled");

    rsx! {
        // Set page title
        document::Title { "{title}" }
        document::Link { rel: "stylesheet", href: ENTRY_CSS }

        div { class: "entry-page-layout",
            // Left gutter with prev button
            if let Some(ref prev) = book_entry_view.prev {
                div { class: "nav-gutter nav-prev",
                    NavButton {
                        direction: "prev",
                        entry: prev.entry.clone(),
                        ident: ident.clone(),
                        book_title: book_title.clone()
                    }
                }
            }

            // Main content area
            div { class: "entry-content-main",
                // Metadata header
                EntryMetadata {
                    entry_view: entry_view.clone(),
                    ident: ident.clone(),
                    created_at: entry_record.created_at.clone()
                }

                // Rendered markdown
                EntryMarkdownDirect {
                    content: entry_record,
                    ident: ident.clone()
                }
            }

            // Right gutter with next button
            if let Some(ref next) = book_entry_view.next {
                div { class: "nav-gutter nav-next",
                    NavButton {
                        direction: "next",
                        entry: next.entry.clone(),
                        ident: ident.clone(),
                        book_title: book_title.clone()
                    }
                }
            }
        }
    }
}

#[component]
pub fn EntryCard(entry: BookEntryView<'static>, book_title: SmolStr) -> Element {
    use crate::Route;
    use jacquard::{from_data, IntoStatic};
    use weaver_api::app_bsky::actor::profile::Profile;
    use weaver_api::sh_weaver::notebook::entry::Entry;

    let entry_view = &entry.entry;
    let title = entry_view
        .title
        .as_ref()
        .map(|t| t.as_ref())
        .unwrap_or("Untitled");

    let ident = use_handle(entry_view.uri.authority().clone().into_static())?;

    // Format date
    let formatted_date = entry_view
        .indexed_at
        .as_ref()
        .format("%B %d, %Y")
        .to_string();

    // Get first author for display
    let first_author = entry_view.authors.first();

    // Render preview from entry content
    let preview_html = from_data::<Entry>(&entry_view.record).ok().map(|entry| {
        let parser = markdown_weaver::Parser::new(&entry.content);
        let mut html_buf = String::new();
        markdown_weaver::html::push_html(&mut html_buf, parser);
        html_buf
    });

    rsx! {
        div { class: "entry-card",
            Link {
                to: Route::Entry {
                    ident: ident(),
                    book_title: book_title.clone(),
                    title: title.to_string().into()
                },
                class: "entry-card-link",



                div { class: "entry-card-meta",
                    div { class: "entry-card-header",

                        h3 { class: "entry-card-title", "{title}" }
                        div { class: "entry-card-date",
                            time { datetime: "{entry_view.indexed_at.as_str()}", "{formatted_date}" }
                        }
                    }
                    if let Some(author) = first_author {
                        div { class: "entry-card-author",
                            {
                                match author.record.get_at_path(".value").and_then(|v| from_data::<Profile>(v).ok()) {
                                    Some(profile) => {
                                        let avatar = profile.avatar
                                            .map(|avatar| {
                                                let cid = avatar.blob().cid();
                                                format!("https://cdn.bsky.app/img/avatar/plain/{}/{cid}@jpeg", entry_view.uri.authority().as_ref())
                                            });
                                        let display_name = profile.display_name
                                            .as_ref()
                                            .map(|n| n.as_ref())
                                            .unwrap_or("Unknown");
                                        rsx! {
                                            if let Some(avatar_url) = avatar {
                                                Avatar {
                                                    AvatarImage { src: avatar_url }
                                                }
                                            }
                                            span { class: "author-name", "{display_name}" }
                                            span { class: "meta-label", "@{ident}" }
                                        }
                                    }
                                    None => {
                                        rsx! {
                                            span { class: "author-name", "Author {author.index}" }
                                        }
                                    }
                                }
                            }
                        }
                    }


                }



                if let Some(ref html) = preview_html {
                    div { class: "entry-card-preview", dangerous_inner_html: "{html}" }
                }
                if let Some(ref tags) = entry_view.tags {
                    if !tags.is_empty() {
                        div { class: "entry-card-tags",
                            for tag in tags.iter() {
                                span { class: "entry-card-tag", "{tag}" }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Metadata header showing title, authors, date, tags
#[component]
fn EntryMetadata(
    entry_view: weaver_api::sh_weaver::notebook::EntryView<'static>,
    ident: AtIdentifier<'static>,
    created_at: Datetime,
) -> Element {
    use weaver_api::app_bsky::actor::profile::Profile;

    let title = entry_view
        .title
        .as_ref()
        .map(|t| t.as_ref())
        .unwrap_or("Untitled");

    //let indexed_at_chrono = entry_view.indexed_at.as_ref();

    rsx! {
        header { class: "entry-metadata",
            h1 { class: "entry-title", "{title}" }

            div { class: "entry-meta-info",
                // Authors
                if !entry_view.authors.is_empty() {
                    div { class: "entry-authors",
                        for (i, author) in entry_view.authors.iter().enumerate() {
                            if i > 0 { span { ", " } }
                            {
                                // Parse author profile from the nested value field
                                match author.record.get_at_path(".value").and_then(|v| from_data::<Profile>(v).ok()) {
                                    Some(profile) => {
                                        let avatar = profile.avatar
                                            .map(|avatar| {
                                                let cid = avatar.blob().cid();
                                                let did = entry_view.uri.authority();
                                                format!("https://cdn.bsky.app/img/avatar/plain/{did}/{cid}@jpeg")
                                            });
                                        let display_name = profile.display_name
                                            .as_ref()
                                            .map(|n| n.as_ref())
                                            .unwrap_or("Unknown");
                                        rsx! {
                                            Link {
                                                to: Route::RepositoryIndex { ident: ident.clone() },
                                                div { class: "entry-authors",
                                                    if let Some(avatar) = avatar {
                                                        Avatar {
                                                            AvatarImage {
                                                                src: avatar
                                                            }
                                                        }
                                                    }
                                                    span { class: "author-name", "{display_name}" }
                                                    span { class: "meta-label", "@{ident}" }
                                                }
                                            }
                                        }
                                    }
                                    None => {
                                        rsx! {
                                            span { class: "author-name", "Author {author.index}" }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                // Date
                div { class: "entry-date",
                    {
                        let formatted_date = created_at.as_ref().format("%B %d, %Y").to_string();

                        rsx! {
                            time { datetime: "{entry_view.indexed_at.as_str()}", "{formatted_date}" }

                        }
                    }
                }

                // Tags
                if let Some(ref tags) = entry_view.tags {
                    div { class: "entry-tags",
                        // TODO: Parse tags structure
                        span { class: "meta-label", "Tags:" }
                        for tag in tags.iter() {
                            span { class: "meta-label", "{tag}" }
                        }
                    }
                }
            }
        }
    }
}

/// Navigation button for prev/next entries
#[component]
fn NavButton(
    direction: &'static str,
    entry: weaver_api::sh_weaver::notebook::EntryView<'static>,
    ident: AtIdentifier<'static>,
    book_title: SmolStr,
) -> Element {
    let entry_title = entry
        .title
        .as_ref()
        .map(|t| t.as_ref())
        .unwrap_or("Untitled");

    let label = if direction == "prev" {
        "← Previous"
    } else {
        "Next →"
    };
    let arrow = if direction == "prev" { "←" } else { "→" };

    rsx! {
        Link {
            to: Route::Entry {
                ident: ident.clone(),
                book_title: book_title.clone(),
                title: entry_title.to_string().into()
            },
            class: "nav-button nav-button-{direction}",
            div { class: "nav-arrow", "{arrow}" }
            div { class: "nav-label", "{label}" }
            div { class: "nav-title", "{entry_title}" }
        }
    }
}

#[derive(Props, Clone, PartialEq)]
pub struct EntryMarkdownProps {
    #[props(default)]
    id: Signal<String>,
    #[props(default)]
    class: Signal<String>,
    content: ReadSignal<entry::Entry<'static>>,
    ident: ReadSignal<AtIdentifier<'static>>,
}

/// Render some text as markdown.
#[allow(unused)]
pub fn EntryMarkdown(props: EntryMarkdownProps) -> Element {
    let processed = crate::data::use_rendered_markdown(
        props.content.read().clone(),
        props.ident.read().clone(),
    )?;

    match &*processed.read_unchecked() {
        Some(Some(html_buf)) => rsx! {
            div {
                id: "{&*props.id.read()}",
                class: "{&*props.class.read()}",
                dangerous_inner_html: "{html_buf}"
            }
        },
        _ => rsx! {
            div {
                id: "{&*props.id.read()}",
                class: "{&*props.class.read()}",
                "Loading..."
            }
        },
    }
}

/// Render entry content directly without signals
#[component]
fn EntryMarkdownDirect(
    #[props(default)] id: String,
    #[props(default = "entry".to_string())] class: String,
    content: entry::Entry<'static>,
    ident: AtIdentifier<'static>,
) -> Element {
    // Use feature-gated hook for SSR support
    let processed = crate::data::use_rendered_markdown(content, ident)?;

    match &*processed.read_unchecked() {
        Some(Some(html_buf)) => rsx! {
            div {
                id: "{id}",
                class: "{class}",
                dangerous_inner_html: "{html_buf}"
            }
        },
        _ => rsx! {
            div {
                id: "{id}",
                class: "{class}",
                "Loading..."
            }
        },
    }
}
