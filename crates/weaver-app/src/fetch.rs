use crate::cache_impl;
use dioxus::Result;
use jacquard::prelude::*;
use jacquard::{client::BasicClient, smol_str::SmolStr, types::ident::AtIdentifier};
use serde::{Deserialize, Serialize};
use std::{sync::Arc, time::Duration};
use weaver_api::{
    com_atproto::repo::strong_ref::StrongRef,
    sh_weaver::notebook::{entry::Entry, BookEntryView, NotebookView},
};
use weaver_common::WeaverExt;

#[derive(Debug, Clone, Deserialize, Serialize)]
struct UfosRecord {
    collection: String,
    did: String,
    record: serde_json::Value,
    rkey: String,
    time_us: u64,
}

#[derive(Clone)]
pub struct CachedFetcher {
    pub client: Arc<BasicClient>,
    book_cache: cache_impl::Cache<
        (AtIdentifier<'static>, SmolStr),
        Arc<(NotebookView<'static>, Vec<StrongRef<'static>>)>,
    >,
    entry_cache: cache_impl::Cache<
        (AtIdentifier<'static>, SmolStr),
        Arc<(BookEntryView<'static>, Entry<'static>)>,
    >,
}

impl CachedFetcher {
    pub fn new(client: Arc<BasicClient>) -> Self {
        Self {
            client,
            book_cache: cache_impl::new_cache(100, Duration::from_secs(1200)),
            entry_cache: cache_impl::new_cache(100, Duration::from_secs(600)),
        }
    }

    pub async fn get_notebook(
        &self,
        ident: AtIdentifier<'static>,
        title: SmolStr,
    ) -> Result<Option<Arc<(NotebookView<'static>, Vec<StrongRef<'static>>)>>> {
        if let Some(entry) = cache_impl::get(&self.book_cache, &(ident.clone(), title.clone())) {
            Ok(Some(entry))
        } else {
            if let Some((notebook, entries)) =
                self.client
                    .notebook_by_title(&ident, &title)
                    .await
                    .map_err(|e| dioxus::CapturedError::from_display(e))?
            {
                let stored = Arc::new((notebook, entries));
                cache_impl::insert(&self.book_cache, (ident, title), stored.clone());
                Ok(Some(stored))
            } else {
                Ok(None)
            }
        }
    }

    pub async fn get_entry(
        &self,
        ident: AtIdentifier<'static>,
        book_title: SmolStr,
        entry_title: SmolStr,
    ) -> Result<Option<Arc<(BookEntryView<'static>, Entry<'static>)>>> {
        if let Some(result) = self.get_notebook(ident.clone(), book_title).await? {
            let (notebook, entries) = result.as_ref();
            if let Some(entry) =
                cache_impl::get(&self.entry_cache, &(ident.clone(), entry_title.clone()))
            {
                Ok(Some(entry))
            } else {
                if let Some(entry) = self
                    .client
                    .entry_by_title(notebook, entries.as_ref(), &entry_title)
                    .await
                    .map_err(|e| dioxus::CapturedError::from_display(e))?
                {
                    let stored = Arc::new(entry);
                    cache_impl::insert(&self.entry_cache, (ident, entry_title), stored.clone());
                    Ok(Some(stored))
                } else {
                    Ok(None)
                }
            }
        } else {
            Ok(None)
        }
    }

    pub async fn fetch_notebooks_from_ufos(
        &self,
    ) -> Result<Vec<Arc<(NotebookView<'static>, Vec<StrongRef<'static>>)>>> {
        use jacquard::{types::aturi::AtUri, IntoStatic};

        let url = "https://ufos-api.microcosm.blue/records?collection=sh.weaver.notebook.book";
        let response = reqwest::get(url)
            .await
            .map_err(|e| dioxus::CapturedError::from_display(e))?;

        let records: Vec<UfosRecord> = response
            .json()
            .await
            .map_err(|e| dioxus::CapturedError::from_display(e))?;

        let mut notebooks = Vec::new();

        for ufos_record in records {
            // Construct URI
            let uri_str = format!(
                "at://{}/{}/{}",
                ufos_record.did, ufos_record.collection, ufos_record.rkey
            );
            let uri = AtUri::new_owned(uri_str)
                .map_err(|e| dioxus::CapturedError::from_display(format!("Invalid URI: {}", e)))?;

            // Fetch the full notebook view (which hydrates authors)
            match self.client.view_notebook(&uri).await {
                Ok((notebook, entries)) => {
                    let ident = uri.authority().clone().into_static();
                    let title = notebook
                        .title
                        .as_ref()
                        .map(|t| SmolStr::new(t.as_ref()))
                        .unwrap_or_else(|| SmolStr::new("Untitled"));

                    let result = Arc::new((notebook, entries));
                    // Cache it
                    cache_impl::insert(&self.book_cache, (ident, title), result.clone());
                    notebooks.push(result);
                }
                Err(_) => continue, // Skip notebooks that fail to load
            }
        }

        Ok(notebooks)
    }

    pub async fn fetch_notebooks_for_did(
        &self,
        ident: &AtIdentifier<'_>,
    ) -> Result<Vec<Arc<(NotebookView<'static>, Vec<StrongRef<'static>>)>>> {
        use jacquard::{
            types::{collection::Collection, nsid::Nsid},
            xrpc::XrpcExt,
            IntoStatic,
        };
        use weaver_api::{
            com_atproto::repo::list_records::ListRecords, sh_weaver::notebook::book::Book,
        };

        // Resolve DID and PDS
        let (repo_did, pds_url) = match ident {
            AtIdentifier::Did(did) => {
                let pds = self
                    .client
                    .pds_for_did(did)
                    .await
                    .map_err(|e| dioxus::CapturedError::from_display(e))?;
                (did.clone(), pds)
            }
            AtIdentifier::Handle(handle) => self
                .client
                .pds_for_handle(handle)
                .await
                .map_err(|e| dioxus::CapturedError::from_display(e))?,
        };

        // Fetch all notebook records for this repo
        let resp = self
            .client
            .xrpc(pds_url)
            .send(
                &ListRecords::new()
                    .repo(repo_did)
                    .collection(Nsid::raw(Book::NSID))
                    .limit(100)
                    .build(),
            )
            .await
            .map_err(|e| dioxus::CapturedError::from_display(e))?;

        let mut notebooks = Vec::new();

        if let Ok(list) = resp.parse() {
            for record in list.records {
                // View the notebook (which hydrates authors)
                match self.client.view_notebook(&record.uri).await {
                    Ok((notebook, entries)) => {
                        let ident = record.uri.authority().clone().into_static();
                        let title = notebook
                            .title
                            .as_ref()
                            .map(|t| SmolStr::new(t.as_ref()))
                            .unwrap_or_else(|| SmolStr::new("Untitled"));

                        let result = Arc::new((notebook, entries));
                        // Cache it
                        cache_impl::insert(&self.book_cache, (ident, title), result.clone());
                        notebooks.push(result);
                    }
                    Err(_) => continue, // Skip notebooks that fail to load
                }
            }
        }

        Ok(notebooks)
    }

    pub async fn list_notebook_entries(
        &self,
        ident: AtIdentifier<'static>,
        book_title: SmolStr,
    ) -> Result<Option<Vec<BookEntryView<'static>>>> {
        if let Some(result) = self.get_notebook(ident.clone(), book_title).await? {
            let (notebook, entries) = result.as_ref();
            let mut book_entries = Vec::new();

            for index in 0..entries.len() {
                match self.client.view_entry(notebook, entries, index).await {
                    Ok(book_entry) => book_entries.push(book_entry),
                    Err(_) => continue, // Skip entries that fail to load
                }
            }

            Ok(Some(book_entries))
        } else {
            Ok(None)
        }
    }
}
