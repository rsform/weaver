#![allow(non_snake_case)]

use dioxus::prelude::*;
use jacquard::smol_str::{SmolStr, ToSmolStr};
use jacquard::types::string::AtIdentifier;

use crate::components::NotebookCss;
use crate::components::css::DefaultNotebookCss;

/// View a standalone entry by rkey (not in notebook context).
#[component]
pub fn StandaloneEntry(
    ident: ReadSignal<AtIdentifier<'static>>,
    rkey: ReadSignal<SmolStr>,
) -> Element {
    use crate::components::{
        ENTRY_CSS, EntryMarkdown, EntryMetadata, EntryOgMeta, NavButton, extract_preview,
    };
    use weaver_api::sh_weaver::actor::ProfileDataViewInner;

    let (entry_res, entry_data) = crate::data::use_standalone_entry_data(ident, rkey);

    #[cfg(feature = "fullstack-server")]
    let _entry_res = entry_res?;

    match &*entry_data.read() {
        Some(data) => {
            let entry_view = &data.entry_view;
            let entry_record = &data.entry;

            let title = entry_view
                .title
                .as_ref()
                .map(|t| t.as_ref())
                .unwrap_or("Untitled");

            tracing::info!("Entry: {title}");
            let author_handle = entry_view
                .authors
                .first()
                .map(|a| match &a.record.inner {
                    ProfileDataViewInner::ProfileView(p) => p.handle.as_ref().to_string(),
                    ProfileDataViewInner::ProfileViewDetailed(p) => p.handle.as_ref().to_string(),
                    ProfileDataViewInner::TangledProfileView(p) => p.handle.as_ref().to_string(),
                    _ => "unknown".to_string(),
                })
                .unwrap_or_else(|| "unknown".to_string());

            let base = if crate::env::WEAVER_APP_ENV == "dev" {
                format!("http://127.0.0.1:{}", crate::env::WEAVER_PORT)
            } else {
                crate::env::WEAVER_APP_HOST.to_string()
            };
            let canonical_url = format!("{}/{}/e/{}", base, ident(), rkey());
            let description = extract_preview(&entry_record.content, 160);

            let entry_signal = use_signal(|| data.entry.clone());

            if let Some(ref ctx) = data.notebook_context {
                let book_entry_view = &ctx.book_entry_view;
                let notebook = &ctx.notebook;
                let book_title: SmolStr = notebook
                    .title
                    .as_ref()
                    .map(|t| t.as_ref().into())
                    .unwrap_or_else(|| "Untitled".into());

                rsx! {
                    EntryOgMeta {
                        title: title.to_string(),
                        description: description.clone(),
                        image_url: String::new(),
                        canonical_url: canonical_url.clone(),
                        author_handle: author_handle.clone(),
                        book_title: Some(book_title.to_string()),
                    }
                    document::Link { rel: "stylesheet", href: ENTRY_CSS }
                    NotebookCss { ident: ident().to_smolstr(),  notebook: book_title.clone() }

                    div { class: "entry-page-layout",
                        if let Some(ref prev) = book_entry_view.prev {
                            div { class: "nav-gutter nav-prev",
                                NavButton {
                                    direction: "prev",
                                    entry: prev.entry.clone(),
                                    ident: ident(),
                                    book_title: book_title.clone()
                                }
                            }
                        }

                        div { class: "entry-content-main notebook-content",
                            EntryMetadata {
                                entry_view: entry_view.clone(),
                                created_at: entry_record.created_at.clone(),
                                entry_uri: entry_view.uri.clone(),
                                book_title: Some(book_title.clone()),
                                ident: ident()
                            }
                            EntryMarkdown { content: entry_signal, ident }
                        }

                        if let Some(ref next) = book_entry_view.next {
                            div { class: "nav-gutter nav-next",
                                NavButton {
                                    direction: "next",
                                    entry: next.entry.clone(),
                                    ident: ident(),
                                    book_title: book_title.clone()
                                }
                            }
                        }
                    }
                }
            } else {
                // Standalone view without notebook navigation
                rsx! {
                    EntryOgMeta {
                        title: title.to_string(),
                        description: description.clone(),
                        image_url: String::new(),
                        canonical_url: canonical_url.clone(),
                        author_handle: author_handle.clone(),
                    }
                    document::Link { rel: "stylesheet", href: ENTRY_CSS }
                    DefaultNotebookCss {}


                    div { class: "entry-page-layout",
                        div { class: "entry-content-main notebook-content",
                            EntryMetadata {
                                entry_view: entry_view.clone(),
                                created_at: entry_record.created_at.clone(),
                                entry_uri: entry_view.uri.clone(),
                                book_title: None,
                                ident: ident()
                            }
                            EntryMarkdown { content: entry_signal, ident }
                        }
                    }
                }
            }
        }
        None => rsx! { p { "Loading..." } },
    }
}

