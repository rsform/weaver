#![allow(non_snake_case)]

use crate::components::avatar::{Avatar, AvatarImage};
use dioxus::prelude::*;
use weaver_api::sh_weaver::notebook::NotebookView;

const NOTEBOOK_COVER_CSS: Asset = asset!("/assets/styling/notebook-cover.css");

#[component]
pub fn NotebookCover(notebook: NotebookView<'static>, title: String) -> Element {
    use jacquard::from_data;
    use weaver_api::sh_weaver::notebook::book::Book;

    // Deserialize the book record from the view
    let book = match from_data::<Book>(&notebook.record) {
        Ok(book) => book,
        Err(_) => {
            return rsx! {
                document::Stylesheet { href: NOTEBOOK_COVER_CSS }
                div { class: "notebook-cover",
                    h1 { class: "notebook-cover-title", "{title}" }
                    div { "Error loading notebook details" }
                }
            }
        }
    };

    rsx! {
        document::Stylesheet { href: NOTEBOOK_COVER_CSS }

        div { class: "notebook-cover",
            h1 { class: "notebook-cover-title", "{title}" }

            // Authors section
            if !notebook.authors.is_empty() {
                div { class: "notebook-cover-authors",
                    NotebookAuthors { authors: notebook.authors.clone() }
                }
            }

            // Metadata
            div { class: "notebook-cover-meta",
                // Entry count
                span { class: "notebook-cover-stat",
                    "{book.entry_list.len()} "
                    if book.entry_list.len() == 1 { "entry" } else { "entries" }
                }

                // Created date
                if let Some(ref created_at) = book.created_at {
                    {
                        let formatted_date = created_at.as_ref().format("%B %d, %Y").to_string();
                        rsx! {
                            span { class: "notebook-cover-date",
                                "Created {formatted_date}"
                            }
                        }
                    }
                }
            }

            // Tags if present
            if let Some(ref tags) = notebook.tags {
                if !tags.is_empty() {
                    div { class: "notebook-cover-tags",
                        for tag in tags.iter() {
                            span { class: "notebook-cover-tag", "{tag}" }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn NotebookAuthors(
    authors: Vec<weaver_api::sh_weaver::notebook::AuthorListView<'static>>,
) -> Element {
    rsx! {
        div { class: "notebook-authors-list",
            for (i, author) in authors.iter().enumerate() {
                if i > 0 { span { class: "author-separator", ", " } }
                NotebookAuthor { author: author.clone() }
            }
        }
    }
}

#[component]
fn NotebookAuthor(author: weaver_api::sh_weaver::notebook::AuthorListView<'static>) -> Element {
    use crate::data::use_handle;
    use weaver_api::sh_weaver::actor::ProfileDataViewInner;

    // Author already has profile data hydrated
    match &author.record.inner {
        ProfileDataViewInner::ProfileView(p) => {
            let display_name = p
                .display_name
                .as_ref()
                .map(|n| n.as_ref())
                .unwrap_or("Unknown");
            let handle = use_handle(p.did.clone().into())?;

            rsx! {
                div { class: "notebook-author",
                    if let Some(ref avatar) = p.avatar {
                        Avatar {
                            AvatarImage { src: avatar.as_ref() }
                        }
                    }
                    div { class: "notebook-author-info",
                        div { class: "notebook-author-name", "{display_name}" }
                        div { class: "notebook-author-handle", "@{handle()}" }
                    }
                }
            }
        }
        ProfileDataViewInner::ProfileViewDetailed(p) => {
            let display_name = p
                .display_name
                .as_ref()
                .map(|n| n.as_ref())
                .unwrap_or("Unknown");
            let handle = use_handle(p.did.clone().into())?;

            rsx! {
                div { class: "notebook-author",
                    if let Some(ref avatar) = p.avatar {
                        Avatar {
                            AvatarImage { src: avatar.as_ref() }
                        }
                    }
                    div { class: "notebook-author-info",
                        div { class: "notebook-author-name", "{display_name}" }
                        div { class: "notebook-author-handle", "@{handle()}" }
                    }
                }
            }
        }
        ProfileDataViewInner::TangledProfileView(p) => {
            rsx! {
                div { class: "notebook-author",
                    div { class: "notebook-author-name", "@{p.handle.as_ref()}" }
                }
            }
        }
        _ => rsx! {
            div { class: "notebook-author",
                "Unknown author"
            }
        },
    }
}
