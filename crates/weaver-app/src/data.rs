//! Feature-gated data fetching layer that abstracts over SSR and client-only modes.
//!
//! In fullstack-server mode, hooks use `use_server_future` with inline async closures.
//! In client-only mode, hooks use `use_resource` with context-provided fetchers.

use crate::auth::AuthState;
#[cfg(feature = "server")]
use crate::blobcache::BlobCache;
use dioxus::prelude::*;
#[cfg(feature = "fullstack-server")]
#[allow(unused_imports)]
use dioxus::{CapturedError, fullstack::extract::Extension};
use jacquard::{
    IntoStatic,
    identity::resolver::IdentityError,
    types::{aturi::AtUri, did::Did, string::Handle},
};
#[allow(unused_imports)]
use jacquard::{
    prelude::IdentityResolver,
    smol_str::{SmolStr, format_smolstr},
    types::{cid::Cid, string::AtIdentifier},
};
#[allow(unused_imports)]
use std::sync::Arc;
use weaver_api::com_atproto::repo::strong_ref::StrongRef;
use weaver_api::sh_weaver::actor::ProfileDataView;
use weaver_api::sh_weaver::notebook::{BookEntryView, EntryView, NotebookView, entry::Entry};
use weaver_common::ResolvedContent;
// ============================================================================
// Wrapper Hooks (feature-gated)
// ============================================================================

