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
use std::cell::Ref;
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
    ident: AtIdentifier<'static>,
    book_title: SmolStr,
    title: SmolStr,
) -> Result<Memo<Option<(BookEntryView<'static>, Entry<'static>)>>, RenderError> {
    let fetcher = use_context::<crate::fetch::Fetcher>();
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
    let fetcher = use_context::<crate::fetch::Fetcher>();
    let fetcher = fetcher.clone();
    let ident = use_signal(|| ident);
    let book_title = use_signal(|| book_title);
    let entry_title = use_signal(|| title);
    let res = use_resource(move || {
        let fetcher = fetcher.clone();
        async move {
            fetcher
                .get_entry(ident(), book_title(), entry_title())
                .await
                .ok()
                .flatten()
                .map(|arc| {
                    (
                        serde_json::to_value(entry.0.clone()).unwrap(),
                        serde_json::to_value(entry.1.clone()).unwrap(),
                    )
                })
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

pub fn get_handle(did: Did<'static>) -> AtIdentifier<'static> {
    let ident = AtIdentifier::Did(did);
    use_handle(ident.clone())
        .read()
        .as_ref()
        .unwrap_or(&Ok(ident))
        .as_ref()
        .unwrap()
        .clone()
}

pub fn use_handle(
    ident: AtIdentifier<'static>,
) -> Resource<Result<AtIdentifier<'static>, IdentityError>> {
    let fetcher = use_context::<crate::fetch::Fetcher>();
    let fetcher = fetcher.clone();
    let ident = use_signal(|| ident);

    use_resource(move || {
        let client = fetcher.get_client();
        async move {
            client
                .resolve_ident_owned(&*ident.read())
                .await
                .map(|doc| {
                    doc.handles()
                        .first()
                        .map(|h| AtIdentifier::Handle(h.clone()).into_static())
                })
                .map(|h| h.ok_or(IdentityError::invalid_well_known()))?
        }
    })
}

#[derive(Clone, PartialEq, Eq)]
pub struct NotebookHandle(pub Arc<Option<AtIdentifier<'static>>>);

impl NotebookHandle {
    pub fn as_ref(&self) -> Option<&AtIdentifier<'static>> {
        self.0.as_ref().as_ref()
    }
}

pub fn use_notebook_handle(ident: Signal<Option<AtIdentifier<'static>>>) -> NotebookHandle {
    let ident = if let Some(ident) = &*ident.read() {
        if let Some(Ok(handle)) = &*use_handle(ident.clone()).read() {
            Some(handle.clone())
        } else {
            Some(ident.clone())
        }
    } else {
        ident.read().clone()
    };
    use_context_provider(|| NotebookHandle(Arc::new(ident)))
}

/// Hook to render markdown client-side only (no SSR).
#[cfg(feature = "fullstack-server")]
pub fn use_rendered_markdown(
    content: Entry<'static>,
    ident: AtIdentifier<'static>,
) -> Result<Resource<Option<String>>, RenderError> {
    let ident = use_signal(|| ident);
    let content = use_signal(|| content);
    let fetcher = use_context::<crate::fetch::Fetcher>();
    Ok(use_server_future(move || {
        let client = fetcher.get_client();
        async move {
            let did = match ident() {
                AtIdentifier::Did(d) => d,
                AtIdentifier::Handle(h) => client.resolve_handle(&h).await.ok()?,
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
    let fetcher = use_context::<crate::fetch::Fetcher>();
    Ok(use_resource(move || {
        let client = fetcher.get_client();
        async move {
            let did = match ident() {
                AtIdentifier::Did(d) => d,
                AtIdentifier::Handle(h) => client.resolve_handle(&h).await.ok()?,
            };
            Some(render_markdown_impl(content(), did).await)
        }
    }))
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
    ident: AtIdentifier<'static>,
) -> Result<Memo<Option<ProfileDataView<'static>>>, RenderError> {
    let fetcher = use_context::<crate::fetch::Fetcher>();
    let ident = use_signal(|| ident);
    let res = use_server_future(move || {
        let fetcher = fetcher.clone();
        async move {
            fetcher
                .fetch_profile(&ident())
                .await
                .ok()
                .map(|arc| serde_json::to_value(&*arc).ok())
                .flatten()
        }
    })?;
    Ok(use_memo(move || {
        if let Some(Some(value)) = &*res.read_unchecked() {
            jacquard::from_json_value::<ProfileDataView>(value.clone()).ok()
        } else {
            None
        }
    }))
}

/// Fetches profile data client-side only (no SSR)
#[cfg(not(feature = "fullstack-server"))]
pub fn use_profile_data(
    ident: AtIdentifier<'static>,
) -> Result<Memo<Option<ProfileDataView<'static>>>, RenderError> {
    let fetcher = use_context::<crate::fetch::Fetcher>();
    let ident = use_signal(|| ident);
    let res = use_resource(move || {
        let fetcher = fetcher.clone();
        async move {
            fetcher
                .fetch_profile(&ident())
                .await
                .ok()
                .map(|arc| serde_json::to_value(&*arc).ok())
                .flatten()
        }
    });
    Ok(use_memo(move || {
        if let Some(Some(value)) = &*res.read_unchecked() {
            jacquard::from_json_value::<ProfileDataView>(value.clone()).ok()
        } else {
            None
        }
    }))
}

/// Fetches notebooks for a specific DID
#[cfg(feature = "fullstack-server")]
pub fn use_notebooks_for_did(
    ident: AtIdentifier<'static>,
) -> Result<Memo<Option<Vec<(NotebookView<'static>, Vec<StrongRef<'static>>)>>>, RenderError> {
    let fetcher = use_context::<crate::fetch::Fetcher>();
    let ident = use_signal(|| ident);
    let res = use_server_future(move || {
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
    })?;
    Ok(use_memo(move || {
        if let Some(Some(values)) = &*res.read_unchecked() {
            values
                .iter()
                .map(|v| {
                    jacquard::from_json_value::<(NotebookView, Vec<StrongRef>)>(v.clone()).ok()
                })
                .collect::<Option<Vec<_>>>()
        } else {
            None
        }
    }))
}

/// Fetches notebooks client-side only (no SSR)
#[cfg(not(feature = "fullstack-server"))]
pub fn use_notebooks_for_did(
    ident: AtIdentifier<'static>,
) -> Result<Memo<Option<Vec<(NotebookView<'static>, Vec<StrongRef<'static>>)>>>, RenderError> {
    let fetcher = use_context::<crate::fetch::Fetcher>();
    let ident = use_signal(|| ident);
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
                        .map(|arc| serde_json::to_value(arc.as_ref()).ok())
                        .collect::<Option<Vec<_>>>()
                })
                .flatten()
        }
    });
    Ok(use_memo(move || {
        if let Some(Some(values)) = &*res.read_unchecked() {
            values
                .iter()
                .map(|v| {
                    jacquard::from_json_value::<(NotebookView, Vec<StrongRef>)>(v.clone()).ok()
                })
                .collect::<Option<Vec<_>>>()
        } else {
            None
        }
    }))
}

/// Fetches notebooks from UFOS with SSR support in fullstack mode
#[cfg(feature = "fullstack-server")]
pub fn use_notebooks_from_ufos()
-> Result<Memo<Option<Vec<(NotebookView<'static>, Vec<StrongRef<'static>>)>>>, RenderError> {
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
    })?;
    Ok(use_memo(move || {
        if let Some(Some(values)) = &*res.read_unchecked() {
            values
                .iter()
                .map(|v| {
                    jacquard::from_json_value::<(NotebookView, Vec<StrongRef>)>(v.clone()).ok()
                })
                .collect::<Option<Vec<_>>>()
        } else {
            None
        }
    }))
}

