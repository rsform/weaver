use crate::{Route, data, fetch};
use dioxus::prelude::*;
use jacquard::{smol_str::SmolStr, types::ident::AtIdentifier};
use weaver_api::com_atproto::repo::strong_ref::StrongRef;
use weaver_api::sh_weaver::notebook::NotebookView;

const NOTEBOOK_CARD_CSS: Asset = asset!("/assets/styling/notebook-card.css");

#[component]
pub fn Repository(ident: ReadSignal<AtIdentifier<'static>>) -> Element {
    tracing::debug!("Repository component rendering for ident: {:?}", ident());
    // Fetch notebooks for this specific DID with SSR support;
    tracing::debug!("Repository component context set up");

    rsx! {
        div {
            Outlet::<Route> {}
        }
    }
}

#[component]
pub fn RepositoryIndex(ident: ReadSignal<AtIdentifier<'static>>) -> Element {
    tracing::debug!(
        "RepositoryIndex component rendering for ident: {:?}",
        ident()
    );
    use crate::components::ProfileDisplay;
    let notebooks = data::use_notebooks_for_did(ident);
    let profile = crate::data::use_profile_data(ident);
    tracing::debug!("RepositoryIndex got profile and notebooks");
    rsx! {
        document::Stylesheet { href: NOTEBOOK_CARD_CSS }

        div { class: "repository-layout",
            // Profile sidebar (desktop) / header (mobile)
            aside { class: "repository-sidebar",
                ProfileDisplay { profile, notebooks }
            }

            // Main content area
            main { class: "repository-main",
                div { class: "notebooks-list",
                    match &*notebooks.read() {
                        Some(notebook_list) => rsx! {
                            for notebook in notebook_list.iter() {
                                {
                                    let view = &notebook.0;
                                    let entries = &notebook.1;
                                    rsx! {
                                        div {
                                            key: "{view.cid}",
                                            NotebookCard {
                                                notebook: view.clone(),
                                                entry_refs: entries.clone()
                                            }
                                        }
                                    }
                                }
                            }
                        },
                        None => rsx! {
                            div { "Loading notebooks..." }
                        }
                    }
                }
            }
        }
    }
}

