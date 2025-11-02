use std::sync::Arc;

use jacquard::{
    IntoStatic,
    client::{AgentSessionExt, BasicClient},
    from_data,
    prelude::IdentityResolver,
    to_data,
    types::{
        aturi::AtUri, collection::Collection, ident::AtIdentifier, nsid::Nsid,
        string::Datetime,
    },
    xrpc::XrpcExt,
};
use miette::{IntoDiagnostic, Result};
use weaver_api::{
    app_bsky::actor::profile::Profile as BskyProfile,
    com_atproto::repo::{list_records::ListRecords, strong_ref::StrongRef},
    sh_weaver::notebook::{book::Book, entry::Entry, page::Page},
};

// Re-export view types for use elsewhere
pub use weaver_api::sh_weaver::notebook::{
    AuthorListView, BookEntryRef, BookEntryView, EntryView, NotebookView,
};

pub async fn view_notebook(
    client: Arc<BasicClient>,
    uri: &AtUri<'_>,
) -> Result<(NotebookView<'static>, Vec<StrongRef<'static>>)> {
    let notebook = client.get_record::<Book>(uri).await?.into_output()?;

    let title = notebook.value.title.clone();
    let tags = notebook.value.tags.clone();

    let mut authors = Vec::new();

    for (index, author) in notebook.value.authors.iter().enumerate() {
        // TODO: swap to using weaver profiles here, or pick between them
        let author_uri =
            BskyProfile::uri(format!("at://{}/app.bsky.actor.profile/self", author.did))?;
        let author_profile = client.fetch_record(&author_uri).await?;

        authors.push(
            AuthorListView::new()
                .uri(author_uri.as_uri().clone())
                .record(to_data(&author_profile)?)
                .index(index as i64)
                .build(),
        );
    }
    let entries = notebook
        .value
        .entry_list
        .iter()
        .cloned()
        .map(IntoStatic::into_static)
        .collect();

    Ok((
        NotebookView::new()
            .cid(notebook.cid.unwrap())
            .uri(notebook.uri)
            .indexed_at(Datetime::now())
            .maybe_title(title)
            .maybe_tags(tags)
            .authors(authors)
            .record(to_data(&notebook.value)?)
            .build(),
        entries,
    ))
}

pub async fn fetch_entry_view<'a>(
    client: Arc<BasicClient>,
    notebook: &NotebookView<'a>,
    entry_ref: &StrongRef<'_>,
) -> Result<EntryView<'a>> {
    let entry = client
        .fetch_record(&Entry::uri(entry_ref.uri.clone())?)
        .await?;

    let title = entry.value.title.clone();
    let tags = entry.value.tags.clone();

    Ok(EntryView::new()
        .cid(entry.cid.unwrap())
        .uri(entry.uri)
        .indexed_at(Datetime::now())
        .record(to_data(&entry.value)?)
        .maybe_tags(tags)
        .title(title)
        .authors(notebook.authors.clone())
        .build())
}

pub async fn view_entry<'a>(
    client: Arc<BasicClient>,
    notebook: &NotebookView<'a>,
    entries: &[StrongRef<'_>],
    index: usize,
) -> Result<BookEntryView<'a>> {
    let entry_ref = entries
        .get(index)
        .ok_or(miette::miette!("entry out of bounds"))?;
    let entry = fetch_entry_view(client.clone(), notebook, entry_ref).await?;
    let prev_entry = if index > 0 {
        let prev_entry_ref = entries[index - 1].clone();
        fetch_entry_view(client.clone(), notebook, &prev_entry_ref)
            .await
            .ok()
    } else {
        None
    }
    .map(|e| BookEntryRef::new().entry(e).build());
    let next_entry = if index < entries.len() - 1 {
        let next_entry_ref = entries[index + 1].clone();
        fetch_entry_view(client.clone(), notebook, &next_entry_ref)
            .await
            .ok()
    } else {
        None
    }
    .map(|e| BookEntryRef::new().entry(e).build());
    Ok(BookEntryView::new()
        .entry(entry)
        .maybe_next(next_entry)
        .maybe_prev(prev_entry)
        .index(index as i64)
        .build())
}