/// Fetches notebooks from UFOS client-side only (no SSR)
#[cfg(not(feature = "fullstack-server"))]
pub fn use_notebooks_from_ufos()
-> Result<Memo<Option<Vec<(NotebookView<'static>, Vec<StrongRef<'static>>)>>>, RenderError> {
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
                        .map(|arc| serde_json::to_value(arc.as_ref()).ok())
                        .collect::<Option<Vec<_>>>()
                })
                .flatten()
        }
    });
    Ok(use_memo(move || {
        if let Some(Some(values)) = &*res.read_unchecked() {
            values
                .iter()
                .map(|v| {
                    jacquard::from_json_value::<(NotebookView, Vec<StrongRef>)>(v.clone()).ok()
                })
                .collect::<Option<Vec<_>>>()
        } else {
            None
        }
    }))
}

/// Fetches notebook metadata with SSR support in fullstack mode
#[cfg(feature = "fullstack-server")]
pub fn use_notebook(
    ident: AtIdentifier<'static>,
    book_title: SmolStr,
) -> Result<Memo<Option<(NotebookView<'static>, Vec<StrongRef<'static>>)>>, RenderError> {
    let fetcher = use_context::<crate::fetch::Fetcher>();
    let ident = use_signal(|| ident);
    let book_title = use_signal(|| book_title);
    let res = use_server_future(move || {
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
    })?;
    Ok(use_memo(move || {
        if let Some(Some(value)) = &*res.read_unchecked() {
            jacquard::from_json_value::<(NotebookView, Vec<StrongRef>)>(value.clone()).ok()
        } else {
            None
        }
    }))
}

