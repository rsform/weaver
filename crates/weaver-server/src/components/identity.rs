use crate::{fetch, Route};
use dioxus::prelude::*;
use jacquard::types::ident::AtIdentifier;
use weaver_api::sh_weaver::notebook::NotebookView;

const NOTEBOOK_CARD_CSS: Asset = asset!("/assets/styling/notebook-card.css");

#[component]
pub fn Repository(ident: AtIdentifier<'static>) -> Element {
    rsx! {
        // We can create elements inside the rsx macro with the element name followed by a block of attributes and children.
        div {
            Outlet::<Route> {}
        }
    }
}

#[component]
pub fn RepositoryIndex(ident: AtIdentifier<'static>) -> Element {
    let fetcher = use_context::<fetch::CachedFetcher>();
    let notebooks = use_signal(|| fetcher.list_recent_notebooks());
    rsx! {
        document::Link { rel: "stylesheet", href: NOTEBOOK_CARD_CSS }

        div { class: "notebooks-list",
            for notebook in notebooks.iter() {
                {
                    let view = &notebook.0;
                    rsx! {
                        div {
                            key: "{view.cid}",
                            NotebookCard { notebook: view.clone() }
                        }
                    }
                }
            }
        }
    }
}

#[component]
pub fn NotebookCard(notebook: NotebookView<'static>) -> Element {
    use crate::components::avatar::{Avatar, AvatarImage};
    use jacquard::{from_data, prelude::IdentityResolver, IntoStatic};
    use weaver_api::app_bsky::actor::profile::Profile;
    use weaver_api::sh_weaver::notebook::book::Book;

    let title = notebook
        .title
        .as_ref()
        .map(|t| t.as_ref())
        .unwrap_or("Untitled Notebook");

    // Format date
    let formatted_date = notebook.indexed_at.as_ref().format("%B %d, %Y").to_string();

    // Get first author for display
    let first_author = notebook.authors.first();

    let ident = notebook.uri.authority().clone().into_static();
    let ident_for_avatar = ident.clone();

    rsx! {
        div { class: "notebook-card",
            Link {
                to: Route::Entry {
                    ident,
                    book_title: title.to_string().into(),
                    title: "".into() // Will redirect to first entry
                },
                class: "notebook-card-link",

                div { class: "notebook-card-header",
                    h2 { class: "notebook-card-title", "{title}" }
                }

                div { class: "notebook-card-meta",
                    if let Some(author) = first_author {
                        div { class: "notebook-card-author",
                            {
                                match from_data::<Profile>(author.record.get_at_path(".value").unwrap()) {
                                    Ok(profile) => {
                                        let avatar = profile.avatar
                                            .map(|avatar| {
                                                let cid = avatar.blob().cid();
                                                format!("https://cdn.bsky.app/img/avatar/plain/{}/{cid}@jpeg", ident_for_avatar.as_ref())
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

                    div { class: "notebook-card-date",
                        time { datetime: "{notebook.indexed_at.as_str()}", "{formatted_date}" }
                    }
                }

                if let Some(ref tags) = notebook.tags {
                    if !tags.is_empty() {
                        div { class: "notebook-card-tags",
                            for tag in tags.iter() {
                                span { class: "notebook-card-tag", "{tag}" }
                            }
                        }
                    }
                }
            }
        }
    }
}
