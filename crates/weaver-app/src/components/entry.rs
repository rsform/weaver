#![allow(non_snake_case)]

#[cfg(feature = "server")]
use crate::blobcache::BlobCache;
use crate::{
    components::avatar::{Avatar, AvatarImage},
    data::use_handle,
};

use crate::Route;
use dioxus::prelude::*;

const ENTRY_CSS: Asset = asset!("/assets/styling/entry.css");

#[allow(unused_imports)]
use jacquard::smol_str::ToSmolStr;
use jacquard::types::string::Datetime;
#[allow(unused_imports)]
use jacquard::{
    smol_str::SmolStr,
    types::{cid::Cid, string::AtIdentifier},
};
#[allow(unused_imports)]
use std::sync::Arc;
use weaver_api::sh_weaver::notebook::{BookEntryView, EntryView, entry};

#[component]
pub fn EntryPage(
    ident: ReadSignal<AtIdentifier<'static>>,
    book_title: ReadSignal<SmolStr>,
    title: ReadSignal<SmolStr>,
) -> Element {
    tracing::debug!(
        "EntryPage component rendering for ident: {:?}, book: {}, title: {}",
        ident(),
        book_title(),
        title()
    );
    rsx! {
        {std::iter::once(rsx! {Entry {ident, book_title, title}})}
    }
}

#[component]
pub fn Entry(
    ident: ReadSignal<AtIdentifier<'static>>,
    book_title: ReadSignal<SmolStr>,
    title: ReadSignal<SmolStr>,
) -> Element {
    tracing::debug!(
        "Entry component rendering for ident: {:?}, book: {}, title: {}",
        ident(),
        book_title(),
        title()
    );
    // Use feature-gated hook for SSR support
    let entry = crate::data::use_entry_data(ident, book_title, title);
    let fetcher = use_context::<crate::fetch::Fetcher>();
    tracing::debug!("Entry component got entry data");

    // Handle blob caching when entry data is available
    match &*entry.read() {
        Some((book_entry_view, entry_record)) => {
            if let Some(embeds) = &entry_record.embeds {
                if let Some(images) = &embeds.images {
                    // Register blob mappings with service worker (client-side only)
                    #[cfg(all(
                        target_family = "wasm",
                        target_os = "unknown",
                        not(feature = "fullstack-server")
                    ))]
                    {
                        let fetcher = fetcher.clone();
                        let images = images.clone();
                        spawn(async move {
                            let _ = crate::service_worker::register_entry_blobs(
                                &ident(),
                                book_title().as_str(),
                                &images,
                                &fetcher,
                            )
                            .await;
                        });
                    }
                }
            }
            rsx! { EntryPageView {
                book_entry_view: book_entry_view.clone(),
                entry_record: entry_record.clone(),
                ident: ident(),
                book_title: book_title()
            } }
        }
        _ => rsx! { p { "Loading..." } },
    }
}

/// Full entry page with metadata, content, and navigation
#[component]
fn EntryPageView(
    book_entry_view: ReadSignal<BookEntryView<'static>>,
    entry_record: ReadSignal<entry::Entry<'static>>,
    ident: ReadSignal<AtIdentifier<'static>>,
    book_title: ReadSignal<SmolStr>,
) -> Element {
    // Extract metadata
    let entry_view = &book_entry_view().entry;
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
            if let Some(ref prev) = book_entry_view().prev {
                div { class: "nav-gutter nav-prev",
                    NavButton {
                        direction: "prev",
                        entry: prev.entry.clone(),
                        ident: ident(),
                        book_title: book_title()
                    }
                }
            }

            // Main content area
            div { class: "entry-content-main notebook-content",
                // Metadata header
                EntryMetadata {
                    entry_view: entry_view.clone(),
                    created_at: entry_record().created_at.clone()
                }

                // Rendered markdown
                EntryMarkdown {
                    content: entry_record,
                    ident
                }
            }

            // Right gutter with next button
            if let Some(ref next) = book_entry_view().next {
                div { class: "nav-gutter nav-next",
                    NavButton {
                        direction: "next",
                        entry: next.entry.clone(),
                        ident: ident(),
                        book_title: book_title()
                    }
                }
            }
        }
    }
}

