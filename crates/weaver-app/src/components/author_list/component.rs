//! AuthorList component for displaying multiple authors with progressive disclosure.

use crate::Route;
use dioxus::prelude::*;
use jacquard::IntoStatic;
use jacquard::types::ident::AtIdentifier;
use jacquard::types::string::{Did, Handle, Uri};
use weaver_api::sh_weaver::actor::ProfileDataViewInner;
use weaver_api::sh_weaver::notebook::AuthorListView;

const AUTHOR_CSS: Asset = asset!("./author.css");

/// Normalized author data extracted from ProfileDataViewInner variants.
#[derive(Clone, PartialEq)]
pub struct AuthorInfo {
    pub did: Did<'static>,
    pub handle: Handle<'static>,
    pub display_name: Option<String>,
    pub avatar_url: Option<Uri<'static>>,
}

impl AuthorInfo {
    /// Check if this author matches an AtIdentifier (comparing DID or handle as appropriate).
    pub fn matches_ident(&self, ident: &AtIdentifier<'_>) -> bool {
        match ident {
            AtIdentifier::Did(did) => self.did == *did,
            AtIdentifier::Handle(handle) => self.handle == *handle,
        }
    }
}

/// Extract normalized author info from ProfileDataViewInner.
/// Returns None for unknown/unhandled variants.
pub fn extract_author_info(inner: &ProfileDataViewInner<'_>) -> Option<AuthorInfo> {
    match inner {
        ProfileDataViewInner::ProfileView(p) => Some(AuthorInfo {
            did: p.did.clone().into_static(),
            handle: p.handle.clone().into_static(),
            display_name: p.display_name.as_ref().map(|n| n.to_string()),
            avatar_url: p.avatar.clone().map(|u| u.into_static()),
        }),
        ProfileDataViewInner::ProfileViewDetailed(p) => Some(AuthorInfo {
            did: p.did.clone().into_static(),
            handle: p.handle.clone().into_static(),
            display_name: p.display_name.as_ref().map(|n| n.to_string()),
            avatar_url: p.avatar.clone().map(|u| u.into_static()),
        }),
        ProfileDataViewInner::TangledProfileView(p) => Some(AuthorInfo {
            did: p.did.clone().into_static(),
            handle: p.handle.clone().into_static(),
            display_name: None,
            avatar_url: None,
        }),
        _ => None,
    }
}

#[derive(Clone, Copy, PartialEq)]
enum DisplayMode {
    Hidden,
    Full,
    Compact,
    Collapsed,
}

fn determine_display_mode(
    author_infos: &[AuthorInfo],
    profile_ident: &Option<AtIdentifier<'static>>,
) -> DisplayMode {
    let count = author_infos.len();

    // Context-aware: single author matching profile ident = hidden
    if count == 1 {
        if let Some(pident) = profile_ident {
            if author_infos[0].matches_ident(pident) {
                return DisplayMode::Hidden;
            }
        }
    }

    match count {
        0 => DisplayMode::Hidden,
        1 | 2 => DisplayMode::Full,
        3 | 4 => DisplayMode::Compact,
        _ => DisplayMode::Collapsed,
    }
}

#[derive(Props, Clone, PartialEq)]
pub struct AuthorListProps {
    /// The authors to display.
    pub authors: Vec<AuthorListView<'static>>,

    /// Optional profile identity for context-aware visibility.
    /// If set and there's only 1 author matching this identity, render nothing.
    #[props(default)]
    pub profile_ident: Option<AtIdentifier<'static>>,

    /// Optional resource owner identity - this author will be sorted first.
    #[props(default)]
    pub owner_ident: Option<AtIdentifier<'static>>,

    /// Avatar size in the full block display (default: 42).
    #[props(default = 42)]
    pub avatar_size: u32,

    /// Additional CSS class for the container.
    #[props(default)]
    pub class: Option<String>,
}

