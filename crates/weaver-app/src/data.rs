//! Feature-gated data fetching layer that abstracts over SSR and client-only modes.
//!
//! In fullstack-server mode, hooks use `use_server_future` with inline async closures.
//! In client-only mode, hooks use `use_resource` with context-provided fetchers.

#[cfg(feature = "server")]
use crate::blobcache::BlobCache;
use dioxus::prelude::*;
#[cfg(feature = "fullstack-server")]
#[allow(unused_imports)]
use dioxus::{fullstack::extract::Extension, CapturedError};
use jacquard::types::{did::Did, string::Handle};
#[allow(unused_imports)]
use jacquard::{
    prelude::IdentityResolver,
    smol_str::SmolStr,
    types::{cid::Cid, string::AtIdentifier},
};
#[allow(unused_imports)]
use std::sync::Arc;
use weaver_api::sh_weaver::notebook::{entry::Entry, BookEntryView};
// ============================================================================
// Wrapper Hooks (feature-gated)
// ============================================================================

/// Fetches entry data with SSR support in fullstack mode.
/// Returns a MappedSignal over the server future resource.
#[cfg(feature = "fullstack-server")]
pub fn use_entry_data(
    ident: AtIdentifier<'static>,
    book_title: SmolStr,
    title: SmolStr,
) -> Result<Memo<Option<(BookEntryView<'static>, Entry<'static>)>>, RenderError> {
    let fetcher = use_context::<crate::fetch::CachedFetcher>();
    let fetcher = fetcher.clone();
    let ident = use_signal(|| ident);
    let book_title = use_signal(|| book_title);
    let entry_title = use_signal(|| title);
    let res = use_server_future(move || {
        let fetcher = fetcher.clone();
        async move {
            if let Some(entry) = fetcher
                .get_entry(ident(), book_title(), entry_title())
                .await
                .ok()
                .flatten()
            {
                let (_book_entry_view, entry_record) = (&entry.0, &entry.1);
                if let Some(embeds) = &entry_record.embeds {
                    if let Some(images) = &embeds.images {
                        let ident = ident.clone();
                        let images = images.clone();
                        for image in &images.images {
                            use jacquard::smol_str::ToSmolStr;

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
                Some((
                    serde_json::to_value(entry.0.clone()).unwrap(),
                    serde_json::to_value(entry.1.clone()).unwrap(),
                ))
            } else {
                None
            }
        }
    });
    res.map(|r| {
        use_memo(move || {
            if let Some(Some((ev, e))) = &*r.read_unchecked() {
                use jacquard::from_json_value;

                let book_entry = from_json_value::<BookEntryView>(ev.clone()).unwrap();
                let entry = from_json_value::<Entry>(e.clone()).unwrap();

                Some((book_entry, entry))
            } else {
                None
            }
        })
    })
}

/// Fetches entry data client-side only (no SSR).
#[cfg(not(feature = "fullstack-server"))]
pub fn use_entry_data(
    ident: AtIdentifier<'static>,
    book_title: SmolStr,
    title: SmolStr,
) -> Result<Memo<Option<(BookEntryView<'static>, Entry<'static>)>>, RenderError> {
    let fetcher = use_context::<crate::fetch::CachedFetcher>();
    let fetcher = fetcher.clone();
    let ident = use_signal(|| ident);
    let book_title = use_signal(|| book_title);
    let entry_title = use_signal(|| title);
    let r = use_resource(move || {
        let fetcher = fetcher.clone();
        async move {
            fetcher
                .get_entry(ident(), book_title(), entry_title())
                .await
                .ok()
                .flatten()
                .map(|arc| (arc.0.clone(), arc.1.clone()))
        }
    });
    Ok(use_memo(move || {
        if let Some(Some((ev, e))) = &*r.read_unchecked() {
            Some((ev.clone(), e.clone()))
        } else {
            None
        }
    }))
}

pub fn use_handle(
    ident: AtIdentifier<'static>,
) -> Result<Memo<AtIdentifier<'static>>, RenderError> {
    let fetcher = use_context::<crate::fetch::CachedFetcher>();
    let fetcher = fetcher.clone();
    let ident = use_signal(|| ident);
    #[cfg(feature = "fullstack-server")]
    let h_str = {
        use_server_future(move || {
            let fetcher = fetcher.clone();
            async move {
                use jacquard::smol_str::ToSmolStr;

                fetcher
                    .client
                    .resolve_ident_owned(&ident())
                    .await
                    .map(|doc| doc.handles().first().map(|h| h.to_smolstr()))
                    .ok()
                    .flatten()
            }
        })
    };
    #[cfg(not(feature = "fullstack-server"))]
    let h_str = {
        use_resource(move || {
            let fetcher = fetcher.clone();
            async move {
                use jacquard::smol_str::ToSmolStr;

                fetcher
                    .client
                    .resolve_ident_owned(&ident())
                    .await
                    .map(|doc| doc.handles().first().map(|h| h.to_smolstr()))
                    .ok()
                    .flatten()
            }
        })
    };
    Ok(h_str.map(|h_str| {
        use_memo(move || {
            if let Some(Some(e)) = &*h_str.read_unchecked() {
                use jacquard::IntoStatic;

                AtIdentifier::Handle(Handle::raw(&e).into_static())
            } else {
                ident()
            }
        })
    })?)
}

/// Hook to render markdown client-side only (no SSR).
#[cfg(feature = "fullstack-server")]
pub fn use_rendered_markdown(
    content: Entry<'static>,
    ident: AtIdentifier<'static>,
) -> Result<Resource<Option<String>>, RenderError> {
    let ident = use_signal(|| ident);
    let content = use_signal(|| content);
    let fetcher = use_context::<crate::fetch::CachedFetcher>();
    Ok(use_server_future(move || {
        let fetcher = fetcher.clone();
        async move {
            let did = match ident() {
                AtIdentifier::Did(d) => d,
                AtIdentifier::Handle(h) => fetcher.client.resolve_handle(&h).await.ok()?,
            };
            Some(render_markdown_impl(content(), did).await)
        }
    })?)
}

/// Hook to render markdown client-side only (no SSR).
#[cfg(not(feature = "fullstack-server"))]
pub fn use_rendered_markdown(
    content: Entry<'static>,
    ident: AtIdentifier<'static>,
) -> Result<Resource<Option<String>>, RenderError> {
    let ident = use_signal(|| ident);
    let content = use_signal(|| content);
    let fetcher = use_context::<crate::fetch::CachedFetcher>();
    Ok(use_resource(move || {
        let fetcher = fetcher.clone();
        async move {
            let did = match ident() {
                AtIdentifier::Did(d) => d,
                AtIdentifier::Handle(h) => fetcher.client.resolve_handle(&h).await.ok()?,
            };
            Some(render_markdown_impl(content(), did).await)
        }
    }))
}

/// Internal implementation of markdown rendering.
async fn render_markdown_impl(content: Entry<'static>, did: Did<'static>) -> String {
    use n0_future::stream::StreamExt;
    use weaver_renderer::{
        atproto::{ClientContext, ClientWriter},
        ContextIterator, NotebookProcessor,
    };

    let ctx = ClientContext::<()>::new(content.clone(), did);
    let parser = markdown_weaver::Parser::new(&content.content);
    let iter = ContextIterator::default(parser);
    let processor = NotebookProcessor::new(ctx, iter);

    let events: Vec<_> = StreamExt::collect(processor).await;

    let mut html_buf = String::new();
    let _ = ClientWriter::<_, _, ()>::new(events.into_iter(), &mut html_buf).run();
    html_buf
}

#[cfg(feature = "fullstack-server")]
#[put("/cache/{ident}/{cid}?name", cache: Extension<Arc<BlobCache>>)]
pub async fn cache_blob(ident: SmolStr, cid: SmolStr, name: Option<SmolStr>) -> Result<()> {
    let ident = AtIdentifier::new_owned(ident)?;
    let cid = Cid::new_owned(cid.as_bytes())?;
    cache.cache(ident, cid, name).await
}