pub async fn fetch_page_view<'a>(
    client: Arc<BasicClient>,
    notebook: &NotebookView<'a>,
    entry_ref: &StrongRef<'_>,
) -> Result<EntryView<'a>> {
    let entry = client
        .fetch_record(&Page::uri(entry_ref.uri.clone())?)
        .await?;

    let title = entry.value.title.clone();
    let tags = entry.value.tags.clone();

    Ok(EntryView::new()
        .cid(entry.cid.unwrap())
        .uri(entry.uri)
        .indexed_at(Datetime::now())
        .record(to_data(&entry.value)?)
        .maybe_tags(tags)
        .title(title)
        .authors(notebook.authors.clone())
        .build())
}

pub async fn view_page<'a>(
    client: Arc<BasicClient>,
    notebook: &NotebookView<'a>,
    pages: &[StrongRef<'_>],
    index: usize,
) -> Result<BookEntryView<'a>> {
    let entry_ref = pages
        .get(index)
        .ok_or(miette::miette!("entry out of bounds"))?;
    let entry = fetch_page_view(client.clone(), notebook, entry_ref).await?;
    let prev_entry = if index > 0 {
        let prev_entry_ref = pages[index - 1].clone();
        fetch_page_view(client.clone(), notebook, &prev_entry_ref)
            .await
            .ok()
    } else {
        None
    }
    .map(|e| BookEntryRef::new().entry(e).build());
    let next_entry = if index < pages.len() - 1 {
        let next_entry_ref = pages[index + 1].clone();
        fetch_page_view(client.clone(), notebook, &next_entry_ref)
            .await
            .ok()
    } else {
        None
    }
    .map(|e| BookEntryRef::new().entry(e).build());
    Ok(BookEntryView::new()
        .entry(entry)
        .maybe_next(next_entry)
        .maybe_prev(prev_entry)
        .index(index as i64)
        .build())
}

pub async fn entry_by_title<'a>(
    client: Arc<BasicClient>,
    notebook: &NotebookView<'a>,
    entries: &[StrongRef<'_>],
    title: &str,
) -> Result<Option<(BookEntryView<'a>, Entry<'a>)>> {
    for (index, entry_ref) in entries.iter().enumerate() {
        let resp = client.get_record::<Entry>(&entry_ref.uri).await?;
        if let Ok(entry) = resp.parse()
            && entry.value.title == title
        {
            return Ok(Some((
                view_entry(client.clone(), notebook, entries, index).await?,
                entry.value.into_static(),
            )));
        }
    }
    Ok(None)
}

pub async fn notebook_by_title<'a>(
    client: Arc<BasicClient>,
    ident: &AtIdentifier<'_>,
    title: &str,
) -> Result<Option<(NotebookView<'static>, Vec<StrongRef<'static>>)>> {
    let (repo_did, pds_url) = match ident {
        AtIdentifier::Did(did) => {
            let pds = client.pds_for_did(did).await?;
            (did.clone(), pds)
        }
        AtIdentifier::Handle(handle) => client.pds_for_handle(handle).await?,
    };
    // TODO: use the cursor to search through all records with this NSID for the repo
    let resp = client
        .xrpc(pds_url)
        .send(
            &ListRecords::new()
                .repo(repo_did)
                .collection(Nsid::raw(Book::NSID))
                .limit(100)
                .build(),
        )
        .await?;
    if let Ok(list) = resp.parse() {
        for record in list.records {
            let notebook: Book = from_data(&record.value).into_diagnostic()?;
            if let Some(book_title) = notebook.title
                && book_title == title
            {
                let tags = notebook.tags.clone();

                let mut authors = Vec::new();

                for (index, author) in notebook.authors.iter().enumerate() {
                    // TODO: swap to using weaver profiles here, or pick between them
                    let author_uri = BskyProfile::uri(format!(
                        "at://{}/app.bsky.actor.profile/self",
                        author.did
                    ))?;
                    let author_profile = client.fetch_record(&author_uri).await?;

                    authors.push(
                        AuthorListView::new()
                            .uri(author_uri.as_uri().clone())
                            .record(to_data(&author_profile)?)
                            .index(index as i64)
                            .build(),
                    );
                }
                let entries = notebook
                    .entry_list
                    .iter()
                    .cloned()
                    .map(IntoStatic::into_static)
                    .collect();

                return Ok(Some((
                    NotebookView::new()
                        .cid(record.cid)
                        .uri(record.uri)
                        .indexed_at(Datetime::now())
                        .title(book_title)
                        .maybe_tags(tags)
                        .authors(authors)
                        .record(record.value.clone())
                        .build()
                        .into_static(),
                    entries,
                )));
            }
        }
    }

    Ok(None)
}