#[component]
pub fn NotebookCard(
    notebook: NotebookView<'static>,
    entry_refs: Vec<StrongRef<'static>>,
) -> Element {
    use jacquard::IntoStatic;

    let fetcher = use_context::<fetch::Fetcher>();

    let title = notebook
        .title
        .as_ref()
        .map(|t| t.as_ref())
        .unwrap_or("Untitled Notebook");

    // Format date
    let formatted_date = notebook.indexed_at.as_ref().format("%B %d, %Y").to_string();

    // Show authors only if multiple
    let show_authors = notebook.authors.len() > 1;

    let ident = notebook.uri.authority().clone().into_static();
    let book_title: SmolStr = title.to_string().into();

    // Fetch all entries to get first/last
    let ident_for_fetch = ident.clone();
    let book_title_for_fetch = book_title.clone();
    let entries = use_resource(use_reactive!(|(ident_for_fetch, book_title_for_fetch)| {
        let fetcher = fetcher.clone();
        async move {
            fetcher
                .list_notebook_entries(ident_for_fetch, book_title_for_fetch)
                .await
                .ok()
                .flatten()
        }
    }));
    rsx! {
        div { class: "notebook-card",
            div { class: "notebook-card-container",

                Link {
                    to: Route::EntryPage {
                        ident: ident.clone(),
                        book_title: title.to_string().into(),
                        title: "".into() // Will redirect to first entry
                    },
                    class: "notebook-card-header-link",

                    div { class: "notebook-card-header",
                        h2 { class: "notebook-card-title", "{title}" }

                        div { class: "notebook-card-date",
                            time { datetime: "{notebook.indexed_at.as_str()}", "{formatted_date}" }
                        }
                    }
                }

                // Show authors only if multiple
                if show_authors {
                    div { class: "notebook-card-authors",
                        for (i, author) in notebook.authors.iter().enumerate() {
                            if i > 0 { span { class: "author-separator", ", " } }
                            {
                                use weaver_api::sh_weaver::actor::ProfileDataViewInner;

                                match &author.record.inner {
                                    ProfileDataViewInner::ProfileView(profile) => {
                                        let display_name = profile.display_name.as_ref()
                                            .map(|n| n.as_ref())
                                            .unwrap_or("Unknown");
                                        rsx! {
                                            span { class: "author-name", "{display_name}" }
                                        }
                                    }
                                    ProfileDataViewInner::ProfileViewDetailed(profile) => {
                                        let display_name = profile.display_name.as_ref()
                                            .map(|n| n.as_ref())
                                            .unwrap_or("Unknown");
                                        rsx! {
                                            span { class: "author-name", "{display_name}" }
                                        }
                                    }
                                    ProfileDataViewInner::TangledProfileView(profile) => {
                                        rsx! {
                                            span { class: "author-name", "@{profile.handle.as_ref()}" }
                                        }
                                    }
                                    _ => rsx! {
                                        span { class: "author-name", "Unknown" }
                                    }
                                }
                            }
                        }
                    }
                }

                // Entry previews section
                if let Some(Some(entry_list)) = entries() {
                    div { class: "notebook-card-previews",
                        {
                            use jacquard::from_data;
                            use weaver_api::sh_weaver::notebook::entry::Entry;

                            if entry_list.len() <= 5 {
                                // Show all entries if 5 or fewer
                                rsx! {
                                    for  entry_view in entry_list.iter() {
                                        {
                                            let entry_title = entry_view.entry.title.as_ref()
                                                .map(|t| t.as_ref())
                                                .unwrap_or("Untitled");

                                            let preview_html = from_data::<Entry>(&entry_view.entry.record).ok().map(|entry| {
                                                let parser = markdown_weaver::Parser::new(&entry.content);
                                                let mut html_buf = String::new();
                                                markdown_weaver::html::push_html(&mut html_buf, parser);
                                                html_buf
                                            });

                                            let created_at = from_data::<Entry>(&entry_view.entry.record).ok()
                                                .map(|entry| entry.created_at.as_ref().format("%B %d, %Y").to_string());

                                            rsx! {
                                                Link {
                                                    to: Route::EntryPage {
                                                        ident: ident.clone(),
                                                        book_title: book_title.clone(),
                                                        title: entry_title.to_string().into()
                                                    },
                                                    class: "notebook-entry-preview-link",

                                                    div { class: "notebook-entry-preview",
                                                        div { class: "entry-preview-header",
                                                            div { class: "entry-preview-title", "{entry_title}" }
                                                            if let Some(ref date) = created_at {
                                                                div { class: "entry-preview-date", "{date}" }
                                                            }
                                                        }
                                                        if let Some(ref html) = preview_html {
                                                            div { class: "entry-preview-content", dangerous_inner_html: "{html}" }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            } else {
                                // Show first, interstitial, and last
                                rsx! {
                                    if let Some(first_entry) = entry_list.first() {
                                        {
                                            let entry_title = first_entry.entry.title.as_ref()
                                                .map(|t| t.as_ref())
                                                .unwrap_or("Untitled");

                                            let preview_html = from_data::<Entry>(&first_entry.entry.record).ok().map(|entry| {
                                                let parser = markdown_weaver::Parser::new(&entry.content);
                                                let mut html_buf = String::new();
                                                markdown_weaver::html::push_html(&mut html_buf, parser);
                                                html_buf
                                            });

                                            let created_at = from_data::<Entry>(&first_entry.entry.record).ok()
                                                .map(|entry| entry.created_at.as_ref().format("%B %d, %Y").to_string());

                                            rsx! {
                                                Link {
                                                    to: Route::EntryPage {
                                                        ident: ident.clone(),
                                                        book_title: book_title.clone(),
                                                        title: entry_title.to_string().into()
                                                    },
                                                    class: "notebook-entry-preview-link",

                                                    div { class: "notebook-entry-preview notebook-entry-preview-first",
                                                        div { class: "entry-preview-header",
                                                            div { class: "entry-preview-title", "{entry_title}" }
                                                            if let Some(ref date) = created_at {
                                                                div { class: "entry-preview-date", "{date}" }
                                                            }
                                                        }
                                                        if let Some(ref html) = preview_html {
                                                            div { class: "entry-preview-content", dangerous_inner_html: "{html}" }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }

                                    // Interstitial showing count
                                    {
                                        let middle_count = entry_list.len().saturating_sub(2);
                                        rsx! {
                                            div { class: "notebook-entry-interstitial",
                                                "... {middle_count} more "
                                                if middle_count == 1 { "entry" } else { "entries" }
                                                " ..."
                                            }
                                        }
                                    }

                                    if let Some(last_entry) = entry_list.last() {
                                        {
                                            let entry_title = last_entry.entry.title.as_ref()
                                                .map(|t| t.as_ref())
                                                .unwrap_or("Untitled");

                                            let preview_html = from_data::<Entry>(&last_entry.entry.record).ok().map(|entry| {
                                                let parser = markdown_weaver::Parser::new(&entry.content);
                                                let mut html_buf = String::new();
                                                markdown_weaver::html::push_html(&mut html_buf, parser);
                                                html_buf
                                            });

                                            let created_at = from_data::<Entry>(&last_entry.entry.record).ok()
                                                .map(|entry| entry.created_at.as_ref().format("%B %d, %Y").to_string());

                                            rsx! {
                                                Link {
                                                    to: Route::EntryPage {
                                                        ident: ident.clone(),
                                                        book_title: book_title.clone(),
                                                        title: entry_title.to_string().into()
                                                    },
                                                    class: "notebook-entry-preview-link",

                                                    div { class: "notebook-entry-preview notebook-entry-preview-last",
                                                        div { class: "entry-preview-header",
                                                            div { class: "entry-preview-title", "{entry_title}" }
                                                            if let Some(ref date) = created_at {
                                                                div { class: "entry-preview-date", "{date}" }
                                                            }
                                                        }
                                                        if let Some(ref html) = preview_html {
                                                            div { class: "entry-preview-content", dangerous_inner_html: "{html}" }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
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