/// Fetches entry data with SSR support in fullstack mode.
#[cfg(feature = "fullstack-server")]
pub fn use_entry_data(
    ident: ReadSignal<AtIdentifier<'static>>,
    book_title: ReadSignal<SmolStr>,
    title: ReadSignal<SmolStr>,
) -> (
    Result<Resource<Option<(serde_json::Value, serde_json::Value)>>, RenderError>,
    Memo<Option<(BookEntryView<'static>, Entry<'static>)>>,
) {
    let fetcher = use_context::<crate::fetch::Fetcher>();
    let fetcher = fetcher.clone();
    let res = use_server_future(use_reactive!(|(ident, book_title, title)| {
        let fetcher = fetcher.clone();
        async move {
            let fetch_result = fetcher.get_entry(ident(), book_title(), title()).await;

            match fetch_result {
                Ok(Some(entry)) => {
                    let (_book_entry_view, entry_record) = (&entry.0, &entry.1);
                    if let Some(embeds) = &entry_record.embeds {
                        if let Some(images) = &embeds.images {
                            let ident_val = ident.clone();
                            let images = images.clone();
                            for image in &images.images {
                                use jacquard::smol_str::ToSmolStr;

                                let cid = image.image.blob().cid();
                                cache_blob(
                                    ident_val.to_smolstr(),
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
                }
                Ok(None) => None,
                Err(e) => {
                    tracing::error!(
                        "[use_entry_data] fetch error for {}/{}/{}: {:?}",
                        ident(),
                        book_title(),
                        title(),
                        e
                    );
                    None
                }
            }
        }
    }));
    let memo = use_memo(use_reactive!(|res| {
        let res = res.as_ref().ok()?;
        if let Some(Some((ev, e))) = &*res.read() {
            use jacquard::from_json_value;

            let book_entry = from_json_value::<BookEntryView>(ev.clone()).unwrap();
            let entry = from_json_value::<Entry>(e.clone()).unwrap();
            Some((book_entry, entry))
        } else {
            None
        }
    }));
    (res, memo)
}
/// Fetches entry data client-side only (no SSR).
#[cfg(not(feature = "fullstack-server"))]
pub fn use_entry_data(
    ident: ReadSignal<AtIdentifier<'static>>,
    book_title: ReadSignal<SmolStr>,
    title: ReadSignal<SmolStr>,
) -> (
    Resource<Option<(BookEntryView<'static>, Entry<'static>)>>,
    Memo<Option<(BookEntryView<'static>, Entry<'static>)>>,
) {
    let fetcher = use_context::<crate::fetch::Fetcher>();
    let fetcher = fetcher.clone();
    let res = use_resource(move || {
        let fetcher = fetcher.clone();
        async move {
            if let Some(entry) = fetcher
                .get_entry(ident(), book_title(), title())
                .await
                .ok()
                .flatten()
            {
                let (_book_entry_view, entry_record) = (&entry.0, &entry.1);
                if let Some(embeds) = &entry_record.embeds {
                    if let Some(images) = &embeds.images {
                        #[cfg(all(target_family = "wasm", target_os = "unknown",))]
                        {
                            let _ = crate::service_worker::register_entry_blobs(
                                &ident(),
                                book_title().as_str(),
                                images,
                                &fetcher,
                            )
                            .await;
                        }
                    }
                }
                Some((entry.0.clone(), entry.1.clone()))
            } else {
                None
            }
        }
    });
    let memo = use_memo(move || res.read().clone().flatten());
    (res, memo)
}

#[cfg(feature = "fullstack-server")]
pub fn use_get_handle(did: Did<'static>) -> Memo<AtIdentifier<'static>> {
    let ident = use_signal(use_reactive!(|did| AtIdentifier::Did(did.clone())));
    let old_ident = ident.read().clone();
    let fetcher = use_context::<crate::fetch::Fetcher>();
    let fetcher = fetcher.clone();
    let res = use_resource(move || {
        let client = fetcher.get_client();
        let old_ident = old_ident.clone();
        async move {
            client
                .resolve_ident_owned(&*ident.read())
                .await
                .map(|doc| {
                    doc.handles()
                        .first()
                        .map(|h| AtIdentifier::Handle(h.clone()).into_static())
                })
                .ok()
                .flatten()
                .unwrap_or(old_ident)
        }
    });
    use_memo(move || {
        if let Some(value) = &*res.read() {
            value.clone()
        } else {
            ident.read().clone()
        }
    })
}

#[cfg(not(feature = "fullstack-server"))]
pub fn use_get_handle(did: Did<'static>) -> Memo<AtIdentifier<'static>> {
    let ident = use_signal(use_reactive!(|did| AtIdentifier::Did(did.clone())));
    let old_ident = ident.read().clone();
    let fetcher = use_context::<crate::fetch::Fetcher>();
    let fetcher = fetcher.clone();
    let res = use_resource(move || {
        let client = fetcher.get_client();
        let old_ident = old_ident.clone();
        async move {
            client
                .resolve_ident_owned(&*ident.read())
                .await
                .map(|doc| {
                    doc.handles()
                        .first()
                        .map(|h| AtIdentifier::Handle(h.clone()).into_static())
                })
                .ok()
                .flatten()
                .unwrap_or(old_ident)
        }
    });
    use_memo(move || {
        if let Some(value) = &*res.read() {
            value.clone()
        } else {
            ident.read().clone()
        }
    })
}

#[cfg(feature = "fullstack-server")]
pub fn use_load_handle(
    ident: Option<AtIdentifier<'static>>,
) -> (
    Result<Resource<Option<SmolStr>>, RenderError>,
    Memo<Option<AtIdentifier<'static>>>,
) {
    let ident = use_signal(use_reactive!(|ident| ident.clone()));
    let fetcher = use_context::<crate::fetch::Fetcher>();
    let fetcher = fetcher.clone();
    let res = use_server_future(use_reactive!(|ident| {
        let client = fetcher.get_client();
        async move {
            if let Some(ident) = &*ident.read() {
                use jacquard::smol_str::ToSmolStr;

                client
                    .resolve_ident_owned(ident)
                    .await
                    .map(|doc| doc.handles().first().map(|h| h.to_smolstr()))
                    .unwrap_or(Some(ident.to_smolstr()))
            } else {
                None
            }
        }
    }));

    let memo = use_memo(use_reactive!(|res| {
        if let Ok(res) = res {
            if let Some(value) = &*res.read() {
                if let Some(handle) = value {
                    AtIdentifier::new_owned(handle.clone()).ok()
                } else {
                    ident.read().clone()
                }
            } else {
                ident.read().clone()
            }
        } else {
            ident.read().clone()
        }
    }));

    (res, memo)
}

#[cfg(not(feature = "fullstack-server"))]
pub fn use_load_handle(
    ident: Option<AtIdentifier<'static>>,
) -> (
    Resource<Option<AtIdentifier<'static>>>,
    Memo<Option<AtIdentifier<'static>>>,
) {
    let ident = use_signal(use_reactive!(|ident| ident.clone()));
    let fetcher = use_context::<crate::fetch::Fetcher>();
    let fetcher = fetcher.clone();
    let res = use_resource(move || {
        let client = fetcher.get_client();
        async move {
            if let Some(ident) = &*ident.read() {
                client
                    .resolve_ident_owned(ident)
                    .await
                    .map(|doc| {
                        doc.handles()
                            .first()
                            .map(|h| AtIdentifier::Handle(h.clone()).into_static())
                    })
                    .unwrap_or(Some(ident.clone()))
            } else {
                None
            }
        }
    });

    let memo = use_memo(move || {
        if let Some(value) = &*res.read() {
            value.clone()
        } else {
            ident.read().clone()
        }
    });

    (res, memo)
}
#[cfg(not(feature = "fullstack-server"))]
pub fn use_handle(
    ident: ReadSignal<AtIdentifier<'static>>,
) -> (Resource<AtIdentifier<'static>>, Memo<AtIdentifier<'static>>) {
    let old_ident = ident.read().clone();
    let fetcher = use_context::<crate::fetch::Fetcher>();
    let fetcher = fetcher.clone();
    let res = use_resource(move || {
        let client = fetcher.get_client();
        let old_ident = old_ident.clone();
        async move {
            client
                .resolve_ident_owned(&*ident.read())
                .await
                .map(|doc| {
                    doc.handles()
                        .first()
                        .map(|h| AtIdentifier::Handle(h.clone()).into_static())
                })
                .ok()
                .flatten()
                .unwrap_or(old_ident)
        }
    });

    let memo = use_memo(move || {
        if let Some(value) = &*res.read() {
            value.clone()
        } else {
            ident.read().clone()
        }
    });

    (res, memo)
}
#[cfg(feature = "fullstack-server")]
pub fn use_handle(
    ident: ReadSignal<AtIdentifier<'static>>,
) -> (
    Result<Resource<SmolStr>, RenderError>,
    Memo<AtIdentifier<'static>>,
) {
    let old_ident = ident.read().clone();
    let fetcher = use_context::<crate::fetch::Fetcher>();
    let fetcher = fetcher.clone();
    let res = use_server_future(use_reactive!(|ident| {
        let client = fetcher.get_client();
        let old_ident = old_ident.clone();
        async move {
            use jacquard::smol_str::ToSmolStr;

            client
                .resolve_ident_owned(&ident())
                .await
                .map(|doc| {
                    use jacquard::smol_str::ToSmolStr;

                    doc.handles().first().map(|h| h.to_smolstr())
                })
                .ok()
                .flatten()
                .unwrap_or(old_ident.to_smolstr())
        }
    }));

    let memo = use_memo(use_reactive!(|res| {
        if let Ok(res) = res {
            if let Some(value) = &*res.read() {
                AtIdentifier::new_owned(value).unwrap()
            } else {
                ident.read().clone()
            }
        } else {
            ident.read().clone()
        }
    }));

    (res, memo)
}

/// Hook to render markdown with SSR support.
#[cfg(feature = "fullstack-server")]
pub fn use_rendered_markdown(
    content: ReadSignal<Entry<'static>>,
    ident: ReadSignal<AtIdentifier<'static>>,
) -> (
    Result<Resource<Option<String>>, RenderError>,
    Memo<Option<String>>,
) {
    let fetcher = use_context::<crate::fetch::Fetcher>();
    let fetcher = fetcher.clone();
    let res = use_server_future(use_reactive!(|(content, ident)| {
        let fetcher = fetcher.clone();
        async move {
            let entry = content();
            let did = match ident.read().clone() {
                AtIdentifier::Did(d) => d,
                AtIdentifier::Handle(h) => fetcher.get_client().resolve_handle(&h).await.ok()?,
            };

            let resolved_content = prefetch_embeds(&entry, &fetcher).await;

            Some(render_markdown_impl(entry, did, resolved_content).await)
        }
    }));
    let memo = use_memo(use_reactive!(|res| {
        let res = res.as_ref().ok()?;
        if let Some(Some(value)) = &*res.read() {
            Some(value.clone())
        } else {
            None
        }
    }));
    (res, memo)
}

/// Hook to render markdown client-side only (no SSR).
#[cfg(not(feature = "fullstack-server"))]
pub fn use_rendered_markdown(
    content: ReadSignal<Entry<'static>>,
    ident: ReadSignal<AtIdentifier<'static>>,
) -> (Resource<Option<String>>, Memo<Option<String>>) {
    let fetcher = use_context::<crate::fetch::Fetcher>();
    let fetcher = fetcher.clone();
    let res = use_resource(move || {
        let fetcher = fetcher.clone();
        async move {
            let entry = content();
            let did = match ident() {
                AtIdentifier::Did(d) => d,
                AtIdentifier::Handle(h) => fetcher.get_client().resolve_handle(&h).await.ok()?,
            };

            let resolved_content = prefetch_embeds(&entry, &fetcher).await;

            Some(render_markdown_impl(entry, did, resolved_content).await)
        }
    });
    let memo = use_memo(move || {
        if let Some(Some(value)) = &*res.read() {
            Some(value.clone())
        } else {
            None
        }
    });
    (res, memo)
}

/// Extract AT URIs for embeds from stored records or by parsing markdown.
///
/// Tries stored `embeds.records` first, falls back to parsing markdown content.
fn extract_embed_uris(entry: &Entry<'_>) -> Vec<AtUri<'static>> {
    use jacquard::IntoStatic;

    // Try stored records first
    if let Some(ref embeds) = entry.embeds {
        if let Some(ref records) = embeds.records {
            let stored_uris: Vec<_> = records
                .records
                .iter()
                .map(|r| r.record.uri.clone().into_static())
                .collect();
            if !stored_uris.is_empty() {
                return stored_uris;
            }
        }
    }

    // Fall back to parsing markdown for at:// URIs
    use regex_lite::Regex;
    use std::sync::LazyLock;

    static AT_URI_REGEX: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"at://[^\s\)\]]+").unwrap());

    let uris: Vec<_> = AT_URI_REGEX
        .find_iter(&entry.content)
        .filter_map(|m| AtUri::new(m.as_str()).ok().map(|u| u.into_static()))
        .collect();
    uris
}

/// Pre-fetch embed content for all AT URIs in an entry.
async fn prefetch_embeds(
    entry: &Entry<'static>,
    fetcher: &crate::fetch::Fetcher,
) -> weaver_common::ResolvedContent {
    use weaver_renderer::atproto::fetch_and_render;

    let mut resolved = weaver_common::ResolvedContent::new();
    let uris = extract_embed_uris(entry);

    for uri in uris {
        match fetch_and_render(&uri, fetcher).await {
            Ok(html) => {
                resolved.add_embed(uri, html, None);
            }
            Err(e) => {
                tracing::warn!("[prefetch_embeds] Failed to fetch {}: {}", uri, e);
            }
        }
    }

    resolved
}

/// Internal implementation of markdown rendering.
async fn render_markdown_impl(
    content: Entry<'static>,
    did: Did<'static>,
    resolved_content: weaver_common::ResolvedContent,
) -> String {
    use n0_future::stream::StreamExt;
    use weaver_renderer::{
        ContextIterator, NotebookProcessor,
        atproto::{ClientContext, ClientWriter},
    };

    let ctx = ClientContext::<()>::new(content.clone(), did);
    let parser =
        markdown_weaver::Parser::new_ext(&content.content, weaver_renderer::default_md_options());
    let iter = ContextIterator::default(parser);
    let processor = NotebookProcessor::new(ctx, iter);

    let events: Vec<_> = StreamExt::collect(processor).await;

    let mut html_buf = String::new();
    let writer = ClientWriter::<_, _, ()>::new(events.into_iter(), &mut html_buf)
        .with_embed_provider(resolved_content);
    writer.run().ok();
    html_buf
}

/// Fetches profile data for a given identifier
#[cfg(feature = "fullstack-server")]
pub fn use_profile_data(
    ident: ReadSignal<AtIdentifier<'static>>,
) -> (
    Result<Resource<Option<serde_json::Value>>, RenderError>,
    Memo<Option<ProfileDataView<'static>>>,
) {
    let fetcher = use_context::<crate::fetch::Fetcher>();
    let res = use_server_future(use_reactive!(|ident| {
        let fetcher = fetcher.clone();
        async move {
            fetcher
                .fetch_profile(&ident())
                .await
                .ok()
                .map(|arc| serde_json::to_value(&*arc).ok())
                .flatten()
        }
    }));
    let memo = use_memo(use_reactive!(|res| {
        let res = res.as_ref().ok()?;
        if let Some(Some(value)) = &*res.read() {
            jacquard::from_json_value::<ProfileDataView>(value.clone()).ok()
        } else {
            None
        }
    }));
    (res, memo)
}

/// Fetches profile data client-side only (no SSR)
#[cfg(not(feature = "fullstack-server"))]
pub fn use_profile_data(
    ident: ReadSignal<AtIdentifier<'static>>,
) -> (
    Resource<Option<ProfileDataView<'static>>>,
    Memo<Option<ProfileDataView<'static>>>,
) {
    let fetcher = use_context::<crate::fetch::Fetcher>();
    let res = use_resource(move || {
        let fetcher = fetcher.clone();
        async move {
            fetcher
                .fetch_profile(&ident())
                .await
                .ok()
                .map(|arc| (*arc).clone())
        }
    });
    let memo = use_memo(move || res.read().clone().flatten());
    (res, memo)
}

/// Fetches notebooks for a specific DID
#[cfg(feature = "fullstack-server")]
pub fn use_notebooks_for_did(
    ident: ReadSignal<AtIdentifier<'static>>,
) -> (
    Result<Resource<Option<Vec<serde_json::Value>>>, RenderError>,
    Memo<Option<Vec<(NotebookView<'static>, Vec<StrongRef<'static>>)>>>,
) {
    let fetcher = use_context::<crate::fetch::Fetcher>();
    let res = use_server_future(use_reactive!(|ident| {
        let fetcher = fetcher.clone();
        async move {
            fetcher
                .fetch_notebooks_for_did(&ident())
                .await
                .ok()
                .map(|notebooks| {
                    notebooks
                        .iter()
                        .map(|arc| serde_json::to_value(arc.as_ref()).ok())
                        .collect::<Option<Vec<_>>>()
                })
                .flatten()
        }
    }));
    let memo = use_memo(use_reactive!(|res| {
        let res = res.as_ref().ok()?;
        if let Some(Some(values)) = &*res.read() {
            values
                .iter()
                .map(|v| {
                    jacquard::from_json_value::<(NotebookView, Vec<StrongRef>)>(v.clone()).ok()
                })
                .collect::<Option<Vec<_>>>()
        } else {
            None
        }
    }));
    (res, memo)
}

/// Fetches notebooks client-side only (no SSR)
#[cfg(not(feature = "fullstack-server"))]
pub fn use_notebooks_for_did(
    ident: ReadSignal<AtIdentifier<'static>>,
) -> (
    Resource<Option<Vec<(NotebookView<'static>, Vec<StrongRef<'static>>)>>>,
    Memo<Option<Vec<(NotebookView<'static>, Vec<StrongRef<'static>>)>>>,
) {
    let fetcher = use_context::<crate::fetch::Fetcher>();
    let res = use_resource(move || {
        let fetcher = fetcher.clone();
        async move {
            fetcher
                .fetch_notebooks_for_did(&ident())
                .await
                .ok()
                .map(|notebooks| {
                    notebooks
                        .iter()
                        .map(|arc| arc.as_ref().clone())
                        .collect::<Vec<_>>()
                })
        }
    });
    let memo = use_memo(move || res.read().clone().flatten());
    (res, memo)
}

/// Fetches all entries for a specific DID with SSR support
#[cfg(feature = "fullstack-server")]
pub fn use_entries_for_did(
    ident: ReadSignal<AtIdentifier<'static>>,
) -> (
    Result<Resource<Option<Vec<(serde_json::Value, serde_json::Value)>>>, RenderError>,
    Memo<Option<Vec<(EntryView<'static>, Entry<'static>)>>>,
) {
    let fetcher = use_context::<crate::fetch::Fetcher>();
    let res = use_server_future(use_reactive!(|ident| {
        let fetcher = fetcher.clone();
        async move {
            fetcher
                .fetch_entries_for_did(&ident())
                .await
                .ok()
                .map(|entries| {
                    entries
                        .iter()
                        .filter_map(|arc| {
                            let (view, entry) = arc.as_ref();
                            let view_json = serde_json::to_value(view).ok()?;
                            let entry_json = serde_json::to_value(entry).ok()?;
                            Some((view_json, entry_json))
                        })
                        .collect::<Vec<_>>()
                })
        }
    }));
    let memo = use_memo(use_reactive!(|res| {
        let res = res.as_ref().ok()?;
        if let Some(Some(values)) = &*res.read() {
            let result: Vec<_> = values
                .iter()
                .filter_map(|(view_json, entry_json)| {
                    let view = jacquard::from_json_value::<EntryView>(view_json.clone()).ok()?;
                    let entry = jacquard::from_json_value::<Entry>(entry_json.clone()).ok()?;
                    Some((view, entry))
                })
                .collect();
            Some(result)
        } else {
            None
        }
    }));
    (res, memo)
}

/// Fetches all entries for a specific DID client-side only (no SSR)
#[cfg(not(feature = "fullstack-server"))]
pub fn use_entries_for_did(
    ident: ReadSignal<AtIdentifier<'static>>,
) -> (
    Resource<Option<Vec<(EntryView<'static>, Entry<'static>)>>>,
    Memo<Option<Vec<(EntryView<'static>, Entry<'static>)>>>,
) {
    let fetcher = use_context::<crate::fetch::Fetcher>();
    let res = use_resource(move || {
        let fetcher = fetcher.clone();
        async move {
            fetcher
                .fetch_entries_for_did(&ident())
                .await
                .ok()
                .map(|entries| {
                    entries
                        .iter()
                        .map(|arc| arc.as_ref().clone())
                        .collect::<Vec<_>>()
                })
        }
    });
    let memo = use_memo(move || res.read().clone().flatten());
    (res, memo)
}

// ============================================================================
// Client-only versions (bypass SSR issues on profile page)
// ============================================================================

/// Fetches profile data client-side only - use when SSR causes issues
pub fn use_profile_data_client(
    ident: ReadSignal<AtIdentifier<'static>>,
) -> (
    Resource<Option<ProfileDataView<'static>>>,
    Memo<Option<ProfileDataView<'static>>>,
) {
    let fetcher = use_context::<crate::fetch::Fetcher>();
    let res = use_resource(move || {
        let fetcher = fetcher.clone();
        async move {
            fetcher
                .fetch_profile(&ident())
                .await
                .ok()
                .map(|arc| (*arc).clone())
        }
    });
    let memo = use_memo(move || res.read().clone().flatten());
    (res, memo)
}

/// Fetches notebooks client-side only - use when SSR causes issues
pub fn use_notebooks_for_did_client(
    ident: ReadSignal<AtIdentifier<'static>>,
) -> (
    Resource<Option<Vec<(NotebookView<'static>, Vec<StrongRef<'static>>)>>>,
    Memo<Option<Vec<(NotebookView<'static>, Vec<StrongRef<'static>>)>>>,
) {
    let fetcher = use_context::<crate::fetch::Fetcher>();
    let res = use_resource(move || {
        let fetcher = fetcher.clone();
        async move {
            fetcher
                .fetch_notebooks_for_did(&ident())
                .await
                .ok()
                .map(|notebooks| {
                    notebooks
                        .iter()
                        .map(|arc| arc.as_ref().clone())
                        .collect::<Vec<_>>()
                })
        }
    });
    let memo = use_memo(move || res.read().clone().flatten());
    (res, memo)
}

/// Fetches all entries client-side only - use when SSR causes issues
pub fn use_entries_for_did_client(
    ident: ReadSignal<AtIdentifier<'static>>,
) -> (
    Resource<Option<Vec<(EntryView<'static>, Entry<'static>)>>>,
    Memo<Option<Vec<(EntryView<'static>, Entry<'static>)>>>,
) {
    let fetcher = use_context::<crate::fetch::Fetcher>();
    let res = use_resource(move || {
        let fetcher = fetcher.clone();
        async move {
            fetcher
                .fetch_entries_for_did(&ident())
                .await
                .ok()
                .map(|entries| {
                    entries
                        .iter()
                        .map(|arc| arc.as_ref().clone())
                        .collect::<Vec<_>>()
                })
        }
    });
    let memo = use_memo(move || res.read().clone().flatten());
    (res, memo)
}

/// Fetches notebooks from UFOS with SSR support in fullstack mode
#[cfg(feature = "fullstack-server")]
pub fn use_notebooks_from_ufos() -> (
    Result<Resource<Option<Vec<serde_json::Value>>>, RenderError>,
    Memo<Option<Vec<(NotebookView<'static>, Vec<StrongRef<'static>>)>>>,
) {
    let fetcher = use_context::<crate::fetch::Fetcher>();
    let res = use_server_future(move || {
        let fetcher = fetcher.clone();
        async move {
            fetcher
                .fetch_notebooks_from_ufos()
                .await
                .ok()
                .map(|notebooks| {
                    notebooks
                        .iter()
                        .map(|arc| serde_json::to_value(arc.as_ref()).ok())
                        .collect::<Option<Vec<_>>>()
                })
                .flatten()
        }
    });
    let memo = use_memo(use_reactive!(|res| {
        let res = res.as_ref().ok()?;
        if let Some(Some(values)) = &*res.read() {
            values
                .iter()
                .map(|v| {
                    jacquard::from_json_value::<(NotebookView, Vec<StrongRef>)>(v.clone()).ok()
                })
                .collect::<Option<Vec<_>>>()
        } else {
            None
        }
    }));
    (res, memo)
}

/// Fetches notebooks from UFOS client-side only (no SSR)
#[cfg(not(feature = "fullstack-server"))]
pub fn use_notebooks_from_ufos() -> (
    Resource<Option<Vec<(NotebookView<'static>, Vec<StrongRef<'static>>)>>>,
    Memo<Option<Vec<(NotebookView<'static>, Vec<StrongRef<'static>>)>>>,
) {
    let fetcher = use_context::<crate::fetch::Fetcher>();
    let res = use_resource(move || {
        let fetcher = fetcher.clone();
        async move {
            fetcher
                .fetch_notebooks_from_ufos()
                .await
                .ok()
                .map(|notebooks| {
                    notebooks
                        .iter()
                        .map(|arc| arc.as_ref().clone())
                        .collect::<Vec<_>>()
                })
        }
    });
    let memo = use_memo(move || res.read().clone().flatten());
    (res, memo)
}

/// Fetches entries from UFOS with SSR support in fullstack mode
#[cfg(feature = "fullstack-server")]
pub fn use_entries_from_ufos() -> (
    Result<Resource<Option<Vec<(serde_json::Value, serde_json::Value, u64)>>>, RenderError>,
    Memo<Option<Vec<(EntryView<'static>, Entry<'static>, u64)>>>,
) {
    let fetcher = use_context::<crate::fetch::Fetcher>();
    let res = use_server_future(move || {
        let fetcher = fetcher.clone();
        async move {
            match fetcher.fetch_entries_from_ufos().await {
                Ok(entries) => {
                    // Cache blobs for each entry's embedded images
                    for arc in &entries {
                        let (view, entry, _) = arc.as_ref();
                        if let Some(embeds) = &entry.embeds {
                            if let Some(images) = &embeds.images {
                                use jacquard::smol_str::ToSmolStr;
                                use jacquard::types::aturi::AtUri;
                                // Extract ident from the entry's at-uri
                                if let Ok(at_uri) = AtUri::new(view.uri.as_ref()) {
                                    let ident = at_uri.authority();
                                    for image in &images.images {
                                        let cid = image.image.blob().cid();
                                        cache_blob(
                                            ident.clone().to_smolstr(),
                                            cid.to_smolstr(),
                                            image.name.as_ref().map(|n| n.to_smolstr()),
                                        )
                                        .await
                                        .ok();
                                    }
                                }
                            }
                        }
                    }
                    Some(
                        entries
                            .iter()
                            .filter_map(|arc| {
                                let (view, entry, time) = arc.as_ref();
                                let view_json = serde_json::to_value(view).ok()?;
                                let entry_json = serde_json::to_value(entry).ok()?;
                                Some((view_json, entry_json, *time))
                            })
                            .collect::<Vec<_>>(),
                    )
                }
                Err(e) => {
                    tracing::error!("[use_entries_from_ufos] fetch failed: {:?}", e);
                    None
                }
            }
        }
    });
    let memo = use_memo(use_reactive!(|res| {
        let res = res.as_ref().ok()?;
        if let Some(Some(values)) = &*res.read() {
            let result: Vec<_> = values
                .iter()
                .filter_map(|(view_json, entry_json, time)| {
                    let view = jacquard::from_json_value::<EntryView>(view_json.clone()).ok()?;
                    let entry = jacquard::from_json_value::<Entry>(entry_json.clone()).ok()?;
                    Some((view, entry, *time))
                })
                .collect();
            Some(result)
        } else {
            None
        }
    }));
    (res, memo)
}

/// Fetches entries from UFOS client-side only (no SSR)
#[cfg(not(feature = "fullstack-server"))]
pub fn use_entries_from_ufos() -> (
    Resource<Option<Vec<(EntryView<'static>, Entry<'static>, u64)>>>,
    Memo<Option<Vec<(EntryView<'static>, Entry<'static>, u64)>>>,
) {
    let fetcher = use_context::<crate::fetch::Fetcher>();
    let res = use_resource(move || {
        let fetcher = fetcher.clone();
        async move {
            fetcher.fetch_entries_from_ufos().await.ok().map(|entries| {
                entries
                    .iter()
                    .map(|arc| arc.as_ref().clone())
                    .collect::<Vec<_>>()
            })
        }
    });
    let memo = use_memo(move || res.read().clone().flatten());
    (res, memo)
}

/// Fetches notebook metadata with SSR support in fullstack mode
#[cfg(feature = "fullstack-server")]
pub fn use_notebook(
    ident: ReadSignal<AtIdentifier<'static>>,
    book_title: ReadSignal<SmolStr>,
) -> (
    Result<Resource<Option<serde_json::Value>>, RenderError>,
    Memo<Option<(NotebookView<'static>, Vec<StrongRef<'static>>)>>,
) {
    let fetcher = use_context::<crate::fetch::Fetcher>();
    let res = use_server_future(use_reactive!(|(ident, book_title)| {
        let fetcher = fetcher.clone();
        async move {
            fetcher
                .get_notebook(ident(), book_title())
                .await
                .ok()
                .flatten()
                .map(|arc| serde_json::to_value(arc.as_ref()).ok())
                .flatten()
        }
    }));
    let memo = use_memo(use_reactive!(|res| {
        let res = res.as_ref().ok()?;
        if let Some(Some(value)) = &*res.read() {
            jacquard::from_json_value::<(NotebookView, Vec<StrongRef>)>(value.clone()).ok()
        } else {
            None
        }
    }));
    (res, memo)
}

/// Fetches notebook metadata client-side only (no SSR)
#[cfg(not(feature = "fullstack-server"))]
pub fn use_notebook(
    ident: ReadSignal<AtIdentifier<'static>>,
    book_title: ReadSignal<SmolStr>,
) -> (
    Resource<Option<(NotebookView<'static>, Vec<StrongRef<'static>>)>>,
    Memo<Option<(NotebookView<'static>, Vec<StrongRef<'static>>)>>,
) {
    let fetcher = use_context::<crate::fetch::Fetcher>();
    let res = use_resource(move || {
        let fetcher = fetcher.clone();
        async move {
            fetcher
                .get_notebook(ident(), book_title())
                .await
                .ok()
                .flatten()
                .map(|arc| arc.as_ref().clone())
        }
    });
    let memo = use_memo(move || res.read().clone().flatten());
    (res, memo)
}

/// Fetches notebook entries with SSR support in fullstack mode
#[cfg(feature = "fullstack-server")]
pub fn use_notebook_entries(
    ident: ReadSignal<AtIdentifier<'static>>,
    book_title: ReadSignal<SmolStr>,
) -> (
    Result<Resource<Option<Vec<serde_json::Value>>>, RenderError>,
    Memo<Option<Vec<BookEntryView<'static>>>>,
) {
    let fetcher = use_context::<crate::fetch::Fetcher>();
    let res = use_server_future(use_reactive!(|(ident, book_title)| {
        let fetcher = fetcher.clone();
        async move {
            fetcher
                .list_notebook_entries(ident(), book_title())
                .await
                .ok()
                .flatten()
                .map(|entries| {
                    entries
                        .iter()
                        .map(|e| serde_json::to_value(e).ok())
                        .collect::<Option<Vec<_>>>()
                })
                .flatten()
        }
    }));
    let memo = use_memo(use_reactive!(|res| {
        let res = res.as_ref().ok()?;
        if let Some(Some(values)) = &*res.read() {
            values
                .iter()
                .map(|v| jacquard::from_json_value::<BookEntryView>(v.clone()).ok())
                .collect::<Option<Vec<_>>>()
        } else {
            None
        }
    }));

    (res, memo)
}

/// Fetches notebook entries client-side only (no SSR)
#[cfg(not(feature = "fullstack-server"))]
pub fn use_notebook_entries(
    ident: ReadSignal<AtIdentifier<'static>>,
    book_title: ReadSignal<SmolStr>,
) -> (
    Resource<Option<Vec<BookEntryView<'static>>>>,
    Memo<Option<Vec<BookEntryView<'static>>>>,
) {
    let fetcher = use_context::<crate::fetch::Fetcher>();
    let r = use_resource(move || {
        let fetcher = fetcher.clone();
        async move {
            fetcher
                .list_notebook_entries(ident(), book_title())
                .await
                .ok()
                .flatten()
        }
    });
    let memo = use_memo(move || r.read().as_ref().and_then(|v| v.clone()));
    (r, memo)
}

// ============================================================================
// Ownership Checking
// ============================================================================

/// Check if the current authenticated user owns a resource identified by an AtIdentifier.
///
/// Returns a memo that is:
/// - `Some(true)` if the user is authenticated and their DID matches the resource owner
/// - `Some(false)` if the user is authenticated but doesn't match, or resource is a handle
/// - `None` if the user is not authenticated
///
/// For handles, this does a synchronous check that returns `false` since we can't resolve
/// handles synchronously. Use `use_is_owner_async` for handle resolution.
pub fn use_is_owner(resource_owner: ReadSignal<AtIdentifier<'static>>) -> Memo<Option<bool>> {
    let auth_state = use_context::<Signal<AuthState>>();

    use_memo(move || {
        let current_did = auth_state.read().did.clone()?;
        let owner = resource_owner();

        match owner {
            AtIdentifier::Did(did) => Some(did == current_did),
            AtIdentifier::Handle(_) => Some(false), // Can't resolve synchronously
        }
    })
}

/// Check ownership with async handle resolution.
///
/// Returns a resource that resolves to:
/// - `Some(true)` if the user owns the resource
/// - `Some(false)` if the user doesn't own the resource
/// - `None` if the user is not authenticated
#[cfg(feature = "fullstack-server")]
pub fn use_is_owner_async(
    resource_owner: ReadSignal<AtIdentifier<'static>>,
) -> Resource<Option<bool>> {
    let auth_state = use_context::<Signal<AuthState>>();
    let fetcher = use_context::<crate::fetch::Fetcher>();

    use_resource(move || {
        let fetcher = fetcher.clone();
        let owner = resource_owner();
        async move {
            let current_did = auth_state.read().did.clone()?;

            match owner {
                AtIdentifier::Did(did) => Some(did == current_did),
                AtIdentifier::Handle(handle) => match fetcher.resolve_handle(&handle).await {
                    Ok(resolved_did) => Some(resolved_did == current_did),
                    Err(_) => Some(false),
                },
            }
        }
    })
}

/// Check ownership with async handle resolution (client-only mode).
#[cfg(not(feature = "fullstack-server"))]
pub fn use_is_owner_async(
    resource_owner: ReadSignal<AtIdentifier<'static>>,
) -> Resource<Option<bool>> {
    let auth_state = use_context::<Signal<AuthState>>();
    let fetcher = use_context::<crate::fetch::Fetcher>();

    use_resource(move || {
        let fetcher = fetcher.clone();
        let owner = resource_owner();
        async move {
            let current_did = auth_state.read().did.clone()?;

            match owner {
                AtIdentifier::Did(did) => Some(did == current_did),
                AtIdentifier::Handle(handle) => match fetcher.resolve_handle(&handle).await {
                    Ok(resolved_did) => Some(resolved_did == current_did),
                    Err(_) => Some(false),
                },
            }
        }
    })
}

// ============================================================================
// Edit Access Checking (Ownership + Collaboration)
// ============================================================================

use weaver_api::sh_weaver::actor::ProfileDataViewInner;
use weaver_api::sh_weaver::notebook::{AuthorListView, PermissionsState};

/// Extract DID from a ProfileDataView by matching on the inner variant.
pub fn extract_did_from_author(author: &AuthorListView<'_>) -> Option<Did<'static>> {
    match &author.record.inner {
        ProfileDataViewInner::ProfileView(p) => Some(p.did.clone().into_static()),
        ProfileDataViewInner::ProfileViewDetailed(p) => Some(p.did.clone().into_static()),
        ProfileDataViewInner::TangledProfileView(p) => Some(p.did.clone().into_static()),
        _ => None,
    }
}

/// Check if the current user can edit a resource based on the permissions state.
///
/// Returns a memo that is:
/// - `Some(true)` if the user is authenticated and their DID is in permissions.editors
/// - `Some(false)` if the user is authenticated but not in editors
/// - `None` if the user is not authenticated or permissions not yet loaded
///
/// This checks the ACL-based permissions (who CAN edit), not authors (who contributed).
pub fn use_can_edit(permissions: Memo<Option<PermissionsState<'static>>>) -> Memo<Option<bool>> {
    let auth_state = use_context::<Signal<AuthState>>();

    use_memo(move || {
        let current_did = auth_state.read().did.clone()?;
        let perms = permissions()?;

        // Check if current user's DID is in the editors list
        let can_edit = perms.editors.iter().any(|grant| grant.did == current_did);

        Some(can_edit)
    })
}

/// Legacy: Check if the current user can edit based on authors list.
///
/// Use `use_can_edit` with permissions instead when available.
/// This is kept for backwards compatibility during transition.
pub fn use_can_edit_from_authors(
    authors: Memo<Vec<AuthorListView<'static>>>,
) -> Memo<Option<bool>> {
    let auth_state = use_context::<Signal<AuthState>>();

    use_memo(move || {
        let current_did = auth_state.read().did.clone()?;
        let author_list = authors();

        let can_edit = author_list
            .iter()
            .filter_map(extract_did_from_author)
            .any(|did| did == current_did);

        Some(can_edit)
    })
}

/// Check edit access for a resource URI using the WeaverExt trait methods.
///
/// This performs an async check that queries Constellation for collaboration records.
/// Use this when you have a resource URI but not the pre-populated authors list.
pub fn use_can_edit_resource(resource_uri: ReadSignal<AtUri<'static>>) -> Resource<Option<bool>> {
    let auth_state = use_context::<Signal<AuthState>>();
    let fetcher = use_context::<crate::fetch::Fetcher>();

    use_resource(move || {
        let fetcher = fetcher.clone();
        let uri = resource_uri();
        async move {
            use weaver_common::agent::WeaverExt;

            let current_did = auth_state.read().did.clone()?;

            // Check ownership first (fast path)
            if let AtIdentifier::Did(owner_did) = uri.authority() {
                if *owner_did == current_did {
                    return Some(true);
                }
            }

            // Check collaboration via Constellation
            match fetcher.can_user_edit_resource(&uri, &current_did).await {
                Ok(can_edit) => Some(can_edit),
                Err(_) => Some(false),
            }
        }
    })
}

// ============================================================================
// Standalone Entry by Rkey Hooks
// ============================================================================

/// Fetches standalone entry data by rkey with SSR support.
/// Returns entry + optional notebook context if entry is in exactly one notebook.
#[cfg(feature = "fullstack-server")]
pub fn use_standalone_entry_data(
    ident: ReadSignal<AtIdentifier<'static>>,
    rkey: ReadSignal<SmolStr>,
) -> (
    Result<
        Resource<
            Option<(
                serde_json::Value,
                serde_json::Value,
                Option<(serde_json::Value, serde_json::Value)>,
            )>,
        >,
        RenderError,
    >,
    Memo<Option<crate::fetch::StandaloneEntryData>>,
) {
    let fetcher = use_context::<crate::fetch::Fetcher>();
    let res = use_server_future(use_reactive!(|(ident, rkey)| {
        let fetcher = fetcher.clone();
        async move {
            match fetcher.get_entry_by_rkey(ident(), rkey()).await {
                Ok(Some(data)) => {
                    // Cache blobs for embedded images
                    if let Some(embeds) = &data.entry.embeds {
                        if let Some(images) = &embeds.images {
                            use jacquard::smol_str::ToSmolStr;
                            use jacquard::types::aturi::AtUri;
                            if let Ok(at_uri) = AtUri::new(data.entry_view.uri.as_ref()) {
                                let ident_str = at_uri.authority().to_smolstr();
                                #[cfg(all(target_family = "wasm", target_os = "unknown"))]
                                {
                                    tracing::debug!("Registering standalone entry blobs");
                                    let _ = crate::service_worker::register_standalone_entry_blobs(
                                        &ident(),
                                        rkey().as_str(),
                                        images,
                                        &fetcher,
                                    )
                                    .await;
                                }
                                for image in &images.images {
                                    let cid = image.image.blob().cid();
                                    cache_blob(
                                        ident_str.clone(),
                                        cid.to_smolstr(),
                                        image.name.as_ref().map(|n| n.to_smolstr()),
                                    )
                                    .await
                                    .ok();
                                }
                            }
                        }
                    }
                    let entry_json = serde_json::to_value(&data.entry).ok()?;
                    let entry_view_json = serde_json::to_value(&data.entry_view).ok()?;
                    let notebook_ctx_json = data
                        .notebook_context
                        .as_ref()
                        .map(|ctx| {
                            let notebook_json = serde_json::to_value(&ctx.notebook).ok()?;
                            let book_entry_json =
                                serde_json::to_value(&ctx.book_entry_view).ok()?;
                            Some((notebook_json, book_entry_json))
                        })
                        .flatten();
                    Some((entry_json, entry_view_json, notebook_ctx_json))
                }
                Ok(None) => None,
                Err(e) => {
                    tracing::error!("[use_standalone_entry_data] fetch error: {:?}", e);
                    None
                }
            }
        }
    }));

    let memo = use_memo(use_reactive!(|res| {
        use crate::fetch::{NotebookContext, StandaloneEntryData};
        use weaver_api::sh_weaver::notebook::{
            BookEntryView, EntryView, NotebookView, entry::Entry,
        };

        let res = res.as_ref().ok()?;
        let Some(Some((entry_json, entry_view_json, notebook_ctx_json))) = res.read().clone()
        else {
            return None;
        };

        let entry: Entry<'static> = jacquard::from_json_value::<Entry>(entry_json).ok()?;
        let entry_view: EntryView<'static> =
            jacquard::from_json_value::<EntryView>(entry_view_json).ok()?;
        let notebook_context = notebook_ctx_json
            .map(|(notebook_json, book_entry_json)| {
                let notebook: NotebookView<'static> =
                    jacquard::from_json_value::<NotebookView>(notebook_json).ok()?;
                let book_entry_view: BookEntryView<'static> =
                    jacquard::from_json_value::<BookEntryView>(book_entry_json).ok()?;
                Some(NotebookContext {
                    notebook,
                    book_entry_view,
                })
            })
            .flatten();

        Some(StandaloneEntryData {
            entry,
            entry_view,
            notebook_context,
        })
    }));

    (res, memo)
}

/// Fetches standalone entry data client-side only (no SSR)
#[cfg(not(feature = "fullstack-server"))]
pub fn use_standalone_entry_data(
    ident: ReadSignal<AtIdentifier<'static>>,
    rkey: ReadSignal<SmolStr>,
) -> (
    Resource<Option<crate::fetch::StandaloneEntryData>>,
    Memo<Option<crate::fetch::StandaloneEntryData>>,
) {
    let fetcher = use_context::<crate::fetch::Fetcher>();
    let res = use_resource(move || {
        let fetcher = fetcher.clone();
        async move {
            fetcher
                .get_entry_by_rkey(ident(), rkey())
                .await
                .ok()
                .flatten()
                .map(|arc| (*arc).clone())
        }
    });
    let memo = use_memo(move || res.read().clone().flatten());
    (res, memo)
}

/// Fetches notebook entry by rkey with SSR support.
#[cfg(feature = "fullstack-server")]
pub fn use_notebook_entry_by_rkey(
    ident: ReadSignal<AtIdentifier<'static>>,
    book_title: ReadSignal<SmolStr>,
    rkey: ReadSignal<SmolStr>,
) -> (
    Result<Resource<Option<(serde_json::Value, serde_json::Value)>>, RenderError>,
    Memo<Option<(BookEntryView<'static>, Entry<'static>)>>,
) {
    let fetcher = use_context::<crate::fetch::Fetcher>();
    let res = use_server_future(use_reactive!(|(ident, book_title, rkey)| {
        let fetcher = fetcher.clone();
        async move {
            match fetcher
                .get_notebook_entry_by_rkey(ident(), book_title(), rkey())
                .await
            {
                Ok(Some(data)) => {
                    let book_entry_json = serde_json::to_value(&data.0).ok()?;
                    let entry_json = serde_json::to_value(&data.1).ok()?;
                    Some((book_entry_json, entry_json))
                }
                Ok(None) => None,
                Err(e) => {
                    tracing::error!("[use_notebook_entry_by_rkey] fetch error: {:?}", e);
                    None
                }
            }
        }
    }));

    let memo = use_memo(use_reactive!(|res| {
        let res = res.as_ref().ok()?;
        if let Some(Some((book_entry_json, entry_json))) = &*res.read() {
            let book_entry: BookEntryView<'static> =
                jacquard::from_json_value::<BookEntryView>(book_entry_json.clone()).ok()?;
            let entry: Entry<'static> =
                jacquard::from_json_value::<Entry>(entry_json.clone()).ok()?;
            Some((book_entry, entry))
        } else {
            None
        }
    }));

    (res, memo)
}

/// Fetches notebook entry by rkey client-side only (no SSR)
#[cfg(not(feature = "fullstack-server"))]
pub fn use_notebook_entry_by_rkey(
    ident: ReadSignal<AtIdentifier<'static>>,
    book_title: ReadSignal<SmolStr>,
    rkey: ReadSignal<SmolStr>,
) -> (
    Resource<Option<(BookEntryView<'static>, Entry<'static>)>>,
    Memo<Option<(BookEntryView<'static>, Entry<'static>)>>,
) {
    let fetcher = use_context::<crate::fetch::Fetcher>();
    let res = use_resource(move || {
        let fetcher = fetcher.clone();
        async move {
            fetcher
                .get_notebook_entry_by_rkey(ident(), book_title(), rkey())
                .await
                .ok()
                .flatten()
                .map(|arc| (*arc).clone())
        }
    });
    let memo = use_memo(move || res.read().clone().flatten());
    (res, memo)
}

#[cfg(feature = "fullstack-server")]
#[put("/cache/{ident}/{cid}?name", cache: Extension<Arc<BlobCache>>)]
pub async fn cache_blob(ident: SmolStr, cid: SmolStr, name: Option<SmolStr>) -> Result<()> {
    let ident = AtIdentifier::new_owned(ident)?;
    let cid = Cid::new_owned(cid.as_bytes())?;
    cache.cache(ident, cid, name).await
}

/// Cache blob bytes directly (for pre-warming after upload).
/// If `notebook` is provided, uses scoped cache key `{notebook}_{name}`.
#[cfg(feature = "fullstack-server")]
#[put("/cache-bytes/{cid}?name&notebook", cache: Extension<Arc<BlobCache>>)]
pub async fn cache_blob_bytes(
    cid: SmolStr,
    name: Option<SmolStr>,
    notebook: Option<SmolStr>,
    body: jacquard::bytes::Bytes,
) -> Result<()> {
    let cid = Cid::new_owned(cid.as_bytes())?;
    let cache_key = match (&notebook, &name) {
        (Some(nb), Some(n)) => Some(format_smolstr!("{}_{}", nb, n)),
        (None, Some(n)) => Some(n.clone()),
        _ => None,
    };
    cache.insert_bytes(cid, body, cache_key);
    Ok(())
}