#[component]
pub fn EntryCard(
    entry: BookEntryView<'static>,
    book_title: SmolStr,
    author_count: usize,
    ident: AtIdentifier<'static>,
) -> Element {
    use crate::Route;
    use jacquard::from_data;
    use weaver_api::sh_weaver::notebook::entry::Entry;

    let entry_view = &entry.entry;
    let title = entry_view
        .title
        .as_ref()
        .map(|t| t.as_ref())
        .unwrap_or("Untitled");
    // Format date
    let formatted_date = entry_view
        .indexed_at
        .as_ref()
        .format("%B %d, %Y")
        .to_string();

    // Only show author if notebook has multiple authors
    let show_author = author_count > 1;
    let first_author = if show_author {
        entry_view.authors.first()
    } else {
        None
    };

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
                to: Route::EntryPage {
                    ident: ident,
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
                                use weaver_api::sh_weaver::actor::ProfileDataViewInner;

                                match &author.record.inner {
                                    ProfileDataViewInner::ProfileView(profile) => {
                                        let display_name = profile.display_name.as_ref().map(|n| n.as_ref()).unwrap_or("Unknown");
                                         let handle = profile.handle.clone();
                                        rsx! {
                                            if let Some(ref avatar_url) = profile.avatar {
                                                Avatar {
                                                    AvatarImage { src: avatar_url.as_ref() }
                                                }
                                            }
                                            span { class: "author-name", "{display_name}" }
                                            span { class: "meta-label", "@{handle}" }
                                        }
                                    }
                                    ProfileDataViewInner::ProfileViewDetailed(profile) => {
                                        let display_name = profile.display_name.as_ref().map(|n| n.as_ref()).unwrap_or("Unknown");
                                         let handle = profile.handle.clone();
                                        rsx! {
                                            if let Some(ref avatar_url) = profile.avatar {
                                                Avatar {
                                                    AvatarImage { src: avatar_url.as_ref() }
                                                }
                                            }
                                            span { class: "author-name", "{display_name}" }
                                            span { class: "meta-label", "@{handle}" }
                                        }
                                    }
                                    ProfileDataViewInner::TangledProfileView(profile) => {
                                        rsx! {
                                            span { class: "author-name", "@{profile.handle.as_ref()}" }
                                        }
                                    }
                                    _ => {
                                        rsx! {
                                            span { class: "author-name", "Unknown" }
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
fn EntryMetadata(entry_view: EntryView<'static>, created_at: Datetime) -> Element {
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
                                use weaver_api::sh_weaver::actor::ProfileDataViewInner;

                                match &author.record.inner {
                                    ProfileDataViewInner::ProfileView(profile) => {
                                        let display_name = profile.display_name.as_ref().map(|n| n.as_ref()).unwrap_or("Unknown");
                                        let handle = profile.handle.clone();

                                        rsx! {
                                            Link {
                                                to: Route::RepositoryIndex { ident: AtIdentifier::Handle(handle.clone()) },
                                                div { class: "entry-authors",
                                                    if let Some(ref avatar_url) = profile.avatar {
                                                        Avatar {
                                                            AvatarImage {
                                                                src: avatar_url.as_ref()
                                                            }
                                                        }
                                                    }
                                                    span { class: "author-name", "{display_name}" }
                                                    span { class: "meta-label", "@{handle}" }
                                                }
                                            }
                                        }
                                    }
                                    ProfileDataViewInner::ProfileViewDetailed(profile) => {
                                        let display_name = profile.display_name.as_ref().map(|n| n.as_ref()).unwrap_or("Unknown");
                                        let handle = profile.handle.clone();
                                        rsx! {
                                            Link {
                                                to: Route::RepositoryIndex { ident: AtIdentifier::Handle(handle.clone()) },
                                                div { class: "entry-authors",
                                                    if let Some(ref avatar_url) = profile.avatar {
                                                        Avatar {
                                                            AvatarImage {
                                                                src: avatar_url.as_ref()
                                                            }
                                                        }
                                                    }
                                                    span { class: "author-name", "{display_name}" }
                                                    span { class: "meta-label", "@{handle}" }
                                                }
                                            }
                                        }
                                    }
                                    ProfileDataViewInner::TangledProfileView(profile) => {
                                        rsx! {
                                            span { class: "author-name", "@{profile.handle.as_ref()}" }
                                        }
                                    }
                                    _ => {
                                        rsx! {
                                            span { class: "author-name", "Unknown" }
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
    entry: EntryView<'static>,
    ident: AtIdentifier<'static>,
    book_title: SmolStr,
) -> Element {
    let entry_title = entry
        .title
        .as_ref()
        .map(|t| t.as_ref())
        .unwrap_or("Untitled");
    let arrow = if direction == "prev" { "←" } else { "→" };

    rsx! {
        Link {
            to: Route::EntryPage {
                ident: ident.clone(),
                book_title: book_title.clone(),
                title: entry_title.to_string().into()
            },
            class: "nav-button nav-button-{direction}",
            div { class: "nav-arrow", "{arrow}" }
            div { class: "nav-title", "{entry_title}" }
        }
    }
}

#[derive(Props, Clone, PartialEq)]
pub struct EntryMarkdownProps {
    #[props(default)]
    id: Signal<String>,
    #[props(default = use_signal(||"entry".to_string()))]
    class: Signal<String>,
    content: ReadSignal<entry::Entry<'static>>,
    ident: ReadSignal<AtIdentifier<'static>>,
}

/// Render some text as markdown.
pub fn EntryMarkdown(props: EntryMarkdownProps) -> Element {
    let processed = crate::data::use_rendered_markdown(props.content, props.ident);

    match &*processed.read() {
        Some(html_buf) => rsx! {
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
    let content = use_signal(|| content);
    let ident = use_signal(|| ident);
    let processed = crate::data::use_rendered_markdown(content.into(), ident.into());

    match &*processed.read() {
        Some(html_buf) => rsx! {
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
