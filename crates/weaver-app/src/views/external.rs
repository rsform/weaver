#![allow(non_snake_case)]

use dioxus::prelude::*;
use jacquard::smol_str::{SmolStr, format_smolstr};
use jacquard::types::string::AtIdentifier;
use weaver_api::sh_weaver::notebook::AuthorListView;

use crate::components::css::DefaultNotebookCss;
use crate::components::{AuthorList, extract_author_info};
use crate::fetch::Fetcher;

#[component]
pub fn WhiteWindEntry(
    ident: ReadSignal<AtIdentifier<'static>>,
    rkey: ReadSignal<SmolStr>,
) -> Element {
    use crate::components::{ENTRY_CSS, EntryOgMeta, calculate_reading_stats, extract_preview};

    let (entry_res, entry_data) = crate::data::use_whitewind_entry_data(ident, rkey);

    #[cfg(feature = "fullstack-server")]
    let _entry_res = entry_res?;

    match &*entry_data.read() {
        Some(data) => {
            let title = data
                .entry
                .title
                .as_ref()
                .map(|t| t.as_ref())
                .unwrap_or("Untitled");

            let subtitle = data.entry.subtitle.as_ref().map(|s| s.as_ref().to_string());

            let base = if crate::env::WEAVER_APP_ENV == "dev" {
                format_smolstr!("http://127.0.0.1:{}", crate::env::WEAVER_PORT)
            } else {
                SmolStr::new_static(crate::env::WEAVER_APP_HOST)
            };
            let canonical_url = format_smolstr!("{}/{}/w/{}", base, ident(), rkey());

            let author_info = extract_author_info(&data.profile.inner);
            let author_handle = author_info
                .as_ref()
                .map(|a| a.handle.as_ref().into())
                .unwrap_or_else(|| SmolStr::new_static("unknown"));

            let description = extract_preview(&data.entry.content, 160);
            let content = data.entry.content.clone();
            let (word_count, reading_time_mins) = calculate_reading_stats(&content);

            let author_list_view = AuthorListView::new()
                .index(0)
                .record(data.profile.clone())
                .build();

            let formatted_date = data
                .entry
                .created_at
                .as_ref()
                .map(|d| d.as_ref().format("%B %d, %Y").to_string());

            rsx! {
                EntryOgMeta {
                    title: title.to_string(),
                    description: description.clone(),
                    image_url: String::new(),
                    canonical_url: canonical_url.to_string(),
                    author_handle: author_handle.to_string(),
                }
                document::Link { rel: "stylesheet", href: ENTRY_CSS }
                DefaultNotebookCss {}

                div { class: "entry-page-layout",
                    div { class: "entry-content-main notebook-content",
                        header { class: "entry-metadata",
                            div { class: "entry-header-row",
                                h1 { class: "entry-title", "{title}" }
                            }
                            if let Some(ref sub) = subtitle {
                                p { class: "entry-subtitle", "{sub}" }
                            }
                            div { class: "entry-meta-info",
                                div { class: "entry-authors",
                                    AuthorList { authors: vec![author_list_view] }
                                }
                                if let Some(ref date) = formatted_date {
                                    div { class: "entry-date",
                                        time { "{date}" }
                                    }
                                }
                                div { class: "entry-meta-secondary",
                                    div { class: "entry-reading-stats",
                                        span { class: "word-count", "{word_count} words" }
                                        span { class: "reading-time", "{reading_time_mins} min read" }
                                    }
                                }
                            }
                            div { class: "entry-source",
                                a {
                                    href: "https://whtwnd.com/{author_handle}/{rkey()}",
                                    target: "_blank",
                                    class: "source-badge",
                                    "View on WhiteWind ↗"
                                }
                            }
                        }
                        WhiteWindMarkdown { content: content.to_string() }
                    }
                }
            }
        }
        None => rsx! { p { "Loading..." } },
    }
}

