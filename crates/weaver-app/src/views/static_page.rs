use crate::components::css::DefaultNotebookCss;
use crate::components::ENTRY_CSS;
use dioxus::prelude::*;
use weaver_renderer::atproto::ClientWriter;

const ABOUT_MD: &str = include_str!("../../assets/about.md");
const TERMS_MD: &str = include_str!("../../assets/terms.md");
const PRIVACY_MD: &str = include_str!("../../assets/privacy.md");

fn render_markdown(content: &str) -> String {
    let parser = markdown_weaver::Parser::new_ext(content, weaver_renderer::default_md_options());
    let mut html = String::new();
    let _ = ClientWriter::<_, _, ()>::new(parser, &mut html).run();
    html
}

#[derive(Clone, Copy, PartialEq)]
pub enum StaticPageKind {
    About,
    Terms,
    Privacy,
}

impl StaticPageKind {
    fn content(&self) -> &'static str {
        match self {
            StaticPageKind::About => ABOUT_MD,
            StaticPageKind::Terms => TERMS_MD,
            StaticPageKind::Privacy => PRIVACY_MD,
        }
    }

    fn title(&self) -> &'static str {
        match self {
            StaticPageKind::About => "About",
            StaticPageKind::Terms => "Terms of Service",
            StaticPageKind::Privacy => "Privacy Policy",
        }
    }
}

#[component]
pub fn StaticPage(kind: StaticPageKind) -> Element {
    let html = render_markdown(kind.content());

    rsx! {
        DefaultNotebookCss {}
        document::Link { rel: "stylesheet", href: ENTRY_CSS }
        document::Title { "{kind.title()} - Weaver" }

        div { class: "static-page",
            article { class: "entry notebook-content",
                dangerous_inner_html: "{html}"
            }
        }
    }
}

#[component]
pub fn AboutPage() -> Element {
    rsx! { StaticPage { kind: StaticPageKind::About } }
}

#[component]
pub fn TermsPage() -> Element {
    rsx! { StaticPage { kind: StaticPageKind::Terms } }
}

#[component]
pub fn PrivacyPage() -> Element {
    rsx! { StaticPage { kind: StaticPageKind::Privacy } }
}
