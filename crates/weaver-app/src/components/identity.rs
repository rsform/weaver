use crate::auth::AuthState;
use crate::components::css::DefaultNotebookCss;
use crate::components::{AuthorList, FeedEntryCard, ProfileActions, ProfileActionsMenubar};
use crate::{Route, data, fetch};
use dioxus::prelude::*;
use jacquard::{smol_str::SmolStr, types::ident::AtIdentifier};
use std::collections::HashSet;
use weaver_api::com_atproto::repo::strong_ref::StrongRef;
use weaver_api::sh_weaver::notebook::{
    BookEntryRef, BookEntryView, EntryView, NotebookView, entry::Entry,
};

/// Constructs BookEntryViews from notebook entry refs and all available entries.
///
/// Matches StrongRefs by URI to find the corresponding EntryView,
/// then builds BookEntryView with index and prev/next navigation refs.
fn build_book_entry_views(
    entry_refs: &[StrongRef<'static>],
    all_entries: &[(EntryView<'static>, Entry<'static>)],
) -> Vec<BookEntryView<'static>> {
    use jacquard::IntoStatic;

    // Build a lookup map for faster matching
    let entry_map: std::collections::HashMap<&str, &EntryView<'static>> = all_entries
        .iter()
        .map(|(view, _)| (view.uri.as_ref(), view))
        .collect();

    let mut views = Vec::with_capacity(entry_refs.len());

    for (idx, strong_ref) in entry_refs.iter().enumerate() {
        let Some(entry_view) = entry_map.get(strong_ref.uri.as_ref()).copied() else {
            continue;
        };

        // Build prev ref (if not first)
        let prev = if idx > 0 {
            entry_refs
                .get(idx - 1)
                .and_then(|prev_ref| entry_map.get(prev_ref.uri.as_ref()).copied())
                .map(|prev_view| {
                    BookEntryRef::new()
                        .entry(prev_view.clone())
                        .build()
                        .into_static()
                })
        } else {
            None
        };

        // Build next ref (if not last)
        let next = if idx + 1 < entry_refs.len() {
            entry_refs
                .get(idx + 1)
                .and_then(|next_ref| entry_map.get(next_ref.uri.as_ref()).copied())
                .map(|next_view| {
                    BookEntryRef::new()
                        .entry(next_view.clone())
                        .build()
                        .into_static()
                })
        } else {
            None
        };

        views.push(
            BookEntryView::new()
                .entry(entry_view.clone())
                .index(idx as i64)
                .maybe_prev(prev)
                .maybe_next(next)
                .build()
                .into_static(),
        );
    }

    views
}

