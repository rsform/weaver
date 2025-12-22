#![allow(non_snake_case)]

use dioxus::prelude::*;
use jacquard::smol_str::{SmolStr, format_smolstr};
use jacquard::types::string::AtIdentifier;
use weaver_api::sh_weaver::notebook::AuthorListView;

use crate::components::css::DefaultNotebookCss;
use crate::components::{AuthorList, extract_author_info};

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
