#![allow(non_snake_case)]

#[cfg(feature = "server")]
use crate::blobcache::BlobCache;
use crate::{
    components::avatar::{Avatar, AvatarFallback, AvatarImage},
    fetch,
};
use dioxus::prelude::*;

const ENTRY_CSS: Asset = asset!("/assets/styling/entry.css");
#[allow(unused_imports)]
use dioxus::{fullstack::extract::Extension, CapturedError};
use jacquard::{
    from_data, prelude::IdentityResolver, smol_str::ToSmolStr, types::string::Datetime,
};
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
    let entry = use_resource(use_reactive!(|(ident, book_title, title)| async move {
        let fetcher = use_context::<fetch::CachedFetcher>();
        let entry = fetcher
            .get_entry(ident.clone(), book_title, title)
            .await
            .ok()
            .flatten();
        if let Some(entry) = &entry {
            let entry = &entry.1;
            if let Some(embeds) = &entry.embeds {
                if let Some(images) = &embeds.images {
                    for image in &images.images {
                        let cid = image.image.blob().cid();
                        cache_blob(
                            ident.to_smolstr(),
                            cid.to_smolstr(),
                            image.name.as_ref().map(|n| n.to_smolstr()),
                        )
                        .await
                        .ok();
                    }
                }
            }
        }
        entry
    }));

    match &*entry.read_unchecked() {
        Some(Some(entry_data)) => {
            rsx! { EntryPage {
                book_entry_view: entry_data.0.clone(),
                entry_record: entry_data.1.clone(),
                ident: ident_clone,
                book_title: book_title_clone
            } }
        }
        Some(None) => {
            rsx! { div { class: "error", "Entry not found" } }
        }
        None => rsx! { p { "Loading..." } },
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
pub fn EntryCard(entry: BookEntryView<'static>) -> Element {
    rsx! {}
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

    let indexed_at_chrono = entry_view.indexed_at.as_ref();

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
                                match from_data::<Profile>(author.record.get_at_path(".value").unwrap()) {
                                    Ok(profile) => {
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
                                    Err(_) => {
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
                        span { class: "meta-label", "Tags: " }
                        span { "[tags]" }
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
    use crate::Route;

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
}

/// Render some text as markdown.
#[allow(unused)]
pub fn EntryMarkdown(props: EntryMarkdownProps) -> Element {
    let content = &*props.content.read();
    let parser = markdown_weaver::Parser::new(&content.content);

    let mut html_buf = String::new();
    markdown_weaver::html::push_html(&mut html_buf, parser);

    rsx! {
        div {
            id: "{&*props.id.read()}",
            class: "{&*props.class.read()}",
            dangerous_inner_html: "{html_buf}"
        }
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
    use n0_future::stream::StreamExt;
    use weaver_renderer::{
        atproto::{ClientContext, ClientWriter},
        ContextIterator, NotebookProcessor,
    };

    let processed = use_resource(use_reactive!(|(content, ident)| async move {
        // Create client context for link/image/embed handling
        let fetcher = use_context::<fetch::CachedFetcher>();
        let did = match ident {
            AtIdentifier::Did(d) => d,
            AtIdentifier::Handle(h) => fetcher.client.resolve_handle(&h).await.ok()?,
        };
        let ctx = ClientContext::<()>::new(content.clone(), did);
        let parser = markdown_weaver::Parser::new(&content.content);
        let iter = ContextIterator::default(parser);
        let processor = NotebookProcessor::new(ctx, iter);

        // Collect events from the processor stream
        let events: Vec<_> = StreamExt::collect(processor).await;

        // Render to HTML
        let mut html_buf = String::new();
        let _ = ClientWriter::<_, _, ()>::new(events.into_iter(), &mut html_buf).run();
        Some(html_buf)
    }));

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

#[put("/cache/{ident}/{cid}?name", cache: Extension<Arc<BlobCache>>)]
pub async fn cache_blob(ident: SmolStr, cid: SmolStr, name: Option<SmolStr>) -> Result<()> {
    let ident = AtIdentifier::new_owned(ident)?;
    let cid = Cid::new_owned(cid.as_bytes())?;
    cache.cache(ident, cid, name).await
}