#[component]
fn WhiteWindMarkdown(content: String) -> Element {
    use markdown_weaver::Parser;
    use weaver_renderer::atproto::ClientWriter;

    let html = {
        let parser =
            Parser::new_ext(&content, weaver_renderer::default_md_options()).into_offset_iter();
        let mut html_buf = String::new();
        let _ = ClientWriter::<_, _, ()>::new(parser, &mut html_buf, &content).run();
        html_buf
    };

    rsx! {
        div {
            class: "entry",
            dangerous_inner_html: "{html}"
        }
    }
}

#[component]
pub fn LeafletEntry(
    ident: ReadSignal<AtIdentifier<'static>>,
    rkey: ReadSignal<SmolStr>,
) -> Element {
    use crate::components::{ENTRY_CSS, EntryOgMeta};
    use weaver_api::pub_leaflet::document::DocumentPagesItem;

    let (entry_res, entry_data) = crate::data::use_leaflet_document_data(ident, rkey);

    #[cfg(feature = "fullstack-server")]
    let _entry_res = entry_res?;

    match &*entry_data.read() {
        Some(data) => {
            let title = data.document.title.as_ref();

            let base = if crate::env::WEAVER_APP_ENV == "dev" {
                format_smolstr!("http://127.0.0.1:{}", crate::env::WEAVER_PORT)
            } else {
                SmolStr::new_static(crate::env::WEAVER_APP_HOST)
            };
            let canonical_url = format_smolstr!("{}/{}/l/{}", base, ident(), rkey());

            let author_info = extract_author_info(&data.profile.inner);
            let author_handle = author_info
                .as_ref()
                .map(|a| a.handle.as_ref().into())
                .unwrap_or_else(|| SmolStr::new_static("unknown"));

            let author_list_view = AuthorListView::new()
                .index(0)
                .record(data.profile.clone())
                .build();

            let pages = data.document.pages.clone();
            let author_did = data.document.author.clone();

            rsx! {
                EntryOgMeta {
                    title: title.to_string(),
                    description: String::new(),
                    image_url: String::new(),
                    canonical_url: canonical_url.to_string(),
                    author_handle: author_handle.to_string(),
                }
                document::Link { rel: "stylesheet", href: ENTRY_CSS }
                DefaultNotebookCss {}

                div { class: "entry-page-layout",
                    div { class: "entry-content-main notebook-content",
                        header { class: "entry-metadata",
                            div { class: "entry-header-row",
                                h1 { class: "entry-title", "{title}" }
                            }
                            div { class: "entry-meta-info",
                                div { class: "entry-authors",
                                    AuthorList { authors: vec![author_list_view] }
                                }
                            }
                            if let Some(ref base_path) = data.publication_base_path {
                                div { class: "entry-source",
                                    a {
                                        href: "https://{base_path}/{rkey()}",
                                        target: "_blank",
                                        class: "source-badge",
                                        "View on Leaflet ↗"
                                    }
                                }
                            }
                        }
                        LeafletContent {
                            pages: pages,
                            author_did: author_did,
                        }
                    }
                }
            }
        }
        None => rsx! { p { "Loading..." } },
    }
}

