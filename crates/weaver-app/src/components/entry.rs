#![allow(non_snake_case)]

use crate::Route;
#[cfg(feature = "server")]
use crate::blobcache::BlobCache;
use crate::{
    components::EntryActions,
    components::avatar::{Avatar, AvatarImage},
    data::use_handle,
};
use dioxus::prelude::*;
use jacquard::IntoStatic;
use jacquard::types::aturi::AtUri;

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

// #[component]
// pub fn EntryPage(
//     ident: ReadSignal<AtIdentifier<'static>>,
//     book_title: ReadSignal<SmolStr>,
//     title: ReadSignal<SmolStr>,
// ) -> Element {
//     rsx! {
//         {std::iter::once(rsx! {Entry {ident, book_title, title}})}
//     }
// }

#[component]
pub fn EntryPage(
    ident: ReadSignal<AtIdentifier<'static>>,
    book_title: ReadSignal<SmolStr>,
    title: ReadSignal<SmolStr>,
) -> Element {
    // Use feature-gated hook for SSR support
    let (entry_res, entry) = crate::data::use_entry_data(ident, book_title, title);
    let route = use_route::<Route>();
    let mut last_route = use_signal(|| route.clone());

    #[cfg(all(
        target_family = "wasm",
        target_os = "unknown",
        not(feature = "fullstack-server")
    ))]
    let fetcher = use_context::<crate::fetch::Fetcher>();

    // Suspend SSR until entry loads
    #[cfg(feature = "fullstack-server")]
    let mut entry_res = entry_res?;

    #[cfg(feature = "fullstack-server")]
    use_effect(use_reactive!(|route| {
        if route != last_route() {
            tracing::debug!("[EntryPage] route changed, restarting resource");
            entry_res.restart();
            last_route.set(route.clone());
        }
    }));

    // Debug: log route params and entry state
    tracing::debug!(
        "[EntryPage] route params: ident={:?}, book_title={:?}, title={:?}",
        ident(),
        book_title(),
        title()
    );
    tracing::debug!("[EntryPage] rendering, entry.is_some={}", entry.read().is_some());

    // Handle blob caching when entry data is available
    // Use read() instead of read_unchecked() for proper reactive tracking
    match &*entry.read() {
        Some((book_entry_view, entry_record)) => {
            if let Some(embeds) = &entry_record.embeds {
                if let Some(_images) = &embeds.images {
                    // Register blob mappings with service worker (client-side only)
                    // #[cfg(all(
                    //     target_family = "wasm",
                    //     target_os = "unknown",
                    //     not(feature = "fullstack-server")
                    // ))]
                    // {
                    //     let fetcher = fetcher.clone();
                    //     let images = _images.clone().into_static();
                    //     spawn(async move {
                    //         let images = images.clone();
                    //         let fetcher = fetcher.clone();
                    //         let _ = crate::service_worker::register_entry_blobs(
                    //             &ident(),
                    //             book_title().as_str(),
                    //             &_images,
                    //             &fetcher,
                    //         )
                    //         .await;
                    //     });
                    // }
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

    tracing::info!("Entry: {book_title} - {title}");

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
                    created_at: entry_record().created_at.clone(),
                    entry_uri: entry_view.uri.clone().into_static(),
                    book_title: Some(book_title()),
                    ident: ident()
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
    use crate::auth::AuthState;
    use jacquard::from_data;
    use weaver_api::sh_weaver::notebook::entry::Entry;

    let mut hidden = use_signal(|| false);

    // If removed from notebook, hide this card
    if hidden() {
        return rsx! {};
    }

    let auth_state = use_context::<Signal<AuthState>>();

    let entry_view = &entry.entry;
    let title = entry_view
        .title
        .as_ref()
        .map(|t| t.as_ref())
        .unwrap_or("Untitled");

    // Get path from view for URL, fallback to title
    let entry_path = entry_view
        .path
        .as_ref()
        .map(|p| p.as_ref().to_string())
        .unwrap_or_else(|| title.to_string());

    // Parse entry record for content preview
    let parsed_entry = from_data::<Entry>(&entry_view.record).ok();

    // Format date
    let formatted_date = entry_view
        .indexed_at
        .as_ref()
        .format("%B %d, %Y")
        .to_string();

    // Check ownership
    let is_owner = {
        let current_did = auth_state.read().did.clone();
        match (&current_did, &ident) {
            (Some(did), AtIdentifier::Did(ident_did)) => *did == *ident_did,
            _ => false,
        }
    };

    let entry_uri = entry_view.uri.clone().into_static();

    // Only show author if notebook has multiple authors
    let show_author = author_count > 1;
    let first_author = if show_author {
        entry_view.authors.first()
    } else {
        None
    };

    // Render preview from entry content
    let preview_html = parsed_entry.as_ref().map(|entry| {
        let parser = markdown_weaver::Parser::new(&entry.content);
        let mut html_buf = String::new();
        markdown_weaver::html::push_html(&mut html_buf, parser);
        html_buf
    });

    rsx! {
        div { class: "entry-card",
            div { class: "entry-card-meta",
                div { class: "entry-card-header",
                    Link {
                        to: Route::EntryPage {
                            ident: ident.clone(),
                            book_title: book_title.clone(),
                            title: entry_path.clone().into()
                        },
                        class: "entry-card-title-link",
                        h3 { class: "entry-card-title", "{title}" }
                    }
                    div { class: "entry-card-date",
                        time { datetime: "{entry_view.indexed_at.as_str()}", "{formatted_date}" }
                    }
                    if is_owner {
                        EntryActions {
                            entry_uri,
                            entry_title: title.to_string(),
                            in_notebook: true,
                            notebook_title: Some(book_title.clone()),
                            on_removed: Some(EventHandler::new(move |_| hidden.set(true)))
                        }
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

/// Metadata header showing title, authors, date, tags
#[component]
fn EntryMetadata(
    entry_view: EntryView<'static>,
    created_at: Datetime,
    entry_uri: AtUri<'static>,
    book_title: Option<SmolStr>,
    ident: AtIdentifier<'static>,
) -> Element {
    let navigator = use_navigator();

    let title = entry_view
        .title
        .as_ref()
        .map(|t| t.as_ref())
        .unwrap_or("Untitled");

    let entry_title = title.to_string();

    // Navigate back to notebook when entry is removed
    let nav_book_title = book_title.clone();
    let nav_ident = ident.clone();
    let on_removed = move |_| {
        if let Some(ref title) = nav_book_title {
            navigator.push(Route::NotebookIndex {
                ident: nav_ident.clone(),
                book_title: title.clone(),
            });
        }
    };

    rsx! {
        header { class: "entry-metadata",
            div { class: "entry-header-row",
                h1 { class: "entry-title", "{title}" }
                EntryActions {
                    entry_uri: entry_uri.clone(),
                    entry_title,
                    in_notebook: book_title.is_some(),
                    notebook_title: book_title.clone(),
                    on_removed: Some(EventHandler::new(on_removed))
                }
            }

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

    // Get path from view for URL, fallback to title
    let entry_path = entry
        .path
        .as_ref()
        .map(|p| p.as_ref().to_string())
        .unwrap_or_else(|| entry_title.to_string());

    let arrow = if direction == "prev" { "←" } else { "→" };

    rsx! {
        Link {
            to: Route::EntryPage {
                ident: ident.clone(),
                book_title: book_title.clone(),
                title: entry_path.into()
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
