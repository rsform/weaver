use crate::auth::AuthState;
use crate::components::{FeedEntryCard, ProfileActions, ProfileActionsMenubar};
use crate::{Route, data, fetch};
use dioxus::prelude::*;
use jacquard::{smol_str::SmolStr, types::ident::AtIdentifier};
use std::collections::HashSet;
use weaver_api::com_atproto::repo::strong_ref::StrongRef;
use weaver_api::sh_weaver::notebook::{EntryView, NotebookView, entry::Entry};

/// A single item in the profile timeline (either notebook or standalone entry)
#[derive(Clone, PartialEq)]
pub enum ProfileTimelineItem {
    Notebook {
        notebook: NotebookView<'static>,
        entries: Vec<StrongRef<'static>>,
        /// Most recent entry's created_at for sorting
        sort_date: jacquard::types::string::Datetime,
    },
    StandaloneEntry {
        entry_view: EntryView<'static>,
        entry: Entry<'static>,
    },
}

impl ProfileTimelineItem {
    pub fn sort_date(&self) -> &jacquard::types::string::Datetime {
        match self {
            Self::Notebook { sort_date, .. } => sort_date,
            Self::StandaloneEntry { entry, .. } => &entry.created_at,
        }
    }
}

/// OpenGraph and Twitter Card meta tags for profile/repository pages
#[component]
pub fn ProfileOgMeta(
    display_name: String,
    handle: String,
    bio: String,
    image_url: String,
    canonical_url: String,
    notebook_count: usize,
) -> Element {
    let page_title = format!("{} (@{}) | Weaver", display_name, handle);
    let full_description = if notebook_count > 0 {
        format!("{} notebooks · {}", notebook_count, bio)
    } else if bio.is_empty() {
        format!("@{} on Weaver", handle)
    } else {
        bio.clone()
    };

    rsx! {
        document::Title { "{page_title}" }
        document::Meta { property: "og:title", content: "{display_name}" }
        document::Meta { property: "og:description", content: "{full_description}" }
        document::Meta { property: "og:image", content: "{image_url}" }
        document::Meta { property: "og:type", content: "profile" }
        document::Meta { property: "og:url", content: "{canonical_url}" }
        document::Meta { property: "og:site_name", content: "Weaver" }
        document::Meta { property: "profile:username", content: "{handle}" }
        document::Meta { name: "twitter:card", content: "summary_large_image" }
        document::Meta { name: "twitter:title", content: "{display_name}" }
        document::Meta { name: "twitter:description", content: "{full_description}" }
        document::Meta { name: "twitter:image", content: "{image_url}" }
        document::Meta { name: "twitter:creator", content: "@{handle}" }
    }
}

const NOTEBOOK_CARD_CSS: Asset = asset!("/assets/styling/notebook-card.css");

#[component]
pub fn Repository(ident: ReadSignal<AtIdentifier<'static>>) -> Element {
    rsx! {
        div {
            Outlet::<Route> {}
        }
    }
}

