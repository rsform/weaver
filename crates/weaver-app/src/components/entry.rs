#![allow(non_snake_case)]

use crate::Route;
#[cfg(feature = "server")]
use crate::blobcache::BlobCache;
use crate::components::AuthorList;
use crate::{components::EntryActions, data::use_handle};
use dioxus::prelude::*;
use jacquard::types::aturi::AtUri;
use jacquard::{IntoStatic, types::string::Handle};

pub const ENTRY_CSS: Asset = asset!("/assets/styling/entry.css");

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
    tracing::debug!(
        "[EntryPage] rendering, entry.is_some={}",
        entry.read().is_some()
    );

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

/// Extract a plain-text preview from markdown content (first ~160 chars)
pub fn extract_preview(content: &str, max_len: usize) -> String {
    // Simple extraction: skip markdown syntax, get plain text
    let plain: String = content
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            // Skip headings, images, links, code blocks
            !trimmed.starts_with('#')
                && !trimmed.starts_with('!')
                && !trimmed.starts_with("```")
                && !trimmed.is_empty()
        })
        .take(5)
        .collect::<Vec<_>>()
        .join(" ");

    // Clean up markdown inline syntax
    let cleaned = plain
        .replace("**", "")
        .replace("__", "")
        .replace('*', "")
        .replace('_', "")
        .replace('`', "");

    if cleaned.len() <= max_len {
        cleaned
    } else {
        // Use char boundary-safe truncation to avoid panic on multibyte chars
        let truncated: String = cleaned.chars().take(max_len - 3).collect();
        format!("{}...", truncated)
    }
}

/// Truncate markdown content for preview (preserves markdown syntax)
/// Takes first few paragraphs up to max_chars, truncating at paragraph boundary
fn truncate_markdown_preview(content: &str, max_chars: usize, max_paragraphs: usize) -> String {
    let mut result = String::new();
    let mut char_count = 0;
    let mut para_count = 0;
    let mut in_code_block = false;

    for line in content.lines() {
        // Track code blocks to avoid breaking them
        if line.trim().starts_with("```") {
            in_code_block = !in_code_block;
            // Skip code blocks in preview entirely
            if in_code_block {
                continue;
            }
        }

        if in_code_block {
            continue;
        }

        // Skip headings, images in preview
        let trimmed = line.trim();
        if trimmed.starts_with('#') || trimmed.starts_with('!') {
            continue;
        }

        // Empty line = paragraph boundary
        if trimmed.is_empty() {
            if !result.is_empty() && !result.ends_with("\n\n") {
                para_count += 1;
                if para_count >= max_paragraphs || char_count >= max_chars {
                    break;
                }
                result.push_str("\n\n");
            }
            continue;
        }

        // Check if adding this line would exceed limit
        if char_count + line.len() > max_chars && !result.is_empty() {
            break;
        }

        if !result.is_empty() && !result.ends_with('\n') {
            result.push('\n');
        }
        result.push_str(line);
        char_count += line.len();
    }

    result.trim().to_string()
}