/// A single item in the profile timeline (either notebook or standalone entry)
#[derive(Clone, PartialEq)]
pub enum ProfileTimelineItem {
    Notebook {
        notebook: NotebookView<'static>,
        entries: Vec<BookEntryView<'static>>,
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

// Card styles (entry-card, notebook-card) loaded at navbar level
const ENTRY_CSS: Asset = asset!("/assets/styling/entry.css");
const LAYOUTS_CSS: Asset = asset!("/assets/styling/layouts.css");

#[component]
pub fn Repository(ident: ReadSignal<AtIdentifier<'static>>) -> Element {
    rsx! {
        DefaultNotebookCss {  }
        document::Link { rel: "stylesheet", href: LAYOUTS_CSS }
        document::Link { rel: "stylesheet", href: ENTRY_CSS }
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

    let auth_state = use_context::<Signal<AuthState>>();

    // Use client-only versions to avoid SSR issues with concurrent server futures
    let (_profile_res, profile) = data::use_profile_data(ident);
    let (_notebooks_res, notebooks) = data::use_notebooks_for_did(ident);
    let (_entries_res, all_entries) = data::use_entries_for_did(ident);

    #[cfg(feature = "fullstack-server")]
    {
        _profile_res?;
        _notebooks_res?;
        _entries_res?;
    }

    // Check if viewing own profile
    let is_own_profile = use_memo(move || {
        let current_did = auth_state.read().did.clone();
        match (&current_did, ident()) {
            (Some(did), AtIdentifier::Did(profile_did)) => *did == profile_did,
            _ => false,
        }
    });

    // Extract pinned URIs from profile (only Weaver ProfileView has pinned)
    // Returns (Vec for ordering, HashSet for O(1) lookups)
    let pinned_uris = use_memo(move || {
        use jacquard::IntoStatic;
        use weaver_api::sh_weaver::actor::ProfileDataViewInner;

        let Some(prof) = profile.read().as_ref().cloned() else {
            return (Vec::<String>::new(), HashSet::<String>::new());
        };

        match &prof.inner {
            ProfileDataViewInner::ProfileView(p) => {
                let uris: Vec<String> = p
                    .pinned
                    .as_ref()
                    .map(|pins| pins.iter().map(|r| r.uri.as_ref().to_string()).collect())
                    .unwrap_or_default();
                let set: HashSet<String> = uris.iter().cloned().collect();
                (uris, set)
            }
            _ => (Vec::new(), HashSet::new()),
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
    fn is_pinned(uri: &str, pinned_set: &HashSet<String>) -> bool {
        pinned_set.contains(uri)
    }

    // Build pinned items (matching notebooks/entries against pinned URIs)
    let pinned_items = use_memo(move || {
        let nbs = notebooks.read();
        let standalone = standalone_entries.read();
        let ents = all_entries.read();
        let (pinned_vec, pinned_set) = &*pinned_uris.read();

        let mut items: Vec<ProfileTimelineItem> = Vec::new();

        // Check notebooks
        if let Some(nbs) = nbs.as_ref() {
            if let Some(all_ents) = ents.as_ref() {
                for (notebook, entry_refs) in nbs {
                    if is_pinned(notebook.uri.as_ref(), pinned_set) {
                        let book_entries = build_book_entry_views(entry_refs, all_ents);
                        let sort_date = book_entries
                            .iter()
                            .filter_map(|bev| {
                                all_ents
                                    .iter()
                                    .find(|(v, _)| v.uri.as_ref() == bev.entry.uri.as_ref())
                            })
                            .map(|(_, entry)| entry.created_at.clone())
                            .max()
                            .unwrap_or_else(|| notebook.indexed_at.clone());

                        items.push(ProfileTimelineItem::Notebook {
                            notebook: notebook.clone(),
                            entries: book_entries,
                            sort_date,
                        });
                    }
                }
            }
        }

        // Check standalone entries
        for (view, entry) in standalone.iter() {
            if is_pinned(view.uri.as_ref(), pinned_set) {
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
            pinned_vec
                .iter()
                .position(|p| p == uri)
                .unwrap_or(usize::MAX)
        });

        items
    });

    // Build merged timeline sorted by date (newest first), excluding pinned items
    let timeline = use_memo(move || {
        let nbs = notebooks.read();
        let standalone = standalone_entries.read();
        let ents = all_entries.read();
        let (_pinned_vec, pinned_set) = &*pinned_uris.read();

        let mut items: Vec<ProfileTimelineItem> = Vec::new();

        // Add notebooks (excluding pinned)
        if let Some(nbs) = nbs.as_ref() {
            if let Some(all_ents) = ents.as_ref() {
                for (notebook, entry_refs) in nbs {
                    if !is_pinned(notebook.uri.as_ref(), pinned_set) {
                        let book_entries = build_book_entry_views(entry_refs, all_ents);
                        let sort_date = book_entries
                            .iter()
                            .filter_map(|bev| {
                                all_ents
                                    .iter()
                                    .find(|(v, _)| v.uri.as_ref() == bev.entry.uri.as_ref())
                            })
                            .map(|(_, entry)| entry.created_at.clone())
                            .max()
                            .unwrap_or_else(|| notebook.indexed_at.clone());

                        items.push(ProfileTimelineItem::Notebook {
                            notebook: notebook.clone(),
                            entries: book_entries,
                            sort_date,
                        });
                    }
                }
            }
        }

        // Add standalone entries (excluding pinned)
        for (view, entry) in standalone.iter() {
            if !is_pinned(view.uri.as_ref(), pinned_set) {
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
        {og_meta}

        div { class: "repository-layout",
            // Profile sidebar (desktop) / header (mobile)
            aside { class: "repository-sidebar",
                ProfileDisplay { profile, notebooks, entry_count: *entry_count.read(), is_own_profile: is_own_profile() }
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
                                                                entries: entries.clone(),
                                                                is_pinned: true,
                                                                profile_ident: Some(ident()),
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
                                                                entry: entry.clone(),
                                                                show_actions: true,
                                                                is_pinned: true,
                                                                profile_ident: Some(ident()),
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
                                                            entries: entries.clone(),
                                                            is_pinned: false,
                                                            profile_ident: Some(ident()),
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
                                                            entry: entry.clone(),
                                                            show_actions: true,
                                                            is_pinned: false,
                                                            profile_ident: Some(ident()),
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
fn NotebookEntryPreview(
    book_entry_view: weaver_api::sh_weaver::notebook::BookEntryView<'static>,
    ident: AtIdentifier<'static>,
    book_title: SmolStr,
    #[props(default)] extra_class: Option<&'static str>,
) -> Element {
    use jacquard::{IntoStatic, from_data};
    use weaver_api::sh_weaver::notebook::entry::Entry;

    let entry_view = &book_entry_view.entry;

    let entry_title = entry_view
        .title
        .as_ref()
        .map(|t| t.as_ref())
        .unwrap_or("Untitled");

    let entry_path = entry_view
        .path
        .as_ref()
        .map(|p| p.as_ref().to_string())
        .unwrap_or_else(|| entry_title.to_string());

    let parsed_entry = from_data::<Entry>(&entry_view.record).ok();

    let preview_html = parsed_entry.as_ref().map(|entry| {
        let parser = markdown_weaver::Parser::new(&entry.content);
        let mut html_buf = String::new();
        markdown_weaver::html::push_html(&mut html_buf, parser);
        html_buf
    });

    let created_at = parsed_entry
        .as_ref()
        .map(|entry| entry.created_at.as_ref().format("%B %d, %Y").to_string());

    let entry_uri = entry_view.uri.clone().into_static();

    let class_name = if let Some(extra) = extra_class {
        format!("notebook-entry-preview {}", extra)
    } else {
        "notebook-entry-preview".to_string()
    };

    rsx! {
        div { class: "{class_name}",
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
                crate::components::EntryActions {
                    entry_uri,
                    entry_cid: entry_view.cid.clone().into_static(),
                    entry_title: entry_title.to_string(),
                    in_notebook: true,
                    notebook_title: Some(book_title.clone()),
                    permissions: entry_view.permissions.clone()
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

#[component]
pub fn NotebookCard(
    notebook: NotebookView<'static>,
    entries: Vec<BookEntryView<'static>>,
    #[props(default = false)] is_pinned: bool,
    #[props(default)] show_author: Option<bool>,
    /// Profile identity for context-aware author visibility (hides single author on their own profile)
    #[props(default)]
    profile_ident: Option<AtIdentifier<'static>>,
    #[props(default)] on_pinned_changed: Option<EventHandler<bool>>,
    #[props(default)] on_deleted: Option<EventHandler<()>>,
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

    // Show authors: explicit prop overrides, otherwise show only if multiple
    let show_authors = show_author.unwrap_or(notebook.authors.len() > 1);

    let ident = notebook.uri.authority().clone().into_static();
    let book_title: SmolStr = notebook_path.clone().into();

    rsx! {
        div { class: "notebook-card",
            div { class: "notebook-card-container",

                div { class: "notebook-card-header",
                    div { class: "notebook-card-header-top",
                        Link {
                            to: Route::EntryPage {
                                ident: ident.clone(),
                                book_title: notebook_path.clone().into(),
                                title: "".into() // Will redirect to first entry
                            },
                            class: "notebook-card-header-link",
                            h2 { class: "notebook-card-title", "{title}" }
                        }
                        if is_owner {
                            div { class: "notebook-header-actions",
                                Link {
                                    to: Route::NewDraft { ident: notebook_ident.clone(), notebook: Some(book_title.clone()) },
                                    class: "notebook-action-link",
                                    crate::components::button::Button {
                                        variant: crate::components::button::ButtonVariant::Ghost,
                                        "Add"
                                    }
                                }
                                crate::components::NotebookActions {
                                    notebook_uri: notebook.uri.clone().into_static(),
                                    notebook_cid: notebook.cid.clone().into_static(),
                                    notebook_title: title.to_string(),
                                    is_pinned,
                                    on_pinned_changed,
                                    on_deleted
                                }
                            }
                        }
                    }

                    div { class: "notebook-card-date",
                        time { datetime: "{notebook.indexed_at.as_str()}", "{formatted_date}" }
                    }
                }

                // Show authors
                if show_authors {
                    div { class: "notebook-card-authors",
                        AuthorList {
                            authors: notebook.authors.clone(),
                            profile_ident: profile_ident.clone(),
                            owner_ident: Some(ident.clone()),
                        }
                    }
                }

                // Entry previews section
                    div { class: "notebook-card-previews",
                        {
                            use jacquard::from_data;
                            use weaver_api::sh_weaver::notebook::entry::Entry;
                            tracing::info!("rendering entries: {:?}", entries.iter().map(|e|
                              e.entry.uri.as_ref()).collect::<Vec<_>>());

                            if entries.len() <= 5 {
                                // Show all entries if 5 or fewer
                                rsx! {
                                    for entry_view in entries.iter() {
                                        NotebookEntryPreview {
                                            book_entry_view: entry_view.clone(),
                                            ident: ident.clone(),
                                            book_title: book_title.clone(),
                                        }
                                    }
                                }
                            } else {
                                // Show first, interstitial, and last
                                rsx! {
                                    if let Some(first_entry) = entries.first() {
                                        NotebookEntryPreview {
                                            book_entry_view: first_entry.clone(),
                                            ident: ident.clone(),
                                            book_title: book_title.clone(),
                                            extra_class: "notebook-entry-preview-first",
                                        }
                                    }

                                    // Interstitial showing count
                                    {
                                        let middle_count = entries.len().saturating_sub(2);
                                        rsx! {
                                            div { class: "notebook-entry-interstitial",
                                                "... {middle_count} more "
                                                if middle_count == 1 { "entry" } else { "entries" }
                                                " ..."
                                            }
                                        }
                                    }

                                    if let Some(last_entry) = entries.last() {
                                        NotebookEntryPreview {
                                            book_entry_view: last_entry.clone(),
                                            ident: ident.clone(),
                                            book_title: book_title.clone(),
                                            extra_class: "notebook-entry-preview-last",
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
