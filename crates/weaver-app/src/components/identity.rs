use crate::auth::AuthState;
use crate::components::{ProfileActions, ProfileActionsMenubar};
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
    let (notebooks_result, notebooks) = data::use_notebooks_for_did(ident);
    let (profile_result, profile) = crate::data::use_profile_data(ident);
    tracing::debug!("RepositoryIndex got profile and notebooks");

    #[cfg(feature = "fullstack-server")]
    notebooks_result?;

    #[cfg(feature = "fullstack-server")]
    profile_result?;

    rsx! {
        document::Stylesheet { href: NOTEBOOK_CARD_CSS }

        div { class: "repository-layout",
            // Profile sidebar (desktop) / header (mobile)
            aside { class: "repository-sidebar",
                ProfileDisplay { profile, notebooks }
            }

            // Main content area
            main { class: "repository-main",
                // Mobile menubar (hidden on desktop)
                ProfileActionsMenubar { ident }

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

            // Actions sidebar (desktop only)
            ProfileActions { ident }
        }
    }
}

#[component]
pub fn NotebookCard(
    notebook: NotebookView<'static>,
    entry_refs: Vec<StrongRef<'static>>,
) -> Element {
    use jacquard::{from_data, IntoStatic};
    use weaver_api::sh_weaver::notebook::book::Book;

    let fetcher = use_context::<fetch::Fetcher>();
    let auth_state = use_context::<Signal<AuthState>>();

    let title = notebook
        .title
        .as_ref()
        .map(|t| t.as_ref())
        .unwrap_or("Untitled Notebook");

    // Get notebook path for URLs, fallback to title
    let notebook_path = notebook
        .path
        .as_ref()
        .map(|p| p.as_ref().to_string())
        .unwrap_or_else(|| title.to_string());

    // Check ownership for "Add Entry" link
    let notebook_ident = notebook.uri.authority().clone().into_static();
    let is_owner = {
        let current_did = auth_state.read().did.clone();
        match (&current_did, &notebook_ident) {
            (Some(did), AtIdentifier::Did(nb_did)) => *did == *nb_did,
            _ => false,
        }
    };

    // Format date
    let formatted_date = notebook.indexed_at.as_ref().format("%B %d, %Y").to_string();

    // Show authors only if multiple
    let show_authors = notebook.authors.len() > 1;

    let ident = notebook.uri.authority().clone().into_static();
    let book_title: SmolStr = notebook_path.clone().into();

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
                        book_title: notebook_path.clone().into(),
                        title: "".into() // Will redirect to first entry
                    },
                    class: "notebook-card-header-link",

                    div { class: "notebook-card-header",
                        div { class: "notebook-card-header-top",
                            h2 { class: "notebook-card-title", "{title}" }
                            if is_owner {
                                Link {
                                    to: Route::NewDraft { ident: notebook_ident.clone(), notebook: Some(book_title.clone()) },
                                    class: "notebook-add-entry",
                                    "+ Add"
                                }
                            }
                        }

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

                                            // Get path from view, fallback to title
                                            let entry_path = entry_view.entry.path
                                                .as_ref()
                                                .map(|p| p.as_ref().to_string())
                                                .unwrap_or_else(|| entry_title.to_string());

                                            // Parse entry for created_at and preview
                                            let parsed_entry = from_data::<Entry>(&entry_view.entry.record).ok();

                                            let preview_html = parsed_entry.as_ref().map(|entry| {
                                                let parser = markdown_weaver::Parser::new(&entry.content);
                                                let mut html_buf = String::new();
                                                markdown_weaver::html::push_html(&mut html_buf, parser);
                                                html_buf
                                            });

                                            let created_at = parsed_entry.as_ref()
                                                .map(|entry| entry.created_at.as_ref().format("%B %d, %Y").to_string());

                                            let entry_uri = entry_view.entry.uri.clone().into_static();

                                            rsx! {
                                                div { class: "notebook-entry-preview",
                                                    div { class: "entry-preview-header",
                                                        Link {
                                                            to: Route::EntryPage {
                                                                ident: ident.clone(),
                                                                book_title: book_title.clone(),
                                                                title: entry_path.clone().into()
                                                            },
                                                            class: "entry-preview-title-link",
                                                            div { class: "entry-preview-title", "{entry_title}" }
                                                        }
                                                        if let Some(ref date) = created_at {
                                                            div { class: "entry-preview-date", "{date}" }
                                                        }
                                                        if is_owner {
                                                            crate::components::EntryActions {
                                                                entry_uri,
                                                                entry_title: entry_title.to_string(),
                                                                in_notebook: true,
                                                                notebook_title: Some(book_title.clone())
                                                            }
                                                        }
                                                    }
                                                    if let Some(ref html) = preview_html {
                                                        Link {
                                                            to: Route::EntryPage {
                                                                ident: ident.clone(),
                                                                book_title: book_title.clone(),
                                                                title: entry_path.clone().into()
                                                            },
                                                            class: "entry-preview-content-link",
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

                                            // Get path from view, fallback to title
                                            let entry_path = first_entry.entry.path
                                                .as_ref()
                                                .map(|p| p.as_ref().to_string())
                                                .unwrap_or_else(|| entry_title.to_string());

                                            // Parse entry for created_at and preview
                                            let parsed_entry = from_data::<Entry>(&first_entry.entry.record).ok();

                                            let preview_html = parsed_entry.as_ref().map(|entry| {
                                                let parser = markdown_weaver::Parser::new(&entry.content);
                                                let mut html_buf = String::new();
                                                markdown_weaver::html::push_html(&mut html_buf, parser);
                                                html_buf
                                            });

                                            let created_at = parsed_entry.as_ref()
                                                .map(|entry| entry.created_at.as_ref().format("%B %d, %Y").to_string());

                                            let entry_uri = first_entry.entry.uri.clone().into_static();

                                            rsx! {
                                                div { class: "notebook-entry-preview notebook-entry-preview-first",
                                                    div { class: "entry-preview-header",
                                                        Link {
                                                            to: Route::EntryPage {
                                                                ident: ident.clone(),
                                                                book_title: book_title.clone(),
                                                                title: entry_path.clone().into()
                                                            },
                                                            class: "entry-preview-title-link",
                                                            div { class: "entry-preview-title", "{entry_title}" }
                                                        }
                                                        if let Some(ref date) = created_at {
                                                            div { class: "entry-preview-date", "{date}" }
                                                        }
                                                        if is_owner {
                                                            crate::components::EntryActions {
                                                                entry_uri,
                                                                entry_title: entry_title.to_string(),
                                                                in_notebook: true,
                                                                notebook_title: Some(book_title.clone())
                                                            }
                                                        }
                                                    }
                                                    if let Some(ref html) = preview_html {
                                                        Link {
                                                            to: Route::EntryPage {
                                                                ident: ident.clone(),
                                                                book_title: book_title.clone(),
                                                                title: entry_path.clone().into()
                                                            },
                                                            class: "entry-preview-content-link",
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

                                            // Get path from view, fallback to title
                                            let entry_path = last_entry.entry.path
                                                .as_ref()
                                                .map(|p| p.as_ref().to_string())
                                                .unwrap_or_else(|| entry_title.to_string());

                                            // Parse entry for created_at and preview
                                            let parsed_entry = from_data::<Entry>(&last_entry.entry.record).ok();

                                            let preview_html = parsed_entry.as_ref().map(|entry| {
                                                let parser = markdown_weaver::Parser::new(&entry.content);
                                                let mut html_buf = String::new();
                                                markdown_weaver::html::push_html(&mut html_buf, parser);
                                                html_buf
                                            });

                                            let created_at = parsed_entry.as_ref()
                                                .map(|entry| entry.created_at.as_ref().format("%B %d, %Y").to_string());

                                            let entry_uri = last_entry.entry.uri.clone().into_static();

                                            rsx! {
                                                div { class: "notebook-entry-preview notebook-entry-preview-last",
                                                    div { class: "entry-preview-header",
                                                        Link {
                                                            to: Route::EntryPage {
                                                                ident: ident.clone(),
                                                                book_title: book_title.clone(),
                                                                title: entry_path.clone().into()
                                                            },
                                                            class: "entry-preview-title-link",
                                                            div { class: "entry-preview-title", "{entry_title}" }
                                                        }
                                                        if let Some(ref date) = created_at {
                                                            div { class: "entry-preview-date", "{date}" }
                                                        }
                                                        if is_owner {
                                                            crate::components::EntryActions {
                                                                entry_uri,
                                                                entry_title: entry_title.to_string(),
                                                                in_notebook: true,
                                                                notebook_title: Some(book_title.clone())
                                                            }
                                                        }
                                                    }
                                                    if let Some(ref html) = preview_html {
                                                        Link {
                                                            to: Route::EntryPage {
                                                                ident: ident.clone(),
                                                                book_title: book_title.clone(),
                                                                title: entry_path.clone().into()
                                                            },
                                                            class: "entry-preview-content-link",
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