/// Fetches notebook metadata client-side only (no SSR)
#[cfg(not(feature = "fullstack-server"))]
pub fn use_notebook(
    ident: AtIdentifier<'static>,
    book_title: SmolStr,
) -> Result<Memo<Option<(NotebookView<'static>, Vec<StrongRef<'static>>)>>, RenderError> {
    let fetcher = use_context::<crate::fetch::Fetcher>();
    let ident = use_signal(|| ident);
    let book_title = use_signal(|| book_title);
    let res = use_resource(move || {
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
    });
    Ok(use_memo(move || {
        if let Some(Some(value)) = &*res.read_unchecked() {
            jacquard::from_json_value::<(NotebookView, Vec<StrongRef>)>(value.clone()).ok()
        } else {
            None
        }
    }))
}

/// Fetches notebook entries with SSR support in fullstack mode
#[cfg(feature = "fullstack-server")]
pub fn use_notebook_entries(
    ident: AtIdentifier<'static>,
    book_title: SmolStr,
) -> Result<Memo<Option<Vec<BookEntryView<'static>>>>, RenderError> {
    let fetcher = use_context::<crate::fetch::Fetcher>();
    let ident = use_signal(|| ident);
    let book_title = use_signal(|| book_title);
    let res = use_server_future(move || {
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
    })?;
    Ok(use_memo(move || {
        if let Some(Some(values)) = &*res.read_unchecked() {
            values
                .iter()
                .map(|v| jacquard::from_json_value::<BookEntryView>(v.clone()).ok())
                .collect::<Option<Vec<_>>>()
        } else {
            None
        }
    }))
}

/// Fetches notebook entries client-side only (no SSR)
#[cfg(not(feature = "fullstack-server"))]
pub fn use_notebook_entries(
    ident: AtIdentifier<'static>,
    book_title: SmolStr,
) -> Result<Memo<Option<Vec<BookEntryView<'static>>>>, RenderError> {
    let fetcher = use_context::<crate::fetch::Fetcher>();
    let r = use_resource(use_reactive!(|(ident, book_title)| {
        let fetcher = fetcher.clone();
        async move {
            fetcher
                .list_notebook_entries(ident, book_title)
                .await
                .ok()
                .flatten()
        }
    }));
    Ok(use_memo(move || {
        r.read_unchecked().as_ref().and_then(|v| v.clone())
    }))
}

#[cfg(feature = "fullstack-server")]
#[put("/cache/{ident}/{cid}?name", cache: Extension<Arc<BlobCache>>)]
pub async fn cache_blob(ident: SmolStr, cid: SmolStr, name: Option<SmolStr>) -> Result<()> {
    let ident = AtIdentifier::new_owned(ident)?;
    let cid = Cid::new_owned(cid.as_bytes())?;
    cache.cache(ident, cid, name).await
}