#[component]
pub fn RepositoryIndex(ident: ReadSignal<AtIdentifier<'static>>) -> Element {
    use crate::components::ProfileDisplay;
    use jacquard::from_data;
    use weaver_api::sh_weaver::notebook::book::Book;

    let (notebooks_result, notebooks) = data::use_notebooks_for_did(ident);
    let (entries_result, all_entries) = data::use_entries_for_did(ident);
    let (profile_result, profile) = crate::data::use_profile_data(ident);

    #[cfg(feature = "fullstack-server")]
    notebooks_result?;

    #[cfg(feature = "fullstack-server")]
    entries_result?;

    #[cfg(feature = "fullstack-server")]
    profile_result?;

    // Extract pinned URIs from profile (only Weaver ProfileView has pinned)
    let pinned_uris = use_memo(move || {
        use jacquard::IntoStatic;
        use jacquard::types::aturi::AtUri;
        use weaver_api::sh_weaver::actor::ProfileDataViewInner;

        let Some(prof) = profile.read().as_ref().cloned() else {
            return Vec::<AtUri<'static>>::new();
        };

        match &prof.inner {
            ProfileDataViewInner::ProfileView(p) => p
                .pinned
                .as_ref()
                .map(|pins| pins.iter().map(|r| r.uri.clone().into_static()).collect())
                .unwrap_or_default(),
            _ => Vec::new(),
        }
    });

    // Compute standalone entries (entries not in any notebook)
    let standalone_entries = use_memo(move || {
        let nbs = notebooks.read();
        let ents = all_entries.read();

        let (Some(nbs), Some(ents)) = (nbs.as_ref(), ents.as_ref()) else {
            return Vec::new();
        };

        // Collect all entry URIs from all notebook entry_lists
        let notebook_entry_uris: HashSet<&str> = nbs
            .iter()
            .flat_map(|(_, refs)| refs.iter().map(|r| r.uri.as_ref()))
            .collect();

        // Filter entries not in any notebook
        ents.iter()
            .filter(|(view, _)| !notebook_entry_uris.contains(view.uri.as_ref()))
            .cloned()
            .collect::<Vec<_>>()
    });

    // Helper to check if a URI is pinned
    fn is_pinned(uri: &str, pinned: &[jacquard::types::aturi::AtUri<'static>]) -> bool {
        pinned.iter().any(|p| p.as_ref() == uri)
    }

    // Build pinned items (matching notebooks/entries against pinned URIs)
    let pinned_items = use_memo(move || {
        let nbs = notebooks.read();
        let standalone = standalone_entries.read();
        let ents = all_entries.read();
        let pinned = pinned_uris.read();

        let mut items: Vec<ProfileTimelineItem> = Vec::new();

        // Check notebooks
        if let Some(nbs) = nbs.as_ref() {
            if let Some(all_ents) = ents.as_ref() {
                for (notebook, entry_refs) in nbs {
                    if is_pinned(notebook.uri.as_ref(), &pinned) {
                        let sort_date = entry_refs
                            .iter()
                            .filter_map(|r| {
                                all_ents
                                    .iter()
                                    .find(|(v, _)| v.uri.as_ref() == r.uri.as_ref())
                            })
                            .map(|(_, entry)| entry.created_at.clone())
                            .max()
                            .unwrap_or_else(|| notebook.indexed_at.clone());

                        items.push(ProfileTimelineItem::Notebook {
                            notebook: notebook.clone(),
                            entries: entry_refs.clone(),
                            sort_date,
                        });
                    }
                }
            }
        }

        // Check standalone entries
        for (view, entry) in standalone.iter() {
            if is_pinned(view.uri.as_ref(), &pinned) {
                items.push(ProfileTimelineItem::StandaloneEntry {
                    entry_view: view.clone(),
                    entry: entry.clone(),
                });
            }
        }

        // Sort pinned by their order in the pinned list
        items.sort_by_key(|item| {
            let uri = match item {
                ProfileTimelineItem::Notebook { notebook, .. } => notebook.uri.as_ref(),
                ProfileTimelineItem::StandaloneEntry { entry_view, .. } => entry_view.uri.as_ref(),
            };
            pinned
                .iter()
                .position(|p| p.as_ref() == uri)
                .unwrap_or(usize::MAX)
        });

        items
    });

    // Build merged timeline sorted by date (newest first), excluding pinned items
    let timeline = use_memo(move || {
        let nbs = notebooks.read();
        let standalone = standalone_entries.read();
        let ents = all_entries.read();
        let pinned = pinned_uris.read();

        let mut items: Vec<ProfileTimelineItem> = Vec::new();

        // Add notebooks (excluding pinned)
        if let Some(nbs) = nbs.as_ref() {
            if let Some(all_ents) = ents.as_ref() {
                for (notebook, entry_refs) in nbs {
                    if !is_pinned(notebook.uri.as_ref(), &pinned) {
                        let sort_date = entry_refs
                            .iter()
                            .filter_map(|r| {
                                all_ents
                                    .iter()
                                    .find(|(v, _)| v.uri.as_ref() == r.uri.as_ref())
                            })
                            .map(|(_, entry)| entry.created_at.clone())
                            .max()
                            .unwrap_or_else(|| notebook.indexed_at.clone());

                        items.push(ProfileTimelineItem::Notebook {
                            notebook: notebook.clone(),
                            entries: entry_refs.clone(),
                            sort_date,
                        });
                    }
                }
            }
        }

        // Add standalone entries (excluding pinned)
        for (view, entry) in standalone.iter() {
            if !is_pinned(view.uri.as_ref(), &pinned) {
                items.push(ProfileTimelineItem::StandaloneEntry {
                    entry_view: view.clone(),
                    entry: entry.clone(),
                });
            }
        }

        // Sort by date descending (newest first)
        items.sort_by(|a, b| b.sort_date().cmp(&a.sort_date()));

        items
    });

    // Count standalone entries for stats
    let entry_count = use_memo(move || all_entries.read().as_ref().map(|e| e.len()).unwrap_or(0));

    // Build OG metadata when profile is available
    let og_meta = match &*profile.read() {
        Some(profile_view) => {
            use weaver_api::sh_weaver::actor::ProfileDataViewInner;

            let (display_name, handle, bio) = match &profile_view.inner {
                ProfileDataViewInner::ProfileView(p) => (
                    p.display_name
                        .as_ref()
                        .map(|n| n.as_ref().to_string())
                        .unwrap_or_default(),
                    p.handle.as_ref().to_string(),
                    p.description
                        .as_ref()
                        .map(|d| d.as_ref().to_string())
                        .unwrap_or_default(),
                ),
                ProfileDataViewInner::ProfileViewDetailed(p) => (
                    p.display_name
                        .as_ref()
                        .map(|n| n.as_ref().to_string())
                        .unwrap_or_default(),
                    p.handle.as_ref().to_string(),
                    p.description
                        .as_ref()
                        .map(|d| d.as_ref().to_string())
                        .unwrap_or_default(),
                ),
                ProfileDataViewInner::TangledProfileView(p) => {
                    (String::new(), p.handle.as_ref().to_string(), String::new())
                }
                _ => (String::new(), "unknown".to_string(), String::new()),
            };

            let notebook_count = notebooks.read().as_ref().map(|n| n.len()).unwrap_or(0);

            let base = if crate::env::WEAVER_APP_ENV == "dev" {
                format!("http://127.0.0.1:{}", crate::env::WEAVER_PORT)
            } else {
                crate::env::WEAVER_APP_HOST.to_string()
            };
            let og_image_url = format!("{}/og/profile/{}.png", base, ident());
            let canonical_url = format!("{}/{}", base, ident());

            Some(rsx! {
                ProfileOgMeta {
                    display_name: if display_name.is_empty() { handle.clone() } else { display_name },
                    handle,
                    bio,
                    image_url: og_image_url,
                    canonical_url,
                    notebook_count,
                }
            })
        }
        None => None,
    };

    rsx! {
        document::Stylesheet { href: NOTEBOOK_CARD_CSS }
        {og_meta}

        div { class: "repository-layout",
            // Profile sidebar (desktop) / header (mobile)
            aside { class: "repository-sidebar",
                ProfileDisplay { profile, notebooks, entry_count: *entry_count.read() }
            }

            // Main content area
            main { class: "repository-main",
                // Mobile menubar (hidden on desktop)
                ProfileActionsMenubar { ident }

                div { class: "profile-timeline",
                    // Pinned items section
                    {
                        let pinned = pinned_items.read();
                        if !pinned.is_empty() {
                            rsx! {
                                div { class: "pinned-section",
                                    h3 { class: "pinned-header", "Pinned" }
                                    for (idx, item) in pinned.iter().enumerate() {
                                        {
                                            match item {
                                                ProfileTimelineItem::Notebook { notebook, entries, .. } => {
                                                    rsx! {
                                                        div {
                                                            key: "pinned-notebook-{notebook.cid}",
                                                            class: "pinned-item",
                                                            NotebookCard {
                                                                notebook: notebook.clone(),
                                                                entry_refs: entries.clone()
                                                            }
                                                        }
                                                    }
                                                }
                                                ProfileTimelineItem::StandaloneEntry { entry_view, entry } => {
                                                    rsx! {
                                                        div {
                                                            key: "pinned-entry-{idx}",
                                                            class: "pinned-item standalone-entry-item",
                                                            FeedEntryCard {
                                                                entry_view: entry_view.clone(),
                                                                entry: entry.clone()
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        } else {
                            rsx! {}
                        }
                    }

                    // Chronological timeline
                    {
                        let timeline_items = timeline.read();
                        if timeline_items.is_empty() && pinned_items.read().is_empty() {
                            rsx! { div { class: "timeline-empty", "No content yet" } }
                        } else {
                            rsx! {
                                for (idx, item) in timeline_items.iter().enumerate() {
                                    {
                                        match item {
                                            ProfileTimelineItem::Notebook { notebook, entries, .. } => {
                                                rsx! {
                                                    div {
                                                        key: "notebook-{notebook.cid}",
                                                        NotebookCard {
                                                            notebook: notebook.clone(),
                                                            entry_refs: entries.clone()
                                                        }
                                                    }
                                                }
                                            }
                                            ProfileTimelineItem::StandaloneEntry { entry_view, entry } => {
                                                rsx! {
                                                    div {
                                                        key: "entry-{idx}",
                                                        class: "standalone-entry-item",
                                                        FeedEntryCard {
                                                            entry_view: entry_view.clone(),
                                                            entry: entry.clone()
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
    use jacquard::{IntoStatic, from_data};
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
