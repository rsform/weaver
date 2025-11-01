#![allow(non_snake_case)]

use crate::fetch;
use dioxus::prelude::*;
use jacquard::{smol_str::SmolStr, types::string::AtIdentifier, CowStr};
use weaver_api::sh_weaver::notebook::{entry, BookEntryView};

#[component]
pub fn Entry(ident: AtIdentifier<'static>, book_title: SmolStr, title: SmolStr) -> Element {
    let entry = use_resource(use_reactive!(|(ident, book_title, title)| async move {
        let fetcher = use_context::<fetch::CachedFetcher>();
        fetcher
            .get_entry(ident, book_title, title)
            .await
            .ok()
            .flatten()
    }));

    rsx! {
        match &*entry.read_unchecked() {
            Some(Some(entry)) => {
                let class = use_signal(|| String::from("entry"));
                let content = use_signal(||entry.1.clone());
                rsx! { EntryMarkdown {
                    class,
                    content
                } }
            },
            Some(None) => rsx! { p { "Loading entry failed" } },
            None =>  rsx! { p { "Loading..." } }
        }
    }
}

#[component]
pub fn EntryCard(entry: BookEntryView<'static>) -> Element {
    rsx! {}
}

#[derive(Props, Clone, PartialEq)]
pub struct EntryMarkdownProps {
    #[props(default)]
    id: Signal<String>,
    #[props(default)]
    class: Signal<String>,

    content: ReadSignal<entry::Entry<'static>>,
}

/// Render some text as markdown.
pub fn EntryMarkdown(props: EntryMarkdownProps) -> Element {
    let content = &*props.content.read();
    let parser = markdown_weaver::Parser::new(&content.content);

    let mut html_buf = String::new();
    markdown_weaver::html::push_html(&mut html_buf, parser);

    rsx! {
        div {
            id: "{&*props.id.read()}",
            class: "{&*props.class.read()}",
            dangerous_inner_html: "{html_buf}"
        }
    }
}