/// OpenGraph and Twitter Card meta tags for entries
#[component]
pub fn EntryOgMeta(
    title: String,
    description: String,
    image_url: String,
    canonical_url: String,
    author_handle: String,
    #[props(default)] book_title: Option<String>,
) -> Element {
    let page_title = if let Some(ref book) = book_title {
        format!("{} | {} | Weaver", title, book)
    } else {
        format!("{} | Weaver", title)
    };

    rsx! {
        document::Title { "{page_title}" }
        document::Meta { property: "og:title", content: "{title}" }
        document::Meta { property: "og:description", content: "{description}" }
        document::Meta { property: "og:image", content: "{image_url}" }
        document::Meta { property: "og:type", content: "article" }
        document::Meta { property: "og:url", content: "{canonical_url}" }
        document::Meta { property: "og:site_name", content: "Weaver" }
        document::Meta { name: "twitter:card", content: "summary_large_image" }
        document::Meta { name: "twitter:title", content: "{title}" }
        document::Meta { name: "twitter:description", content: "{description}" }
        document::Meta { name: "twitter:image", content: "{image_url}" }
        document::Meta { name: "twitter:creator", content: "@{author_handle}" }
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

    // Get entry path for URLs
    let entry_path = entry_view
        .path
        .as_ref()
        .map(|p| p.as_ref().to_string())
        .unwrap_or_else(|| title.to_string());

    // Get author handle
    let author_handle = entry_view
        .authors
        .first()
        .map(|a| {
            use weaver_api::sh_weaver::actor::ProfileDataViewInner;
            match &a.record.inner {
                ProfileDataViewInner::ProfileView(p) => p.handle.as_ref().to_string(),
                ProfileDataViewInner::ProfileViewDetailed(p) => p.handle.as_ref().to_string(),
                ProfileDataViewInner::TangledProfileView(p) => p.handle.as_ref().to_string(),
                _ => "unknown".to_string(),
            }
        })
        .unwrap_or_else(|| "unknown".to_string());

    // Build OG URLs
    let base = if crate::env::WEAVER_APP_ENV == "dev" {
        format!("http://127.0.0.1:{}", crate::env::WEAVER_PORT)
    } else {
        crate::env::WEAVER_APP_HOST.to_string()
    };
    let canonical_url = format!("{}/{}/{}/{}", base, ident(), book_title(), entry_path);
    let og_image_url = format!(
        "{}/og/{}/{}/{}.png",
        base,
        ident(),
        book_title(),
        entry_path
    );

    // Extract description preview from content
    let description = extract_preview(entry_record().content.as_ref(), 160);

    tracing::info!("Entry: {book_title} - {title}");

    rsx! {
        EntryOgMeta {
            title: title.to_string(),
            description: description,
            image_url: og_image_url,
            canonical_url: canonical_url,
            author_handle: author_handle,
            book_title: Some(book_title().to_string()),
        }
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

    // Check edit access via permissions
    let can_edit = {
        let current_did = auth_state.read().did.clone();
        match &current_did {
            Some(did) => {
                if let Some(ref perms) = entry_view.permissions {
                    perms.editors.iter().any(|grant| grant.did == *did)
                } else {
                    // Fall back to ownership check
                    match &ident {
                        AtIdentifier::Did(ident_did) => *did == *ident_did,
                        _ => false,
                    }
                }
            }
            None => false,
        }
    };

    let entry_uri = entry_view.uri.clone().into_static();

    // Show author list if notebook has multiple authors
    let show_author = author_count > 1;

    // Render preview from truncated entry content
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
                    if can_edit {
                        EntryActions {
                            entry_uri,
                            entry_cid: entry_view.cid.clone().into_static(),
                            entry_title: title.to_string(),
                            in_notebook: true,
                            notebook_title: Some(book_title.clone()),
                            permissions: entry_view.permissions.clone(),
                            on_removed: Some(EventHandler::new(move |_| hidden.set(true)))
                        }
                    }
                }
                if show_author && !entry_view.authors.is_empty() {
                    AuthorList {
                        authors: entry_view.authors.clone(),
                        owner_ident: Some(ident.clone()),
                        class: Some("entry-card-author".to_string()),
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

/// Card for entries in a feed (e.g., home page)
/// Takes EntryView directly (not BookEntryView)
#[component]
pub fn FeedEntryCard(
    entry_view: EntryView<'static>,
    entry: entry::Entry<'static>,
    #[props(default = false)] show_actions: bool,
    #[props(default = false)] is_pinned: bool,
    #[props(default = true)] show_author: bool,
    /// Profile identity for context-aware author visibility (hides single author on their own profile)
    #[props(default)] profile_ident: Option<AtIdentifier<'static>>,
    #[props(default)] on_pinned_changed: Option<EventHandler<bool>>,
) -> Element {
    use crate::Route;
    use crate::auth::AuthState;

    let title = entry_view
        .title
        .as_ref()
        .map(|t| t.as_ref())
        .unwrap_or("Untitled");

    // Extract DID and rkey from the entry URI
    let uri = &entry_view.uri;
    let parsed_uri = jacquard::types::aturi::AtUri::new(uri.as_ref()).ok();

    let ident = parsed_uri
        .as_ref()
        .map(|u| u.authority().clone().into_static())
        .unwrap_or_else(|| AtIdentifier::Handle(Handle::new_static("invalid.handle").unwrap()));

    let rkey: SmolStr = parsed_uri
        .as_ref()
        .and_then(|u| u.rkey().map(|r| SmolStr::new(r.0.as_str())))
        .unwrap_or_default();

    // Format date from record's created_at
    let formatted_date = entry.created_at.as_ref().format("%B %d, %Y").to_string();

    // Whether to show authors
    let has_authors = show_author && !entry_view.authors.is_empty();

    // Check edit access via permissions
    let auth_state = use_context::<Signal<AuthState>>();
    let can_edit = {
        let current_did = auth_state.read().did.clone();
        match &current_did {
            Some(did) => {
                if let Some(ref perms) = entry_view.permissions {
                    perms.editors.iter().any(|grant| grant.did == *did)
                } else {
                    // Fall back to ownership check
                    match &ident {
                        AtIdentifier::Did(ident_did) => *did == *ident_did,
                        _ => false,
                    }
                }
            }
            None => false,
        }
    };

    // Render preview from truncated entry content
    let preview_html = {
        let parser = markdown_weaver::Parser::new(&entry.content);
        let mut html_buf = String::new();
        markdown_weaver::html::push_html(&mut html_buf, parser);
        html_buf
    };

    rsx! {
        div { class: "entry-card feed-entry-card",
            // Header: title (and date if no author)
            div { class: "entry-card-header",
                Link {
                    to: Route::StandaloneEntry {
                        ident: ident.clone(),
                        rkey: rkey.clone().into()
                    },
                    class: "entry-card-title-link",
                    h3 { class: "entry-card-title", "{title}" }
                }
                // Date inline with title when no author shown
                if !has_authors {
                    div { class: "entry-card-date",
                        time { datetime: "{entry.created_at.as_str()}", "{formatted_date}" }
                    }
                }
                if show_actions && can_edit {
                    crate::components::EntryActions {
                        entry_uri: entry_view.uri.clone().into_static(),
                        entry_cid: entry_view.cid.clone().into_static(),
                        entry_title: title.to_string(),
                        in_notebook: false,
                        is_pinned,
                        permissions: entry_view.permissions.clone(),
                        on_pinned_changed
                    }
                }
            }

            // Byline: author + date (only when authors shown)
            if has_authors {
                div { class: "entry-card-byline",
                    AuthorList {
                        authors: entry_view.authors.clone(),
                        profile_ident: profile_ident.clone(),
                        owner_ident: Some(ident.clone()),
                        class: Some("entry-card-author".to_string()),
                    }
                    div { class: "entry-card-date",
                        time { datetime: "{entry.created_at.as_str()}", "{formatted_date}" }
                    }
                }
            }

            div { class: "entry-card-preview", dangerous_inner_html: "{preview_html}" }
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
pub fn EntryMetadata(
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
                    entry_cid: entry_view.cid.clone().into_static(),
                    entry_title,
                    in_notebook: book_title.is_some(),
                    notebook_title: book_title.clone(),
                    permissions: entry_view.permissions.clone(),
                    on_removed: Some(EventHandler::new(on_removed))
                }
            }

            div { class: "entry-meta-info",
                // Authors
                if !entry_view.authors.is_empty() {
                    div { class: "entry-authors",
                        AuthorList {
                            authors: entry_view.authors.clone(),
                            owner_ident: Some(ident.clone()),
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
pub fn NavButton(
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
    let (_res, processed) = crate::data::use_rendered_markdown(props.content, props.ident);
    #[cfg(feature = "fullstack-server")]
    _res?;

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
    let (_res, processed) = crate::data::use_rendered_markdown(content.into(), ident.into());
    #[cfg(feature = "fullstack-server")]
    _res?;

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