#[component]
fn LeafletContent(
    pages: Vec<weaver_api::pub_leaflet::document::DocumentPagesItem<'static>>,
    author_did: jacquard::types::string::AtIdentifier<'static>,
) -> Element {
    use jacquard::IntoStatic;
    use jacquard::prelude::*;
    use weaver_api::pub_leaflet::document::DocumentPagesItem;
    use weaver_renderer::leaflet::{LeafletRenderContext, render_linear_document};

    let fetcher = use_context::<Fetcher>();

    let html = use_resource(move || {
        let pages = pages.clone();
        let author_did = author_did.clone();
        let fetcher = fetcher.clone();
        async move {
            let mut html = String::new();

            // Resolve author DID
            let did = match &author_did {
                jacquard::types::string::AtIdentifier::Did(d) => d.clone().into_static(),
                jacquard::types::string::AtIdentifier::Handle(h) => {
                    match fetcher.resolve_handle(h).await {
                        Ok(d) => d.into_static(),
                        Err(_) => return String::from("<p>Failed to resolve author</p>"),
                    }
                }
            };

            let ctx = LeafletRenderContext::new(did);

            for page in &pages {
                match page {
                    DocumentPagesItem::LinearDocument(linear_doc) => {
                        html.push_str(&render_linear_document(linear_doc, &ctx, &fetcher).await);
                    }
                    DocumentPagesItem::Canvas(_) => {
                        html.push_str("<div class=\"embed-video-placeholder\">[Canvas layout not yet supported]</div>");
                    }
                    DocumentPagesItem::Unknown(_) => {
                        html.push_str(
                            "<div class=\"embed-video-placeholder\">[Unknown page type]</div>",
                        );
                    }
                }
            }

            html
        }
    });

    match &*html.read() {
        Some(content) => rsx! {
            div {
                class: "entry leaflet-document",
                dangerous_inner_html: "{content}"
            }
        },
        None => rsx! { p { "Rendering..." } },
    }
}

