use crate::Route;
use crate::components::{BskyIcon, TangledIcon};
use dioxus::prelude::*;
use jacquard::types::string::AtIdentifier;

const FOOTER_CSS: Asset = asset!("/assets/styling/footer.css");

const TANGLED_REPO_URL: &str = "https://tangled.org/nonbinary.computer/weaver";
const TANGLED_ISSUES_URL: &str = "https://tangled.org/nonbinary.computer/weaver/issues";
const BSKY_URL: &str = "https://bsky.app/profile/nonbinary.computer";
const GITHUB_SPONSORS_URL: &str = "https://github.com/sponsors/orual";

/// Determines if the current route should show the full footer or just the minimal version.
/// Full footer shows on shell pages (Home, Editor) and on owner's content pages.
fn should_show_full_footer(route: &Route) -> bool {
    match route {
        // Shell pages: always show full footer
        Route::Home {}
        | Route::Editor { .. }
        | Route::AboutPage {}
        | Route::TermsPage {}
        | Route::PrivacyPage {} => true,

        // Callback is transient, minimal is fine
        Route::Callback { .. } => false,

        // Record viewer shows arbitrary user content
        Route::RecordPage { .. } => false,

        // User content pages: check if owner
        Route::RepositoryIndex { ident }
        | Route::DraftsList { ident }
        | Route::DraftEdit { ident, .. }
        | Route::NewDraft { ident, .. }
        | Route::InvitesPage { ident }
        | Route::StandaloneEntry { ident, .. }
        | Route::StandaloneEntryEdit { ident, .. }
        | Route::NotebookIndex { ident, .. }
        | Route::EntryPage { ident, .. }
        | Route::NotebookEntryByRkey { ident, .. }
        | Route::NotebookEntryEdit { ident, .. } => is_owner_ident(ident),
    }
}

/// Check if the given identifier matches the site owner DID.
fn is_owner_ident(ident: &AtIdentifier<'static>) -> bool {
    let owner_did = crate::env::WEAVER_OWNER_DID;
    if owner_did.is_empty() {
        return false;
    }

    match ident {
        AtIdentifier::Did(did) => did.as_ref() == owner_did,
        // Could resolve handle to DID, but keeping it simple for now
        AtIdentifier::Handle(_) => false,
    }
}

#[component]
pub fn Footer() -> Element {
    let route = use_route::<Route>();
    let show_full = should_show_full_footer(&route);

    rsx! {
        document::Link { rel: "stylesheet", href: FOOTER_CSS }

        if show_full {
            footer { class: "site-footer",
                div { class: "footer-content",
                    div { class: "footer-links",
                        a {
                            href: "{crate::env::WEAVER_APP_HOST}/about",
                            class: "footer-link",
                            "About"
                        }

                        span { class: "footer-separator", "|" }

                        a {
                            href: crate::env::WEAVER_TOS_URI,
                            class: "footer-link",
                            "Terms"
                        }

                        span { class: "footer-separator", "|" }

                        a {
                            href: crate::env::WEAVER_PRIVACY_POLICY_URI,
                            class: "footer-link",
                            "Privacy"
                        }

                        span { class: "footer-separator", "|" }

                        a {
                            href: TANGLED_REPO_URL,
                            class: "footer-link",
                            target: "_blank",
                            rel: "noopener",
                            TangledIcon {
                                height: Some(14),
                                width: Some(14),
                            }
                            "Source"
                        }

                        span { class: "footer-separator", "|" }

                        a {
                            href: TANGLED_ISSUES_URL,
                            class: "footer-link",
                            target: "_blank",
                            rel: "noopener",
                            "Report Bug"
                        }

                        span { class: "footer-separator", "|" }

                        a {
                            href: BSKY_URL,
                            class: "footer-link",
                            target: "_blank",
                            rel: "noopener",
                            BskyIcon {
                                height: Some(14),
                                width: Some(14),
                            }
                            "Bluesky"
                        }

                        span { class: "footer-separator", "|" }

                        a {
                            href: GITHUB_SPONSORS_URL,
                            class: "footer-link",
                            target: "_blank",
                            rel: "noopener",
                            "Sponsor"
                        }
                    }
                }
            }
        } else {
            footer { class: "site-footer-minimal",
                div { class: "footer-links",
                    a {
                        href: TANGLED_REPO_URL,
                        class: "footer-link",
                        target: "_blank",
                        rel: "noopener",
                        TangledIcon {
                            height: Some(12),
                            width: Some(12),
                        }
                        "Source"
                    }

                    span { class: "footer-separator", "|" }

                    a {
                        href: TANGLED_ISSUES_URL,
                        class: "footer-link",
                        target: "_blank",
                        rel: "noopener",
                        "Report Bug"
                    }
                }
            }
        }
    }
}
