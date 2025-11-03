#![allow(non_snake_case)]

#[allow(unused_imports)]
use crate::{blobcache::BlobCache, fetch};
#[allow(unused_imports)]
use dioxus::{fullstack::extract::Extension, CapturedError};
use dioxus::{
    fullstack::{get_server_url, reqwest},
    prelude::*,
};
use jacquard::smol_str::ToSmolStr;
#[allow(unused_imports)]
use jacquard::{
    smol_str::SmolStr,
    types::{cid::Cid, string::AtIdentifier},
};
#[allow(unused_imports)]
use std::sync::Arc;
use weaver_api::sh_weaver::notebook::{entry, BookEntryView};

#[component]
pub fn Entry(ident: AtIdentifier<'static>, book_title: SmolStr, title: SmolStr) -> Element {
    let entry = use_resource(use_reactive!(|(ident, book_title, title)| async move {
        let fetcher = use_context::<fetch::CachedFetcher>();
        let entry = fetcher
            .get_entry(ident.clone(), book_title, title)
            .await
            .ok()
            .flatten();
        if let Some(entry) = &entry {
            let entry = &entry.1;
            if let Some(embeds) = &entry.embeds {
                if let Some(images) = &embeds.images {
                    for image in &images.images {
                        let cid = image.image.blob().cid();
                        cache_blob(
                            ident.to_smolstr(),
                            cid.to_smolstr(),
                            image.name.as_ref().map(|n| n.to_smolstr()),
                        )
                        .await
                        .ok();
                    }
                }
            }
        }
        entry
    }));

    match &*entry.read_unchecked() {
        Some(Some(entry_data)) => {
            rsx! { EntryMarkdownDirect {
                content: entry_data.1.clone()
            } }
        },
        Some(None) => {
            rsx! { div { class: "error", "Entry not found" } }
        }
        None => rsx! { p { "Loading..." } }
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

/// Render entry content directly without signals
#[component]
fn EntryMarkdownDirect(
    #[props(default)] id: String,
    #[props(default = "entry".to_string())] class: String,
    content: entry::Entry<'static>,
) -> Element {
    let parser = markdown_weaver::Parser::new(&content.content);

    let mut html_buf = String::new();
    markdown_weaver::html::push_html(&mut html_buf, parser);

    rsx! {
        div {
            id: "{id}",
            class: "{class}",
            dangerous_inner_html: "{html_buf}"
        }
    }
}

#[put("/cache/{ident}/{cid}?name", cache: Extension<Arc<BlobCache>>)]
pub async fn cache_blob(ident: SmolStr, cid: SmolStr, name: Option<SmolStr>) -> Result<()> {
    let ident = AtIdentifier::new_owned(ident)?;
    let cid = Cid::new_owned(cid.as_bytes())?;
    cache.cache(ident, cid, name).await
}