/// View a notebook entry by rkey.
#[component]
pub fn NotebookEntryByRkey(
    ident: ReadSignal<AtIdentifier<'static>>,
    book_title: ReadSignal<SmolStr>,
    rkey: ReadSignal<SmolStr>,
) -> Element {
    use crate::components::{
        ENTRY_CSS, EntryMarkdown, EntryMetadata, EntryOgMeta, NavButton, extract_preview,
    };
    use weaver_api::sh_weaver::actor::ProfileDataViewInner;

    let (entry_res, entry_data) = crate::data::use_notebook_entry_by_rkey(ident, book_title, rkey);

    #[cfg(feature = "fullstack-server")]
    let _entry_res = entry_res?;

    match &*entry_data.read() {
        Some((book_entry_view, entry_record)) => {
            let entry_view = &book_entry_view.entry;

            let title = entry_view
                .title
                .as_ref()
                .map(|t| t.as_ref())
                .unwrap_or("Untitled");

            let entry_path = entry_view
                .path
                .as_ref()
                .map(|p| p.as_ref().to_string())
                .unwrap_or_else(|| title.to_string());

            tracing::info!("Entry: {entry_path} - {title}");

            let author_handle = entry_view
                .authors
                .first()
                .map(|a| match &a.record.inner {
                    ProfileDataViewInner::ProfileView(p) => p.handle.as_ref().to_string(),
                    ProfileDataViewInner::ProfileViewDetailed(p) => p.handle.as_ref().to_string(),
                    ProfileDataViewInner::TangledProfileView(p) => p.handle.as_ref().to_string(),
                    _ => "unknown".to_string(),
                })
                .unwrap_or_else(|| "unknown".to_string());

            let base = if crate::env::WEAVER_APP_ENV == "dev" {
                format!("http://127.0.0.1:{}", crate::env::WEAVER_PORT)
            } else {
                crate::env::WEAVER_APP_HOST.to_string()
            };
            let canonical_url = format!("{}/{}/{}/e/{}", base, ident(), book_title(), rkey());
            let og_image_url = format!(
                "{}/og/{}/{}/{}.png",
                base,
                ident(),
                book_title(),
                entry_path
            );

            let description = extract_preview(&entry_record.content, 160);
            let entry_signal = use_signal(|| entry_record.clone());

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
                NotebookCss { ident: ident().to_smolstr(),  notebook: book_title() }

                div { class: "entry-page-layout",
                    if let Some(ref prev) = book_entry_view.prev {
                        div { class: "nav-gutter nav-prev",
                            NavButton {
                                direction: "prev",
                                entry: prev.entry.clone(),
                                ident: ident(),
                                book_title: book_title()
                            }
                        }
                    }

                    div { class: "entry-content-main notebook-content",
                        EntryMetadata {
                            entry_view: entry_view.clone(),
                            created_at: entry_record.created_at.clone(),
                            entry_uri: entry_view.uri.clone(),
                            book_title: Some(book_title()),
                            ident: ident()
                        }
                        EntryMarkdown { content: entry_signal, ident }
                    }

                    if let Some(ref next) = book_entry_view.next {
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
        None => rsx! { p { "Loading..." } },
    }
}