/// Displays a list of authors with progressive disclosure based on count.
///
/// - 1-2 authors: Full block (avatar + name + handle)
/// - 3-4 authors: Compact (names only, comma-separated)
/// - 5+ authors: Collapsed ("Name, Name, et al.")
///
/// Compact/collapsed modes expand on click to show full dropdown.
#[component]
pub fn AuthorList(props: AuthorListProps) -> Element {
    let mut expanded = use_signal(|| false);

    let container_class = props.class.as_deref().unwrap_or("");

    // Pre-extract all author infos, filtering out unknown variants
    let mut author_infos: Vec<AuthorInfo> = props
        .authors
        .iter()
        .filter_map(|a| extract_author_info(&a.record.inner))
        .collect();

    // Sort owner first if specified
    if let Some(ref owner) = props.owner_ident {
        author_infos.sort_by_key(|info| if info.matches_ident(owner) { 0 } else { 1 });
    }

    let mode = determine_display_mode(&author_infos, &props.profile_ident);

    match mode {
        DisplayMode::Hidden => rsx! {},

        DisplayMode::Full => rsx! {
            document::Stylesheet { href: AUTHOR_CSS }
            div { class: "author-list author-list-full {container_class}",
                for info in author_infos.iter() {
                    AuthorBlock { info: info.clone(), avatar_size: props.avatar_size }
                }
            }
        },

        DisplayMode::Compact => rsx! {
            document::Stylesheet { href: AUTHOR_CSS }
            div {
                class: "author-list author-list-compact {container_class}",
                onclick: move |_| expanded.set(true),
                for (i, info) in author_infos.iter().enumerate() {
                    if i > 0 {
                        span { class: "author-separator", ", " }
                    }
                    AuthorInline { info: info.clone() }
                }

                if expanded() {
                    AuthorDropdown {
                        authors: author_infos.clone(),
                        avatar_size: props.avatar_size,
                        on_close: move |_| expanded.set(false),
                    }
                }
            }
        },

        DisplayMode::Collapsed => {
            let first_two: Vec<_> = author_infos.iter().take(2).cloned().collect();
            let remaining = author_infos.len().saturating_sub(2);

            rsx! {
                document::Stylesheet { href: AUTHOR_CSS }
                div {
                    class: "author-list author-list-collapsed {container_class}",
                    onclick: move |_| expanded.set(true),
                    for (i, info) in first_two.iter().enumerate() {
                        if i > 0 {
                            span { class: "author-separator", ", " }
                        }
                        AuthorInline { info: info.clone() }
                    }
                    span { class: "author-et-al", " et al. ({remaining} more)" }

                    if expanded() {
                        AuthorDropdown {
                            authors: author_infos.clone(),
                            avatar_size: props.avatar_size,
                            on_close: move |_| expanded.set(false),
                        }
                    }
                }
            }
        }
    }
}

/// Full author display with avatar, name, and handle (as a link).
#[component]
fn AuthorBlock(info: AuthorInfo, avatar_size: u32) -> Element {
    let display = info
        .display_name
        .as_deref()
        .unwrap_or_else(|| info.handle.as_ref());
    let handle_display = info.handle.as_ref();

    rsx! {
        Link {
            to: Route::RepositoryIndex {
                ident: AtIdentifier::Handle(info.handle.clone())
            },
            class: "embed-author author-block",
            if let Some(ref avatar) = info.avatar_url {
                img {
                    class: "embed-avatar",
                    src: avatar.as_ref(),
                    alt: "",
                    width: "{avatar_size}",
                    height: "{avatar_size}",
                }
            }
            span { class: "embed-author-info",
                span { class: "embed-author-name", "{display}" }
                span { class: "embed-author-handle", "@{handle_display}" }
            }
        }
    }
}

/// Inline author name only (as a link), for compact display.
#[component]
fn AuthorInline(info: AuthorInfo) -> Element {
    let display = info
        .display_name
        .as_deref()
        .unwrap_or_else(|| info.handle.as_ref());

    rsx! {
        Link {
            to: Route::RepositoryIndex {
                ident: AtIdentifier::Handle(info.handle.clone())
            },
            class: "author-inline",
            "{display}"
        }
    }
}

/// Dropdown overlay showing all authors in full block display.
#[component]
fn AuthorDropdown(
    authors: Vec<AuthorInfo>,
    avatar_size: u32,
    on_close: EventHandler<()>,
) -> Element {
    rsx! {
        div {
            class: "author-list-dropdown-overlay",
            onclick: move |e| {
                e.stop_propagation();
                on_close.call(());
            },
            div {
                class: "author-list-dropdown-content",
                onclick: move |e| e.stop_propagation(),
                div { class: "author-list-dropdown",
                    for info in authors.iter() {
                        AuthorBlock { info: info.clone(), avatar_size }
                    }
                }
            }
        }
    }
}
