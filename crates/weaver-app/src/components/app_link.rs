//! Router-agnostic link and navigation for shared components.
//!
//! AppLink dispatches to either `Link<Route>` or `Link<SubdomainRoute>` based on
//! the current LinkMode context, preserving proper client-side navigation semantics.
//!
//! AppNavigate provides programmatic navigation that dispatches similarly.

use crate::env::WEAVER_APP_HOST;
use crate::host_mode::LinkMode;
use crate::{Route, SubdomainRoute};
use dioxus::prelude::*;
use jacquard::smol_str::SmolStr;
use jacquard::types::string::AtIdentifier;

/// Target for router-agnostic links.
#[derive(Clone, PartialEq)]
pub enum AppLinkTarget {
    /// Entry by title path: /:ident/:book/:title or /:title
    Entry {
        ident: AtIdentifier<'static>,
        book_title: SmolStr,
        entry_path: SmolStr,
    },
    /// Entry by rkey: /:ident/:book/e/:rkey or /e/:rkey
    EntryByRkey {
        ident: AtIdentifier<'static>,
        book_title: SmolStr,
        rkey: SmolStr,
    },
    /// Entry edit: /:ident/:book/e/:rkey/edit or /e/:rkey/edit
    EntryEdit {
        ident: AtIdentifier<'static>,
        book_title: SmolStr,
        rkey: SmolStr,
    },
    /// Notebook index: /:ident/:book or /
    Notebook {
        ident: AtIdentifier<'static>,
        book_title: SmolStr,
    },
    /// Profile/repository: /:ident or /u/:ident
    Profile { ident: AtIdentifier<'static> },
    /// Standalone entry: /:ident/e/:rkey (always main domain in subdomain mode)
    StandaloneEntry {
        ident: AtIdentifier<'static>,
        rkey: SmolStr,
    },
    /// Standalone entry edit: /:ident/e/:rkey/edit
    StandaloneEntryEdit {
        ident: AtIdentifier<'static>,
        rkey: SmolStr,
    },
    /// New draft: /:ident/new?notebook=...
    NewDraft {
        ident: AtIdentifier<'static>,
        notebook: Option<SmolStr>,
    },
    /// Drafts list: /:ident/drafts
    Drafts { ident: AtIdentifier<'static> },
    /// Invites page: /:ident/invites
    Invites { ident: AtIdentifier<'static> },
}

#[derive(Props, Clone, PartialEq)]
pub struct AppLinkProps {
    pub to: AppLinkTarget,
    #[props(default)]
    pub class: Option<String>,
    pub children: Element,
}

/// Router-agnostic link component.
///
/// Renders the appropriate `Link<Route>` or `Link<SubdomainRoute>` based on LinkMode context.
#[component]
pub fn AppLink(props: AppLinkProps) -> Element {
    let link_mode = use_context::<LinkMode>();
    let class = props.class.clone().unwrap_or_default();

    match link_mode {
        LinkMode::MainDomain => {
            let route = match props.to.clone() {
                AppLinkTarget::Entry {
                    ident,
                    book_title,
                    entry_path,
                } => Route::EntryPage {
                    ident,
                    book_title,
                    title: entry_path,
                },
                AppLinkTarget::EntryByRkey {
                    ident,
                    book_title,
                    rkey,
                } => Route::NotebookEntryByRkey {
                    ident,
                    book_title,
                    rkey,
                },
                AppLinkTarget::EntryEdit {
                    ident,
                    book_title,
                    rkey,
                } => Route::NotebookEntryEdit {
                    ident,
                    book_title,
                    rkey,
                },
                AppLinkTarget::Notebook { ident, book_title } => {
                    Route::NotebookIndex { ident, book_title }
                }
                AppLinkTarget::Profile { ident } => Route::RepositoryIndex { ident },
                AppLinkTarget::StandaloneEntry { ident, rkey } => {
                    Route::StandaloneEntry { ident, rkey }
                }
                AppLinkTarget::StandaloneEntryEdit { ident, rkey } => {
                    Route::StandaloneEntryEdit { ident, rkey }
                }
                AppLinkTarget::NewDraft { ident, notebook } => Route::NewDraft { ident, notebook },
                AppLinkTarget::Drafts { ident } => Route::DraftsList { ident },
                AppLinkTarget::Invites { ident } => Route::InvitesPage { ident },
            };
            rsx! {
                Link { to: route, class: "{class}", {props.children} }
            }
        }
        LinkMode::Subdomain => {
            // For subdomain mode, some links go to SubdomainRoute, others to main domain
            match props.to.clone() {
                AppLinkTarget::Entry { entry_path, .. } => {
                    let route = SubdomainRoute::SubdomainEntry { title: entry_path };
                    rsx! {
                        Link { to: route, class: "{class}", {props.children} }
                    }
                }
                AppLinkTarget::EntryByRkey { rkey, .. } => {
                    let route = SubdomainRoute::SubdomainEntryByRkey { rkey };
                    rsx! {
                        Link { to: route, class: "{class}", {props.children} }
                    }
                }
                AppLinkTarget::EntryEdit { rkey, .. } => {
                    let route = SubdomainRoute::SubdomainEntryEdit { rkey };
                    rsx! {
                        Link { to: route, class: "{class}", {props.children} }
                    }
                }
                AppLinkTarget::Notebook { .. } => {
                    let route = SubdomainRoute::SubdomainLanding {};
                    rsx! {
                        Link { to: route, class: "{class}", {props.children} }
                    }
                }
                AppLinkTarget::Profile { ident } => {
                    let route = SubdomainRoute::SubdomainProfile { ident };
                    rsx! {
                        Link { to: route, class: "{class}", {props.children} }
                    }
                }
                // These go to main domain in subdomain mode
                AppLinkTarget::StandaloneEntry { ident, rkey } => {
                    let href = format!("{}/{}/e/{}", WEAVER_APP_HOST, ident, rkey);
                    rsx! {
                        a { href: "{href}", class: "{class}", {props.children} }
                    }
                }
                AppLinkTarget::StandaloneEntryEdit { ident, rkey } => {
                    let href = format!("{}/{}/e/{}/edit", WEAVER_APP_HOST, ident, rkey);
                    rsx! {
                        a { href: "{href}", class: "{class}", {props.children} }
                    }
                }
                AppLinkTarget::NewDraft { ident, notebook } => {
                    let href = match notebook {
                        Some(nb) => format!("{}/{}/new?notebook={}", WEAVER_APP_HOST, ident, nb),
                        None => format!("{}/{}/new", WEAVER_APP_HOST, ident),
                    };
                    rsx! {
                        a { href: "{href}", class: "{class}", {props.children} }
                    }
                }
                AppLinkTarget::Drafts { ident } => {
                    let href = format!("{}/{}/drafts", WEAVER_APP_HOST, ident);
                    rsx! {
                        a { href: "{href}", class: "{class}", {props.children} }
                    }
                }
                AppLinkTarget::Invites { ident } => {
                    let href = format!("{}/{}/invites", WEAVER_APP_HOST, ident);
                    rsx! {
                        a { href: "{href}", class: "{class}", {props.children} }
                    }
                }
            }
        }
    }
}

/// Navigation function type for programmatic routing.
pub type NavigateFn = std::rc::Rc<dyn Fn(AppLinkTarget)>;

/// Hook to get the app-wide navigation function.
/// Must be used with AppNavigatorProvider in context.
pub fn use_app_navigate() -> NavigateFn {
    use_context::<NavigateFn>()
}

/// Provides the main domain navigation function.
/// Call this in App to set up navigation context.
pub fn use_main_navigator_provider() {
    let navigator = use_navigator();
    use_context_provider(move || {
        let navigator = navigator.clone();
        std::rc::Rc::new(move |target: AppLinkTarget| {
            let route = match target {
                AppLinkTarget::Entry {
                    ident,
                    book_title,
                    entry_path,
                } => Route::EntryPage {
                    ident,
                    book_title,
                    title: entry_path,
                },
                AppLinkTarget::EntryByRkey {
                    ident,
                    book_title,
                    rkey,
                } => Route::NotebookEntryByRkey {
                    ident,
                    book_title,
                    rkey,
                },
                AppLinkTarget::EntryEdit {
                    ident,
                    book_title,
                    rkey,
                } => Route::NotebookEntryEdit {
                    ident,
                    book_title,
                    rkey,
                },
                AppLinkTarget::Notebook { ident, book_title } => {
                    Route::NotebookIndex { ident, book_title }
                }
                AppLinkTarget::Profile { ident } => Route::RepositoryIndex { ident },
                AppLinkTarget::StandaloneEntry { ident, rkey } => {
                    Route::StandaloneEntry { ident, rkey }
                }
                AppLinkTarget::StandaloneEntryEdit { ident, rkey } => {
                    Route::StandaloneEntryEdit { ident, rkey }
                }
                AppLinkTarget::NewDraft { ident, notebook } => Route::NewDraft { ident, notebook },
                AppLinkTarget::Drafts { ident } => Route::DraftsList { ident },
                AppLinkTarget::Invites { ident } => Route::InvitesPage { ident },
            };
            navigator.push(route);
        }) as NavigateFn
    });
}

/// Provides the subdomain navigation function.
/// Call this in SubdomainApp to set up navigation context.
pub fn use_subdomain_navigator_provider() {
    let navigator = use_navigator();
    use_context_provider(move || {
        let navigator = navigator.clone();
        std::rc::Rc::new(move |target: AppLinkTarget| {
            match target {
                // These navigate within subdomain
                AppLinkTarget::Entry { entry_path, .. } => {
                    navigator.push(SubdomainRoute::SubdomainEntry { title: entry_path });
                }
                AppLinkTarget::EntryByRkey { rkey, .. } => {
                    navigator.push(SubdomainRoute::SubdomainEntryByRkey { rkey });
                }
                AppLinkTarget::EntryEdit { rkey, .. } => {
                    navigator.push(SubdomainRoute::SubdomainEntryEdit { rkey });
                }
                AppLinkTarget::Notebook { .. } => {
                    navigator.push(SubdomainRoute::SubdomainLanding {});
                }
                AppLinkTarget::Profile { ident } => {
                    navigator.push(SubdomainRoute::SubdomainProfile { ident });
                }
                // These go to main domain - use window.location
                AppLinkTarget::StandaloneEntry { ident, rkey }
                | AppLinkTarget::StandaloneEntryEdit { ident, rkey } => {
                    #[cfg(target_arch = "wasm32")]
                    if let Some(window) = web_sys::window() {
                        let path = format!("{}/{}/e/{}", WEAVER_APP_HOST, ident, rkey);
                        let _ = window.location().set_href(&path);
                    }
                    #[cfg(not(target_arch = "wasm32"))]
                    {
                        let _ = ident;
                        let _ = rkey;
                    }
                }
                AppLinkTarget::NewDraft { ident, notebook } => {
                    #[cfg(target_arch = "wasm32")]
                    if let Some(window) = web_sys::window() {
                        let path = match notebook {
                            Some(nb) => {
                                format!("{}/{}/new?notebook={}", WEAVER_APP_HOST, ident, nb)
                            }
                            None => format!("{}/{}/new", WEAVER_APP_HOST, ident),
                        };
                        let _ = window.location().set_href(&path);
                    }
                    #[cfg(not(target_arch = "wasm32"))]
                    {
                        let _ = notebook;
                        let _ = ident;
                    }
                }
                AppLinkTarget::Drafts { ident } => {
                    #[cfg(target_arch = "wasm32")]
                    if let Some(window) = web_sys::window() {
                        let path = format!("{}/{}/drafts", WEAVER_APP_HOST, ident);
                        let _ = window.location().set_href(&path);
                    }
                    #[cfg(not(target_arch = "wasm32"))]
                    let _ = ident;
                }
                AppLinkTarget::Invites { ident } => {
                    #[cfg(target_arch = "wasm32")]
                    if let Some(window) = web_sys::window() {
                        let path = format!("{}/{}/invites", WEAVER_APP_HOST, ident);
                        let _ = window.location().set_href(&path);
                    }
                    #[cfg(not(target_arch = "wasm32"))]
                    let _ = ident;
                }
            }
        }) as NavigateFn
    });
}