#[cfg(feature = "pckt")]
#[component]
pub fn PcktEntry(ident: ReadSignal<AtIdentifier<'static>>, rkey: ReadSignal<SmolStr>) -> Element {
    use crate::components::{ENTRY_CSS, EntryOgMeta};

    let (entry_res, entry_data) = crate::data::use_pckt_document_data(ident, rkey);

    #[cfg(feature = "fullstack-server")]
    let _entry_res = entry_res?;

    match &*entry_data.read() {
        Some(data) => {
            let title = data.document.title.as_ref();

            let base = if crate::env::WEAVER_APP_ENV == "dev" {
                format_smolstr!("http://127.0.0.1:{}", crate::env::WEAVER_PORT)
            } else {
                SmolStr::new_static(crate::env::WEAVER_APP_HOST)
            };
            let canonical_url = format_smolstr!("{}/{}/sd/{}", base, ident(), rkey());

            let author_info = extract_author_info(&data.profile.inner);
            let author_handle = author_info
                .as_ref()
                .map(|a| a.handle.as_ref().into())
                .unwrap_or_else(|| SmolStr::new_static("unknown"));

            let author_list_view = AuthorListView::new()
                .index(0)
                .record(data.profile.clone())
                .build();

            let description = data
                .document
                .description
                .as_ref()
                .map(|d| d.as_ref().to_string())
                .unwrap_or_default();

            let formatted_date = data
                .document
                .published_at
                .as_ref()
                .format("%B %d, %Y")
                .to_string();

            let content = data.document.content.clone();
            let author_did = ident();

            // Build external URL from publication URL + path (or rkey)
            let doc_path = data
                .document
                .path
                .as_ref()
                .map(|p| p.as_ref().to_string())
                .unwrap_or_else(|| rkey().to_string());

            rsx! {
                EntryOgMeta {
                    title: title.to_string(),
                    description: description.clone(),
                    image_url: String::new(),
                    canonical_url: canonical_url.to_string(),
                    author_handle: author_handle.to_string(),
                }
                document::Link { rel: "stylesheet", href: ENTRY_CSS }
                DefaultNotebookCss {}

                div { class: "entry-page-layout",
                    div { class: "entry-content-main notebook-content",
                        header { class: "entry-metadata",
                            div { class: "entry-header-row",
                                h1 { class: "entry-title", "{title}" }
                            }
                            div { class: "entry-meta-info",
                                div { class: "entry-authors",
                                    AuthorList { authors: vec![author_list_view] }
                                }
                                div { class: "entry-date",
                                    time { "{formatted_date}" }
                                }
                            }
                            if let Some(ref pub_url) = data.publication_url {
                                {
                                    let pub_url = pub_url.trim_end_matches('/');
                                    rsx! {
                                        div { class: "entry-source",
                                            a {
                                                href: "{pub_url}/{doc_path}",
                                                target: "_blank",
                                                class: "source-badge",
                                                "View on Pckt ↗"
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        PcktContent {
                            content: content,
                            author_did: author_did,
                        }
                    }
                }
            }
        }
        None => rsx! { p { "Loading..." } },
    }
}

#[cfg(feature = "pckt")]
#[component]
fn PcktContent(
    content: Option<Vec<jacquard::types::value::Data<'static>>>,
    author_did: AtIdentifier<'static>,
) -> Element {
    use jacquard::IntoStatic;
    use jacquard::prelude::*;
    use weaver_renderer::pckt::{PcktRenderContext, render_content_blocks};

    let fetcher = use_context::<Fetcher>();

    let html = use_resource(move || {
        let content = content.clone();
        let author_did = author_did.clone();
        let fetcher = fetcher.clone();
        async move {
            // Resolve author DID
            let did = match &author_did {
                AtIdentifier::Did(d) => d.clone().into_static(),
                AtIdentifier::Handle(h) => match fetcher.resolve_handle(h).await {
                    Ok(d) => d.into_static(),
                    Err(_) => return String::from("<p>Failed to resolve author</p>"),
                },
            };

            let ctx = PcktRenderContext::new(did);

            if let Some(blocks) = &content {
                render_content_blocks(blocks, &ctx, &fetcher).await
            } else {
                String::from("<p>No content</p>")
            }
        }
    });

    match &*html.read() {
        Some(content) => rsx! {
            div {
                class: "entry pckt-document",
                dangerous_inner_html: "{content}"
            }
        },
        None => rsx! { p { "Rendering..." } },
    }
}

// =============================================================================
// NSID route wrappers (allow replacing at:// with https://host/)
// =============================================================================

#[component]
pub fn WhiteWindEntryNsid(
    ident: ReadSignal<AtIdentifier<'static>>,
    rkey: ReadSignal<SmolStr>,
) -> Element {
    rsx! { WhiteWindEntry { ident, rkey } }
}

#[component]
pub fn LeafletEntryNsid(
    ident: ReadSignal<AtIdentifier<'static>>,
    rkey: ReadSignal<SmolStr>,
) -> Element {
    rsx! { LeafletEntry { ident, rkey } }
}

#[cfg(feature = "pckt")]
#[component]
pub fn PcktEntryNsid(
    ident: ReadSignal<AtIdentifier<'static>>,
    rkey: ReadSignal<SmolStr>,
) -> Element {
    rsx! { PcktEntry { ident, rkey } }
}

#[cfg(feature = "pckt")]
#[component]
pub fn PcktEntryBlogNsid(
    ident: ReadSignal<AtIdentifier<'static>>,
    rkey: ReadSignal<SmolStr>,
) -> Element {
    rsx! { PcktEntry { ident, rkey } }
}

// =============================================================================
// Stub redirects when pckt feature is disabled
// =============================================================================

#[cfg(not(feature = "pckt"))]
#[component]
pub fn PcktEntry(ident: ReadSignal<AtIdentifier<'static>>, rkey: ReadSignal<SmolStr>) -> Element {
    use crate::Route;
    let nav = use_navigator();
    use_effect(move || {
        nav.replace(Route::RecordPage {
            uri: vec![
                "at:".into(),
                "".into(),
                ident().to_string(),
                "site.standard.document".into(),
                rkey().to_string(),
            ],
        });
    });
    rsx! {}
}

#[cfg(not(feature = "pckt"))]
#[component]
pub fn PcktEntryNsid(
    ident: ReadSignal<AtIdentifier<'static>>,
    rkey: ReadSignal<SmolStr>,
) -> Element {
    rsx! { PcktEntry { ident, rkey } }
}

#[cfg(not(feature = "pckt"))]
#[component]
pub fn PcktEntryBlogNsid(
    ident: ReadSignal<AtIdentifier<'static>>,
    rkey: ReadSignal<SmolStr>,
) -> Element {
    rsx! { PcktEntry { ident, rkey } }
}
