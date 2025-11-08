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

    // Fetch notebooks for this specific DID
    let notebooks = use_resource(use_reactive!(|ident| {
        let fetcher = fetcher.clone();
        async move { fetcher.fetch_notebooks_for_did(&ident).await }
    }));

    rsx! {
        document::Link { rel: "stylesheet", href: NOTEBOOK_CARD_CSS }

        div { class: "notebooks-list",
            match notebooks() {
                Some(Ok(notebook_list)) => rsx! {
                    for notebook in notebook_list.iter() {
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
                },
                Some(Err(_)) => rsx! {
                    div { "Error loading notebooks" }
                },
                None => rsx! {
                    div { "Loading notebooks..." }
                }
            }
        }
    }
}

#[component]
pub fn NotebookCard(notebook: NotebookView<'static>) -> Element {
    use crate::components::avatar::{Avatar, AvatarImage};
    use jacquard::IntoStatic;

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
                                use weaver_api::sh_weaver::actor::ProfileDataViewInner;

                                match &author.record.inner {
                                    ProfileDataViewInner::ProfileView(profile) => {
                                        let display_name = profile.display_name.as_ref().map(|n| n.as_ref()).unwrap_or("Unknown");
                                        rsx! {
                                            if let Some(ref avatar_url) = profile.avatar {
                                                Avatar {
                                                    AvatarImage { src: avatar_url.as_ref() }
                                                }
                                            }
                                            span { class: "author-name", "{display_name}" }
                                        }
                                    }
                                    ProfileDataViewInner::ProfileViewDetailed(profile) => {
                                        let display_name = profile.display_name.as_ref().map(|n| n.as_ref()).unwrap_or("Unknown");
                                        rsx! {
                                            if let Some(ref avatar_url) = profile.avatar {
                                                Avatar {
                                                    AvatarImage { src: avatar_url.as_ref() }
                                                }
                                            }
                                            span { class: "author-name", "{display_name}" }
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
