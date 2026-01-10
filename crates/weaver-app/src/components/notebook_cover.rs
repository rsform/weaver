#![allow(non_snake_case)]

use crate::components::{AppLink, AppLinkTarget};
use crate::components::AuthorList;
use crate::components::button::{Button, ButtonVariant};
use dioxus::prelude::*;
use jacquard::IntoStatic;
use jacquard::smol_str::SmolStr;
use jacquard::types::ident::AtIdentifier;
use weaver_api::sh_weaver::notebook::NotebookView;

const NOTEBOOK_COVER_CSS: Asset = asset!("/assets/styling/notebook-cover.css");

#[component]
pub fn NotebookCover(
    notebook: NotebookView<'static>,
    title: String,
    #[props(default = false)] is_owner: bool,
    #[props(default)] ident: Option<AtIdentifier<'static>>,
) -> Element {
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
            };
        }
    };

    rsx! {
        document::Stylesheet { href: NOTEBOOK_COVER_CSS }

        div { class: "notebook-cover",
            h1 { class: "notebook-cover-title", "{title}" }

            // Authors section
            if !notebook.authors.is_empty() {
                {
                    let owner = notebook.uri.authority().clone().into_static();
                    rsx! {
                        div { class: "notebook-cover-authors",
                            AuthorList {
                                authors: notebook.authors.clone(),
                                owner_ident: Some(owner),
                                avatar_size: 48,
                            }
                        }
                    }
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

            // Owner actions
            if is_owner {
                if let Some(ref owner_ident) = ident {
                    div { class: "notebook-cover-actions",
                        AppLink {
                            to: AppLinkTarget::NewDraft {
                                ident: owner_ident.clone(),
                                notebook: Some(SmolStr::from(title.as_str()))
                            },
                            class: Some("notebook-cover-action-link".to_string()),
                            Button {
                                variant: ButtonVariant::Outline,
                                "+ Add Entry"
                            }
                        }
                    }
                }
            }
        }
    }
}
