use dioxus::{CapturedError, Result};
use jacquard::{client::BasicClient, smol_str::SmolStr, types::ident::AtIdentifier};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use weaver_api::{
    com_atproto::repo::strong_ref::StrongRef,
    sh_weaver::notebook::{entry::Entry, BookEntryView, NotebookView},
};
use weaver_common::view::{entry_by_title, fetch_entry_view, notebook_by_title, view_entry};

#[derive(Clone)]
pub struct CachedFetcher {
    pub client: Arc<BasicClient>,
    #[cfg(not(feature = "server"))]
    book_cache: Arc<
        Mutex<
            mini_moka::unsync::Cache<
                (AtIdentifier<'static>, SmolStr),
                Arc<(NotebookView<'static>, Vec<StrongRef<'static>>)>,
            >,
        >,
    >,
    #[cfg(not(feature = "server"))]
    entry_cache: Arc<
        Mutex<
            mini_moka::unsync::Cache<
                (AtIdentifier<'static>, SmolStr),
                Arc<(BookEntryView<'static>, Entry<'static>)>,
            >,
        >,
    >,
    #[cfg(feature = "server")]
    book_cache: Arc<
        Mutex<
            mini_moka::sync::Cache<
                (AtIdentifier<'static>, SmolStr),
                Arc<(NotebookView<'static>, Vec<StrongRef<'static>>)>,
            >,
        >,
    >,
    #[cfg(feature = "server")]
    entry_cache: Arc<
        Mutex<
            mini_moka::sync::Cache<
                (AtIdentifier<'static>, SmolStr),
                Arc<(BookEntryView<'static>, Entry<'static>)>,
            >,
        >,
    >,
}

impl CachedFetcher {
    #[cfg(not(feature = "server"))]
    pub fn new(client: Arc<BasicClient>) -> Self {
        let book_cache = mini_moka::unsync::Cache::builder()
            .max_capacity(100)
            .time_to_idle(Duration::from_secs(1200))
            .build();
        let entry_cache = mini_moka::unsync::Cache::builder()
            .max_capacity(100)
            .time_to_idle(Duration::from_secs(600))
            .build();

        Self {
            client,
            book_cache: Arc::new(Mutex::new(book_cache)),
            entry_cache: Arc::new(Mutex::new(entry_cache)),
        }
    }

    #[cfg(feature = "server")]
    pub fn new(client: Arc<BasicClient>) -> Self {
        let book_cache = mini_moka::sync::Cache::builder()
            .max_capacity(100)
            .time_to_idle(Duration::from_secs(1200))
            .build();
        let entry_cache = mini_moka::sync::Cache::builder()
            .max_capacity(100)
            .time_to_idle(Duration::from_secs(600))
            .build();

        Self {
            client,
            book_cache: Arc::new(Mutex::new(book_cache)),
            entry_cache: Arc::new(Mutex::new(entry_cache)),
        }
    }

    pub async fn get_notebook(
        &self,
        ident: AtIdentifier<'static>,
        title: SmolStr,
    ) -> Result<Option<Arc<(NotebookView<'static>, Vec<StrongRef<'static>>)>>> {
        if let Ok(mut book_cache) = self.book_cache.lock() {
            if let Some(entry) = book_cache.get(&(ident.clone(), title.clone())) {
                Ok(Some(entry.clone()))
            } else {
                if let Some((notebook, entries)) =
                    notebook_by_title(self.client.clone(), &ident, &title)
                        .await
                        .map_err(|e| dioxus::CapturedError::from_display(e))?
                {
                    let stored = Arc::new((notebook, entries));
                    book_cache.insert((ident.clone(), title.into()), stored.clone());
                    Ok(Some(stored))
                } else {
                    Ok(None)
                }
            }
        } else {
            Ok(None)
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
            if let Ok(mut entry_cache) = self.entry_cache.lock() {
                if let Some(entry) = entry_cache.get(&(ident.clone(), entry_title.clone())) {
                    Ok(Some(entry.clone()))
                } else {
                    if let Some(entry) = entry_by_title(
                        self.client.clone(),
                        notebook,
                        entries.as_ref(),
                        &entry_title,
                    )
                    .await
                    .map_err(|e| dioxus::CapturedError::from_display(e))?
                    {
                        let stored = Arc::new(entry);
                        entry_cache.insert((ident.clone(), entry_title.into()), stored.clone());
                        Ok(Some(stored))
                    } else {
                        Ok(None)
                    }
                }
            } else {
                Ok(None)
            }
        } else {
            Ok(None)
        }
    }

    #[cfg(not(feature = "server"))]
    pub fn list_recent_entries(&self) -> Vec<Arc<(BookEntryView<'static>, Entry<'static>)>> {
        if let Ok(entry_cache) = self.entry_cache.lock() {
            let mut entries = Vec::new();
            for (_, entry) in entry_cache.iter() {
                entries.push(entry.clone());
            }
            entries
        } else {
            Vec::new()
        }
    }

    #[cfg(not(feature = "server"))]
    pub fn list_recent_notebooks(
        &self,
    ) -> Vec<Arc<(NotebookView<'static>, Vec<StrongRef<'static>>)>> {
        if let Ok(book_cache) = self.book_cache.lock() {
            let mut entries = Vec::new();
            for (_, entry) in book_cache.iter() {
                entries.push(entry.clone());
            }
            entries
        } else {
            Vec::new()
        }
    }

    #[cfg(feature = "server")]
    pub fn list_recent_entries(&self) -> Vec<Arc<(BookEntryView<'static>, Entry<'static>)>> {
        if let Ok(entry_cache) = self.entry_cache.lock() {
            let mut entries = Vec::new();
            for entry in entry_cache.iter() {
                entries.push(entry.clone());
            }
            entries
        } else {
            Vec::new()
        }
    }

    #[cfg(feature = "server")]
    pub fn list_recent_notebooks(
        &self,
    ) -> Vec<Arc<(NotebookView<'static>, Vec<StrongRef<'static>>)>> {
        if let Ok(book_cache) = self.book_cache.lock() {
            let mut entries = Vec::new();
            for entry in book_cache.iter() {
                entries.push(entry.clone());
            }
            entries
        } else {
            Vec::new()
        }
    }
}
