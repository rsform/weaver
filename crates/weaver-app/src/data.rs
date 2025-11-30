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
    types::{did::Did, string::Handle},
};
#[allow(unused_imports)]
use jacquard::{
    prelude::IdentityResolver,
    smol_str::SmolStr,
    types::{cid::Cid, string::AtIdentifier},
};
#[allow(unused_imports)]
use std::sync::Arc;
use weaver_api::com_atproto::repo::strong_ref::StrongRef;
use weaver_api::sh_weaver::actor::ProfileDataView;
use weaver_api::sh_weaver::notebook::{BookEntryView, NotebookView, entry::Entry};
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
            if let Some(entry) = fetcher
                .get_entry(ident(), book_title(), title())
                .await
                .ok()
                .flatten()
            {
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
            } else {
                None
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
                        #[cfg(all(
                            target_family = "wasm",
                            target_os = "unknown",
                            not(feature = "fullstack-server")
                        ))]
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
                Some(entry)
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
) -> Memo<Option<String>> {
    let fetcher = use_context::<crate::fetch::Fetcher>();
    let res = use_server_future(use_reactive!(|(content, ident)| {
        let client = fetcher.get_client();
        async move {
            let did = match ident.read().clone() {
                AtIdentifier::Did(d) => d,
                AtIdentifier::Handle(h) => client.resolve_handle(&h).await.ok()?,
            };
            Some(render_markdown_impl(content(), did).await)
        }
    }));
    use_memo(use_reactive!(|res| {
        let res = res.ok()?;
        if let Some(Some(value)) = &*res.read() {
            Some(value.clone())
        } else {
            None
        }
    }))
}

/// Hook to render markdown client-side only (no SSR).
#[cfg(not(feature = "fullstack-server"))]
pub fn use_rendered_markdown(
    content: ReadSignal<Entry<'static>>,
    ident: ReadSignal<AtIdentifier<'static>>,
) -> Memo<Option<String>> {
    let fetcher = use_context::<crate::fetch::Fetcher>();
    let res = use_resource(move || {
        let client = fetcher.get_client();
        async move {
            let did = match ident() {
                AtIdentifier::Did(d) => d,
                AtIdentifier::Handle(h) => client.resolve_handle(&h).await.ok()?,
            };
            Some(render_markdown_impl(content(), did).await)
        }
    });
    use_memo(move || {
        if let Some(Some(value)) = &*res.read() {
            Some(value.clone())
        } else {
            None
        }
    })
}

/// Internal implementation of markdown rendering.
async fn render_markdown_impl(content: Entry<'static>, did: Did<'static>) -> String {
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
    let _ = ClientWriter::<_, _, ()>::new(events.into_iter(), &mut html_buf).run();
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
    let res = use_resource(use_reactive!(|ident| {
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
        if let Some(Some(value)) = &*res.read() {
            jacquard::from_json_value::<ProfileDataView>(value.clone()).ok()
        } else {
            None
        }
    }));
    (Ok(res), memo)
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

#[cfg(feature = "fullstack-server")]
#[put("/cache/{ident}/{cid}?name", cache: Extension<Arc<BlobCache>>)]
pub async fn cache_blob(ident: SmolStr, cid: SmolStr, name: Option<SmolStr>) -> Result<()> {
    let ident = AtIdentifier::new_owned(ident)?;
    let cid = Cid::new_owned(cid.as_bytes())?;
    cache.cache(ident, cid, name).await
}
