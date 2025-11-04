use crate::cache_impl;
use dioxus::Result;
use jacquard::{client::BasicClient, smol_str::SmolStr, types::ident::AtIdentifier};
use std::{sync::Arc, time::Duration};
use weaver_api::{
    com_atproto::repo::strong_ref::StrongRef,
    sh_weaver::notebook::{entry::Entry, BookEntryView, NotebookView},
};
use weaver_common::view::{entry_by_title, notebook_by_title};

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
                notebook_by_title(self.client.clone(), &ident, &title)
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
            if let Some(entry) = cache_impl::get(&self.entry_cache, &(ident.clone(), entry_title.clone())) {
                Ok(Some(entry))
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

    pub fn list_recent_entries(&self) -> Vec<Arc<(BookEntryView<'static>, Entry<'static>)>> {
        cache_impl::iter(&self.entry_cache)
    }

    pub fn list_recent_notebooks(
        &self,
    ) -> Vec<Arc<(NotebookView<'static>, Vec<StrongRef<'static>>)>> {
        cache_impl::iter(&self.book_cache)
    }

    pub async fn list_notebook_entries(
        &self,
        ident: AtIdentifier<'static>,
        book_title: SmolStr,
    ) -> Result<Option<Vec<BookEntryView<'static>>>> {
        use weaver_common::view::view_entry;

        if let Some(result) = self.get_notebook(ident.clone(), book_title).await? {
            let (notebook, entries) = result.as_ref();
            let mut book_entries = Vec::new();

            for index in 0..entries.len() {
                match view_entry(self.client.clone(), notebook, entries, index).await {
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
