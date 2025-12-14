// Re-export view types for use elsewhere
pub use weaver_api::sh_weaver::notebook::{
    AuthorListView, BookEntryRef, BookEntryView, EntryView, NotebookView, PermissionGrant,
    PermissionsState,
};

// Re-export jacquard for convenience
use crate::constellation::{GetBacklinksQuery, RecordId};
use crate::error::WeaverError;
pub use jacquard;
use jacquard::bytes::Bytes;
use jacquard::client::{AgentError, AgentErrorKind, AgentSession, AgentSessionExt};
use jacquard::error::ClientError;
use jacquard::prelude::*;
use jacquard::smol_str::SmolStr;
use jacquard::types::blob::{BlobRef, MimeType};
use jacquard::types::string::{AtUri, Did, RecordKey, Rkey};
use jacquard::types::tid::Tid;
use jacquard::types::uri::Uri;
use jacquard::url::Url;
use jacquard::{CowStr, IntoStatic};
use mime_sniffer::MimeTypeSniffer;
use std::path::Path;
use weaver_api::com_atproto::repo::strong_ref::StrongRef;
use weaver_api::sh_weaver::notebook::entry;
use weaver_api::sh_weaver::publish::blob::Blob as PublishedBlob;

use crate::{PublishResult, W_TICKER, normalize_title_path};

const CONSTELLATION_URL: &str = "https://constellation.microcosm.blue";

/// Strip trailing punctuation that URL parsers commonly eat
/// (period, comma, semicolon, colon, exclamation, question mark)
fn strip_trailing_punctuation(s: &str) -> &str {
    s.trim_end_matches(['.', ',', ';', ':', '!', '?'])
}

/// Check if a search term matches a value, with fallback to stripped punctuation
fn title_matches(value: &str, search: &str) -> bool {
    // Exact match first
    if value == search {
        return true;
    }
    // Try with trailing punctuation stripped from search term
    let stripped_search = strip_trailing_punctuation(search);
    if stripped_search != search && value == stripped_search {
        return true;
    }
    // Try with trailing punctuation stripped from value (for titles ending in punctuation)
    let stripped_value = strip_trailing_punctuation(value);
    if stripped_value != value && stripped_value == search {
        return true;
    }
    false
}

/// Extension trait providing weaver-specific multi-step operations on Agent
///
/// This trait extends jacquard's Agent with notebook-specific workflows that
/// involve multiple atproto operations (uploading blobs, creating records, etc.)
///
/// For single-step operations, use jacquard's built-in methods directly:
/// - `agent.create_record()` - Create a single record
/// - `agent.get_record()` - Get a single record
/// - `agent.upload_blob()` - Upload a single blob
///
/// This trait is for multi-step workflows that coordinate between multiple operations.
pub trait WeaverExt: AgentSessionExt + XrpcExt + Send + Sync + Sized {
    /// Publish a blob to the user's PDS
    ///
    /// Multi-step workflow:
    /// 1. Upload blob to PDS
    /// 2. Create blob record with CID
    ///
    /// Returns the AT-URI of the published blob
    fn publish_blob<'a>(
        &'a self,
        blob: Bytes,
        url_path: &'a str,
        rkey: Option<RecordKey<Rkey<'a>>>,
    ) -> impl Future<Output = Result<(StrongRef<'a>, PublishedBlob<'a>), WeaverError>> + 'a {
        async move {
            let mime_type =
                MimeType::new_owned(blob.sniff_mime_type().unwrap_or("application/octet-stream"));

            let blob = self.upload_blob(blob, mime_type.into_static()).await?;
            let publish_record = PublishedBlob::new()
                .path(url_path)
                .upload(BlobRef::Blob(blob))
                .build();
            let record_key = match rkey {
                Some(key) => key,
                None => {
                    let tid = W_TICKER.lock().await.next(None);
                    RecordKey(Rkey::new_owned(tid.as_str())?)
                }
            };
            let record = self
                .create_record(publish_record.clone(), Some(record_key))
                .await?;
            let strong_ref = StrongRef::new().uri(record.uri).cid(record.cid).build();

            Ok((strong_ref, publish_record))
        }
    }

    fn confirm_record_ref<'a>(
        &'a self,
        uri: &'a AtUri<'a>,
    ) -> impl Future<Output = Result<StrongRef<'static>, WeaverError>> + 'a {
        async move {
            let record = self.fetch_record_slingshot(uri).await?;
            let cid = record.cid.ok_or_else(|| {
                AgentError::from(ClientError::invalid_request("Record missing CID"))
            })?;
            Ok(StrongRef::new()
                .uri(record.uri.into_static())
                .cid(cid.into_static())
                .build())
        }
    }

    /// Find or create a notebook by title, returning its URI and entry list
    ///
    /// If the notebook doesn't exist, creates it with the given DID as author.
    fn upsert_notebook(
        &self,
        title: &str,
        author_did: &Did<'_>,
    ) -> impl Future<Output = Result<(AtUri<'static>, Vec<StrongRef<'static>>), WeaverError>>
    where
        Self: Sized,
    {
        async move {
            use jacquard::types::collection::Collection;
            use jacquard::types::nsid::Nsid;
            use weaver_api::com_atproto::repo::list_records::ListRecords;
            use weaver_api::sh_weaver::notebook::book::Book;

            // Find the PDS for this DID
            let pds_url = self.pds_for_did(author_did).await.map_err(|e| {
                AgentError::from(ClientError::from(e).with_context("Failed to resolve PDS for DID"))
            })?;

            // Search for existing notebook with this title (paginated)
            let mut cursor: Option<CowStr<'static>> = None;
            loop {
                let resp = self
                    .xrpc(pds_url.clone())
                    .send(
                        &ListRecords::new()
                            .repo(author_did.clone())
                            .collection(Nsid::raw(Book::NSID))
                            .limit(100)
                            .maybe_cursor(cursor.clone())
                            .build(),
                    )
                    .await
                    .map_err(|e| AgentError::from(ClientError::from(e)))?;

                let list = match resp.parse() {
                    Ok(l) => l,
                    Err(_) => break, // Parse error, stop searching
                };

                for record in list.records {
                    let notebook: Book = jacquard::from_data(&record.value).map_err(|_| {
                        AgentError::from(ClientError::invalid_request(
                            "Failed to parse notebook record",
                        ))
                    })?;
                    if let Some(book_title) = notebook.title
                        && book_title == title
                    {
                        let entries = notebook
                            .entry_list
                            .iter()
                            .cloned()
                            .map(IntoStatic::into_static)
                            .collect();
                        return Ok((record.uri.into_static(), entries));
                    }
                }

                match list.cursor {
                    Some(c) => cursor = Some(c.into_static()),
                    None => break, // No more pages
                }
            }

            // Notebook doesn't exist, create it
            use weaver_api::sh_weaver::actor::Author;
            let path = normalize_title_path(title);
            let author = Author::new().did(author_did.clone()).build();
            let book = Book::new()
                .authors(vec![author])
                .entry_list(vec![])
                .maybe_title(Some(title.into()))
                .maybe_path(Some(path.into()))
                .maybe_created_at(Some(jacquard::types::string::Datetime::now()))
                .build();

            let response = self.create_record(book, None).await?;
            Ok((response.uri, Vec::new()))
        }
    }

    /// Find or create an entry within a notebook
    ///
    /// Multi-step workflow:
    /// 1. Find the notebook by title
    /// 2. If existing_rkey is provided, match by rkey; otherwise match by title
    /// 3. If found: update the entry with new content
    /// 4. If not found: create new entry and append to notebook's entry_list
    ///
    /// The `existing_rkey` parameter allows updating an entry even if its title changed,
    /// and enables pre-generating rkeys for path rewriting before publish.
    ///
    /// Returns (entry_ref, was_created)
    fn upsert_entry(
        &self,
        notebook_title: &str,
        entry_title: &str,
        entry: entry::Entry<'_>,
        existing_rkey: Option<&str>,
    ) -> impl Future<Output = Result<(StrongRef<'static>, bool), WeaverError>>
    where
        Self: Sized,
    {
        async move {
            // Get our own DID
            let (did, _) = self.session_info().await.ok_or_else(|| {
                AgentError::from(ClientError::invalid_request("No session info available"))
            })?;

            // Find or create notebook
            let (notebook_uri, entry_refs) = self.upsert_notebook(notebook_title, &did).await?;

            // If we have an existing rkey, try to find and update that specific entry
            if let Some(rkey) = existing_rkey {
                // Check if this entry exists in the notebook by comparing rkeys
                for entry_ref in &entry_refs {
                    let ref_rkey = entry_ref.uri.rkey().map(|r| r.0.as_str());
                    if ref_rkey == Some(rkey) {
                        // Found it - update
                        let output = self
                            .update_record::<entry::Entry>(&entry_ref.uri, |e| {
                                e.content = entry.content.clone();
                                e.title = entry.title.clone();
                                e.path = entry.path.clone();
                                e.embeds = entry.embeds.clone();
                                e.tags = entry.tags.clone();
                            })
                            .await?;
                        let updated_ref = StrongRef::new()
                            .uri(output.uri.into_static())
                            .cid(output.cid.into_static())
                            .build();
                        return Ok((updated_ref, false));
                    }
                }

                // Entry with this rkey not in notebook - create with specific rkey
                let response = self
                    .create_record(entry, Some(RecordKey::any(rkey)?))
                    .await?;
                let new_ref = StrongRef::new()
                    .uri(response.uri.clone().into_static())
                    .cid(response.cid.clone().into_static())
                    .build();

                use weaver_api::sh_weaver::notebook::book::Book;
                let notebook_entry_ref = StrongRef::new()
                    .uri(response.uri.into_static())
                    .cid(response.cid.into_static())
                    .build();

                self.update_record::<Book>(&notebook_uri, |book| {
                    book.entry_list.push(notebook_entry_ref);
                })
                .await?;

                return Ok((new_ref, true));
            }

            // No existing rkey - use title-based matching (original behavior)

            // Fast path: if notebook is empty, skip search and create directly
            if entry_refs.is_empty() {
                let response = self.create_record(entry, None).await?;
                let new_ref = StrongRef::new()
                    .uri(response.uri.clone().into_static())
                    .cid(response.cid.clone().into_static())
                    .build();

                use weaver_api::sh_weaver::notebook::book::Book;
                let notebook_entry_ref = StrongRef::new()
                    .uri(response.uri.into_static())
                    .cid(response.cid.into_static())
                    .build();

                self.update_record::<Book>(&notebook_uri, |book| {
                    book.entry_list.push(notebook_entry_ref);
                })
                .await?;

                return Ok((new_ref, true));
            }

            // Check if entry with this title exists in the notebook
            // O(n) network calls - unavoidable without title indexing
            for entry_ref in &entry_refs {
                let existing = self
                    .get_record::<entry::Entry>(&entry_ref.uri)
                    .await
                    .map_err(|e| AgentError::from(ClientError::from(e)))?;
                if let Ok(existing_entry) = existing.parse() {
                    if existing_entry.value.title == entry_title {
                        // Update existing entry
                        let output = self
                            .update_record::<entry::Entry>(&entry_ref.uri, |e| {
                                e.content = entry.content.clone();
                                e.embeds = entry.embeds.clone();
                                e.tags = entry.tags.clone();
                            })
                            .await?;
                        let updated_ref = StrongRef::new()
                            .uri(output.uri.into_static())
                            .cid(output.cid.into_static())
                            .build();
                        return Ok((updated_ref, false));
                    }
                }
            }

            // Entry doesn't exist, create it
            let response = self.create_record(entry, None).await?;
            let new_ref = StrongRef::new()
                .uri(response.uri.clone().into_static())
                .cid(response.cid.clone().into_static())
                .build();

            // Add to notebook's entry_list
            use weaver_api::sh_weaver::notebook::book::Book;
            let notebook_entry_ref = StrongRef::new()
                .uri(response.uri.into_static())
                .cid(response.cid.into_static())
                .build();

            self.update_record::<Book>(&notebook_uri, |book| {
                book.entry_list.push(notebook_entry_ref);
            })
            .await?;

            Ok((new_ref, true))
        }
    }

    /// View functions - generic versions that work with any Agent

    /// Fetch a notebook and construct NotebookView with author profiles
    #[cfg(feature = "use-index")]
    fn view_notebook(
        &self,
        uri: &AtUri<'_>,
    ) -> impl Future<Output = Result<(NotebookView<'static>, Vec<StrongRef<'static>>), WeaverError>>
    where
        Self: Sized,
    {
        async move {
            use weaver_api::sh_weaver::notebook::get_notebook::GetNotebook;

            let resp = self
                .send(GetNotebook::new().notebook(uri.clone()).build())
                .await
                .map_err(|e| AgentError::from(ClientError::from(e)))?;

            let output = resp.into_output().map_err(|e| {
                AgentError::from(ClientError::invalid_request(format!(
                    "Failed to get notebook: {}",
                    e
                )))
            })?;

            Ok((
                output.notebook.into_static(),
                output
                    .entries
                    .into_iter()
                    .map(IntoStatic::into_static)
                    .collect(),
            ))
        }
    }

    #[cfg(not(feature = "use-index"))]
    fn view_notebook(
        &self,
        uri: &AtUri<'_>,
    ) -> impl Future<Output = Result<(NotebookView<'static>, Vec<StrongRef<'static>>), WeaverError>>
    where
        Self: Sized,
    {
        async move {
            use jacquard::to_data;
            use weaver_api::sh_weaver::notebook::AuthorListView;
            use weaver_api::sh_weaver::notebook::book::Book;

            let notebook = self
                .get_record::<Book>(uri)
                .await
                .map_err(|e| AgentError::from(e))?
                .into_output()
                .map_err(|_| {
                    AgentError::from(ClientError::invalid_request("Failed to parse Book record"))
                })?;

            let title = notebook.value.title.clone();
            let tags = notebook.value.tags.clone();
            let path = notebook.value.path.clone();

            let mut authors = Vec::new();
            use weaver_api::app_bsky::actor::{
                ProfileViewDetailed, get_profile::GetProfile, profile::Profile as BskyProfile,
            };
            use weaver_api::sh_weaver::actor::{
                ProfileDataView, ProfileDataViewInner, ProfileView,
                profile::Profile as WeaverProfile,
            };

            for (index, author) in notebook.value.authors.iter().enumerate() {
                let (profile_uri, profile_view) = self.hydrate_profile_view(&author.did).await?;
                authors.push(
                    AuthorListView::new()
                        .maybe_uri(profile_uri)
                        .record(profile_view)
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

            // Fetch permissions for this notebook
            let permissions = self.get_permissions_for_resource(uri).await?;

            Ok((
                NotebookView::new()
                    .cid(notebook.cid.ok_or_else(|| {
                        AgentError::from(ClientError::invalid_request("Notebook missing CID"))
                    })?)
                    .uri(notebook.uri)
                    .indexed_at(jacquard::types::string::Datetime::now())
                    .maybe_title(title)
                    .maybe_path(path)
                    .maybe_tags(tags)
                    .authors(authors)
                    .permissions(permissions)
                    .record(to_data(&notebook.value).map_err(|_| {
                        AgentError::from(ClientError::invalid_request(
                            "Failed to serialize notebook",
                        ))
                    })?)
                    .build(),
                entries,
            ))
        }
    }

    /// Fetch an entry and construct EntryView
    fn fetch_entry_view<'a>(
        &self,
        notebook: &NotebookView<'a>,
        entry_ref: &StrongRef<'_>,
    ) -> impl Future<Output = Result<EntryView<'a>, WeaverError>>
    where
        Self: Sized,
    {
        async move {
            use jacquard::to_data;
            use weaver_api::sh_weaver::notebook::entry::Entry;

            let entry_uri = Entry::uri(entry_ref.uri.clone())
                .map_err(|_| AgentError::from(ClientError::invalid_request("Invalid entry URI")))?;

            // Get the rkey for version lookup
            let rkey = entry_uri.rkey().ok_or_else(|| {
                AgentError::from(ClientError::invalid_request("Entry URI missing rkey"))
            })?;

            // Fetch permissions for this entry (includes inherited notebook permissions)
            let permissions = self.get_permissions_for_resource(&entry_uri).await?;

            // Get all collaborators (owner + invited)
            let owner_did = match entry_uri.authority() {
                jacquard::types::ident::AtIdentifier::Did(d) => d.clone().into_static(),
                jacquard::types::ident::AtIdentifier::Handle(h) => {
                    let (did, _) = self.pds_for_handle(h).await.map_err(|e| {
                        AgentError::from(
                            ClientError::from(e).with_context("Failed to resolve handle"),
                        )
                    })?;
                    did.into_static()
                }
            };
            let collaborators = self
                .find_collaborators_for_resource(&entry_uri)
                .await
                .unwrap_or_default();
            let all_dids: Vec<Did<'static>> = std::iter::once(owner_did)
                .chain(collaborators.into_iter())
                .collect();

            // Find all versions across collaborators, get latest by updatedAt
            let versions = self
                .find_all_versions(
                    <Entry as jacquard::types::collection::Collection>::NSID,
                    rkey.0.as_str(),
                    &all_dids,
                )
                .await
                .unwrap_or_default();

            // Use latest version if found, otherwise fall back to original entry_ref
            let (entry_data, final_uri, final_cid) = if let Some(latest) = versions.first() {
                // Deserialize from the latest version's value
                let entry: Entry = jacquard::from_data(&latest.value).map_err(|_| {
                    AgentError::from(ClientError::invalid_request(
                        "Failed to deserialize latest entry",
                    ))
                })?;
                (entry.into_static(), latest.uri.clone(), latest.cid.clone())
            } else {
                // No versions found via find_all_versions, fetch directly
                let entry = self.fetch_record(&entry_uri).await?;
                let cid = entry.cid.ok_or_else(|| {
                    AgentError::from(ClientError::invalid_request("Entry missing CID"))
                })?;
                (
                    entry.value.into_static(),
                    entry.uri.into_static(),
                    cid.into_static(),
                )
            };

            let title = entry_data.title.clone();
            let path = entry_data.path.clone();
            let tags = entry_data.tags.clone();

            // Fetch contributors (evidence-based authors) for this entry
            let contributor_dids = self.find_contributors_for_resource(&entry_uri).await?;
            let mut authors = Vec::new();
            for (index, did) in contributor_dids.iter().enumerate() {
                let (profile_uri, profile_view) = self.hydrate_profile_view(did).await?;
                authors.push(
                    AuthorListView::new()
                        .maybe_uri(profile_uri)
                        .record(profile_view)
                        .index(index as i64)
                        .build(),
                );
            }

            Ok(EntryView::new()
                .cid(final_cid)
                .uri(final_uri)
                .indexed_at(jacquard::types::string::Datetime::now())
                .record(to_data(&entry_data).map_err(|_| {
                    AgentError::from(ClientError::invalid_request("Failed to serialize entry"))
                })?)
                .maybe_tags(tags)
                .title(title)
                .path(path)
                .authors(authors)
                .permissions(permissions)
                .build())
        }
    }

    /// Search for an entry by title within a notebook's entry list
    ///
    /// O(n) network calls - unavoidable without title indexing.
    /// Breaks early on match to minimize unnecessary fetches.
    fn entry_by_title<'a>(
        &self,
        notebook: &NotebookView<'a>,
        entries: &[StrongRef<'_>],
        title: &str,
    ) -> impl Future<Output = Result<Option<(BookEntryView<'a>, entry::Entry<'a>)>, WeaverError>>
    where
        Self: Sized,
    {
        async move {
            use weaver_api::sh_weaver::notebook::BookEntryRef;
            use weaver_api::sh_weaver::notebook::entry::Entry;

            for (index, entry_ref) in entries.iter().enumerate() {
                let resp = self
                    .get_record::<Entry>(&entry_ref.uri)
                    .await
                    .map_err(|e| AgentError::from(e))?;
                if let Ok(entry) = resp.parse() {
                    let path_matches = title_matches(entry.value.path.as_ref(), title);
                    let title_field_matches = title_matches(entry.value.title.as_ref(), title);
                    if path_matches || title_field_matches {
                        // Build BookEntryView with prev/next
                        let entry_view = self.fetch_entry_view(notebook, entry_ref).await?;

                        let prev_entry = if index > 0 {
                            let prev_entry_ref = &entries[index - 1];
                            self.fetch_entry_view(notebook, prev_entry_ref).await.ok()
                        } else {
                            None
                        }
                        .map(|e| BookEntryRef::new().entry(e).build());

                        let next_entry = if index < entries.len() - 1 {
                            let next_entry_ref = &entries[index + 1];
                            self.fetch_entry_view(notebook, next_entry_ref).await.ok()
                        } else {
                            None
                        }
                        .map(|e| BookEntryRef::new().entry(e).build());

                        let book_entry_view = BookEntryView::new()
                            .entry(entry_view)
                            .maybe_next(next_entry)
                            .maybe_prev(prev_entry)
                            .index(index as i64)
                            .build();

                        return Ok(Some((book_entry_view, entry.value.into_static())));
                    }
                }
            }
            Ok(None)
        }
    }

    /// Search for a notebook by title for a given DID or handle
    #[cfg(feature = "use-index")]
    fn notebook_by_title(
        &self,
        ident: &jacquard::types::ident::AtIdentifier<'_>,
        title: &str,
    ) -> impl Future<
        Output = Result<Option<(NotebookView<'static>, Vec<StrongRef<'static>>)>, WeaverError>,
    >
    where
        Self: Sized,
    {
        async move {
            use weaver_api::sh_weaver::notebook::resolve_notebook::ResolveNotebook;

            let resp = self
                .send(
                    ResolveNotebook::new()
                        .actor(ident.clone())
                        .name(title)
                        .build(),
                )
                .await
                .map_err(|e| AgentError::from(ClientError::from(e)))?;

            match resp.into_output() {
                Ok(output) => {
                    // Extract StrongRefs from the BookEntryViews for compatibility
                    let entries: Vec<StrongRef<'static>> = output
                        .entries
                        .iter()
                        .map(|bev| {
                            StrongRef::new()
                                .uri(bev.entry.uri.clone())
                                .cid(bev.entry.cid.clone())
                                .build()
                                .into_static()
                        })
                        .collect();

                    Ok(Some((output.notebook.into_static(), entries)))
                }
                Err(_) => Ok(None),
            }
        }
    }

    /// Search for a notebook by title for a given DID or handle
    #[cfg(not(feature = "use-index"))]
    fn notebook_by_title(
        &self,
        ident: &jacquard::types::ident::AtIdentifier<'_>,
        title: &str,
    ) -> impl Future<
        Output = Result<Option<(NotebookView<'static>, Vec<StrongRef<'static>>)>, WeaverError>,
    >
    where
        Self: Sized,
    {
        async move {
            use jacquard::types::collection::Collection;
            use jacquard::types::nsid::Nsid;
            use weaver_api::com_atproto::repo::list_records::ListRecords;
            use weaver_api::sh_weaver::notebook::AuthorListView;
            use weaver_api::sh_weaver::notebook::book::Book;

            let (repo_did, pds_url) = match ident {
                jacquard::types::ident::AtIdentifier::Did(did) => {
                    let pds = self.pds_for_did(did).await.map_err(|e| {
                        AgentError::from(
                            ClientError::from(e).with_context("Failed to resolve PDS for DID"),
                        )
                    })?;
                    (did.clone(), pds)
                }
                jacquard::types::ident::AtIdentifier::Handle(handle) => {
                    self.pds_for_handle(handle).await.map_err(|e| {
                        AgentError::from(
                            ClientError::from(e).with_context("Failed to resolve handle"),
                        )
                    })?
                }
            };

            // Search with pagination
            let mut cursor: Option<CowStr<'static>> = None;
            loop {
                let resp = self
                    .xrpc(pds_url.clone())
                    .send(
                        &ListRecords::new()
                            .repo(repo_did.clone())
                            .collection(Nsid::raw(Book::NSID))
                            .limit(100)
                            .maybe_cursor(cursor.clone())
                            .build(),
                    )
                    .await
                    .map_err(|e| AgentError::from(ClientError::from(e)))?;

                let list = match resp.parse() {
                    Ok(l) => l,
                    Err(_) => break,
                };

                for record in list.records {
                    let notebook: Book = jacquard::from_data(&record.value).map_err(|_| {
                        AgentError::from(ClientError::invalid_request(
                            "Failed to parse notebook record",
                        ))
                    })?;

                    // Match on path first, then title (with trailing punctuation tolerance)
                    let matched_title = if let Some(ref path) = notebook.path
                        && title_matches(path.as_ref(), title)
                    {
                        Some(path.clone())
                    } else if let Some(ref book_title) = notebook.title
                        && title_matches(book_title.as_ref(), title)
                    {
                        Some(book_title.clone())
                    } else {
                        None
                    };

                    if let Some(matched) = matched_title {
                        let tags = notebook.tags.clone();
                        let path = notebook.path.clone();

                        let mut authors = Vec::new();
                        for (index, author) in notebook.authors.iter().enumerate() {
                            let (profile_uri, profile_view) =
                                self.hydrate_profile_view(&author.did).await?;
                            authors.push(
                                AuthorListView::new()
                                    .maybe_uri(profile_uri)
                                    .record(profile_view)
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

                        // Fetch permissions for this notebook
                        let permissions = self.get_permissions_for_resource(&record.uri).await?;

                        return Ok(Some((
                            NotebookView::new()
                                .cid(record.cid)
                                .uri(record.uri)
                                .indexed_at(jacquard::types::string::Datetime::now())
                                .title(matched)
                                .maybe_path(path)
                                .maybe_tags(tags)
                                .authors(authors)
                                .permissions(permissions)
                                .record(record.value.clone())
                                .build()
                                .into_static(),
                            entries,
                        )));
                    }
                }

                match list.cursor {
                    Some(c) => cursor = Some(c.into_static()),
                    None => break,
                }
            }

            Ok(None)
        }
    }

    /// Hydrate a profile view from either weaver or bsky profile
    #[cfg(feature = "use-index")]
    fn hydrate_profile_view(
        &self,
        did: &Did<'_>,
    ) -> impl Future<
        Output = Result<
            (
                Option<AtUri<'static>>,
                weaver_api::sh_weaver::actor::ProfileDataView<'static>,
            ),
            WeaverError,
        >,
    > {
        async move {
            use weaver_api::sh_weaver::actor::get_profile::GetProfile;

            let resp = self
                .send(GetProfile::new().actor(did.clone()).build())
                .await
                .map_err(|e| AgentError::from(ClientError::from(e)))?;

            let output = resp.into_output().map_err(|e| {
                AgentError::from(ClientError::invalid_request(format!(
                    "Failed to get profile: {}",
                    e
                )))
            })?;

            // URI is goofy in this signature, just return None for now
            Ok((None, output.value.into_static()))
        }
    }

    /// Hydrate a profile view from either weaver or bsky profile
    #[cfg(not(feature = "use-index"))]
    fn hydrate_profile_view(
        &self,
        did: &Did<'_>,
    ) -> impl Future<
        Output = Result<
            (
                Option<AtUri<'static>>,
                weaver_api::sh_weaver::actor::ProfileDataView<'static>,
            ),
            WeaverError,
        >,
    > {
        async move {
            use weaver_api::app_bsky::actor::{
                ProfileViewDetailed, get_profile::GetProfile, profile::Profile as BskyProfile,
            };
            use weaver_api::sh_weaver::actor::{
                ProfileDataView, ProfileDataViewInner, ProfileView,
                profile::Profile as WeaverProfile,
            };

            let handles = self.resolve_did_doc_owned(&did).await?.handles();
            let handle = handles.first().ok_or_else(|| {
                AgentError::from(ClientError::invalid_request("couldn't resolve handle"))
            })?;

            // Try weaver profile first
            let weaver_uri =
                WeaverProfile::uri(format!("at://{}/sh.weaver.actor.profile/self", did)).map_err(
                    |_| {
                        AgentError::from(ClientError::invalid_request("Invalid weaver profile URI"))
                    },
                )?;
            let weaver_future = async {
                if let Ok(weaver_record) = self.fetch_record(&weaver_uri).await {
                    // Convert blobs to CDN URLs
                    let avatar = weaver_record
                        .value
                        .avatar
                        .as_ref()
                        .map(|blob| {
                            let cid = blob.blob().cid();
                            jacquard::types::string::Uri::new_owned(format!(
                                "https://cdn.bsky.app/img/avatar/plain/{}/{}@jpeg",
                                did, cid
                            ))
                        })
                        .transpose()
                        .map_err(|_| {
                            AgentError::from(ClientError::invalid_request("Invalid avatar URI"))
                        })?;
                    let banner = weaver_record
                        .value
                        .banner
                        .as_ref()
                        .map(|blob| {
                            let cid = blob.blob().cid();
                            jacquard::types::string::Uri::new_owned(format!(
                                "https://cdn.bsky.app/img/banner/plain/{}/{}@jpeg",
                                did, cid
                            ))
                        })
                        .transpose()
                        .map_err(|_| {
                            AgentError::from(ClientError::invalid_request("Invalid banner URI"))
                        })?;

                    let profile_view = ProfileView::new()
                        .did(did.clone())
                        .handle(handle.clone())
                        .maybe_display_name(weaver_record.value.display_name.clone())
                        .maybe_description(weaver_record.value.description.clone())
                        .maybe_avatar(avatar)
                        .maybe_banner(banner)
                        .maybe_bluesky(weaver_record.value.bluesky)
                        .maybe_tangled(weaver_record.value.tangled)
                        .maybe_streamplace(weaver_record.value.streamplace)
                        .maybe_location(weaver_record.value.location.clone())
                        .maybe_links(weaver_record.value.links.clone())
                        .maybe_pronouns(weaver_record.value.pronouns.clone())
                        .maybe_pinned(weaver_record.value.pinned.clone())
                        .indexed_at(jacquard::types::string::Datetime::now())
                        .maybe_created_at(weaver_record.value.created_at)
                        .build();

                    Ok((
                        Some(weaver_uri.as_uri().clone().into_static()),
                        ProfileDataView::new()
                            .inner(ProfileDataViewInner::ProfileView(Box::new(profile_view)))
                            .build()
                            .into_static(),
                    ))
                } else {
                    Err(WeaverError::Agent(
                        ClientError::invalid_request("Invalid weaver profile URI").into(),
                    ))
                }
            };
            let bsky_appview_future = async {
                if let Ok(bsky_resp) = self
                    .send(GetProfile::new().actor(did.clone()).build())
                    .await
                {
                    if let Ok(output) = bsky_resp.into_output() {
                        let bsky_uri =
                            BskyProfile::uri(format!("at://{}/app.bsky.actor.profile/self", did))
                                .map_err(|_| {
                                AgentError::from(ClientError::invalid_request(
                                    "Invalid bsky profile URI",
                                ))
                            })?;
                        Ok((
                            Some(bsky_uri.as_uri().clone().into_static()),
                            ProfileDataView::new()
                                .inner(ProfileDataViewInner::ProfileViewDetailed(Box::new(
                                    output.value,
                                )))
                                .build()
                                .into_static(),
                        ))
                    } else {
                        Err(WeaverError::Agent(
                            ClientError::invalid_request("Invalid bsky profile URI").into(),
                        ))
                    }
                } else {
                    Err(WeaverError::Agent(
                        ClientError::invalid_request("Invalid bsky profile URI").into(),
                    ))
                }
            };

            if let Ok((profile_uri, weaver_profileview)) = weaver_future.await {
                return Ok((profile_uri, weaver_profileview));
            } else if let Ok((profile_uri, bsky_profileview)) = bsky_appview_future.await {
                return Ok((profile_uri, bsky_profileview));
            } else {
                Err(WeaverError::Agent(AgentError::from(
                    ClientError::invalid_request("couldn't fetch profile"),
                )))
            }
        }
    }

    /// View an entry at a specific index with prev/next navigation
    #[cfg(feature = "use-index")]
    fn view_entry<'a>(
        &self,
        notebook: &NotebookView<'a>,
        _entries: &[StrongRef<'_>],
        index: usize,
    ) -> impl Future<Output = Result<BookEntryView<'a>, WeaverError>> {
        async move {
            use weaver_api::sh_weaver::notebook::get_book_entry::GetBookEntry;

            let resp = self
                .send(
                    GetBookEntry::new()
                        .notebook(notebook.uri.clone())
                        .index(index as i64)
                        .build(),
                )
                .await
                .map_err(|e| AgentError::from(ClientError::from(e)))?;

            let output = resp.into_output().map_err(|e| {
                AgentError::from(ClientError::invalid_request(format!(
                    "Failed to get book entry: {}",
                    e
                )))
            })?;

            Ok(output.value.into_static())
        }
    }

    /// View an entry at a specific index with prev/next navigation
    #[cfg(not(feature = "use-index"))]
    fn view_entry<'a>(
        &self,
        notebook: &NotebookView<'a>,
        entries: &[StrongRef<'_>],
        index: usize,
    ) -> impl Future<Output = Result<BookEntryView<'a>, WeaverError>> {
        async move {
            use weaver_api::sh_weaver::notebook::BookEntryRef;

            let entry_ref = entries.get(index).ok_or_else(|| {
                AgentError::from(ClientError::invalid_request("entry out of bounds"))
            })?;
            let entry = self.fetch_entry_view(notebook, entry_ref).await?;

            let prev_entry = if index > 0 {
                let prev_entry_ref = &entries[index - 1];
                self.fetch_entry_view(notebook, prev_entry_ref).await.ok()
            } else {
                None
            }
            .map(|e| BookEntryRef::new().entry(e).build());

            let next_entry = if index < entries.len() - 1 {
                let next_entry_ref = &entries[index + 1];
                self.fetch_entry_view(notebook, next_entry_ref).await.ok()
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
    }

    /// View a page at a specific index with prev/next navigation
    fn view_page<'a>(
        &self,
        notebook: &NotebookView<'a>,
        pages: &[StrongRef<'_>],
        index: usize,
    ) -> impl Future<Output = Result<BookEntryView<'a>, WeaverError>> {
        async move {
            use weaver_api::sh_weaver::notebook::BookEntryRef;

            let entry_ref = pages.get(index).ok_or_else(|| {
                AgentError::from(ClientError::invalid_request("entry out of bounds"))
            })?;
            let entry = self.fetch_page_view(notebook, entry_ref).await?;

            let prev_entry = if index > 0 {
                let prev_entry_ref = &pages[index - 1];
                self.fetch_page_view(notebook, prev_entry_ref).await.ok()
            } else {
                None
            }
            .map(|e| BookEntryRef::new().entry(e).build());

            let next_entry = if index < pages.len() - 1 {
                let next_entry_ref = &pages[index + 1];
                self.fetch_page_view(notebook, next_entry_ref).await.ok()
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
    }

    /// Fetch a page view (like fetch_entry_view but for pages)
    fn fetch_page_view<'a>(
        &self,
        notebook: &NotebookView<'a>,
        entry_ref: &StrongRef<'_>,
    ) -> impl Future<Output = Result<EntryView<'a>, WeaverError>>
    where
        Self: Sized,
    {
        async move {
            use jacquard::to_data;
            use weaver_api::sh_weaver::notebook::page::Page;

            let page_uri = Page::uri(entry_ref.uri.clone())
                .map_err(|_| AgentError::from(ClientError::invalid_request("Invalid page URI")))?;

            // Get the rkey for version lookup
            let rkey = page_uri.rkey().ok_or_else(|| {
                AgentError::from(ClientError::invalid_request("Page URI missing rkey"))
            })?;

            // Fetch permissions for this page (includes inherited notebook permissions)
            let permissions = self.get_permissions_for_resource(&page_uri).await?;

            // Get all collaborators (owner + invited)
            let owner_did = match page_uri.authority() {
                jacquard::types::ident::AtIdentifier::Did(d) => d.clone().into_static(),
                jacquard::types::ident::AtIdentifier::Handle(h) => {
                    let (did, _) = self.pds_for_handle(h).await.map_err(|e| {
                        AgentError::from(
                            ClientError::from(e).with_context("Failed to resolve handle"),
                        )
                    })?;
                    did.into_static()
                }
            };
            let collaborators = self
                .find_collaborators_for_resource(&page_uri)
                .await
                .unwrap_or_default();
            let all_dids: Vec<Did<'static>> = std::iter::once(owner_did)
                .chain(collaborators.into_iter())
                .collect();

            // Find all versions across collaborators, get latest by updatedAt
            let versions = self
                .find_all_versions(
                    <Page as jacquard::types::collection::Collection>::NSID,
                    rkey.0.as_str(),
                    &all_dids,
                )
                .await
                .unwrap_or_default();

            // Use latest version if found, otherwise fall back to direct fetch
            let (page_data, final_uri, final_cid) = if let Some(latest) = versions.first() {
                let page: Page = jacquard::from_data(&latest.value).map_err(|_| {
                    AgentError::from(ClientError::invalid_request(
                        "Failed to deserialize latest page",
                    ))
                })?;
                (page.into_static(), latest.uri.clone(), latest.cid.clone())
            } else {
                // No versions found, fetch directly from PDS
                let page = self.fetch_record(&page_uri).await?;
                let cid = page.cid.ok_or_else(|| {
                    AgentError::from(ClientError::invalid_request("Page missing CID"))
                })?;
                (
                    page.value.into_static(),
                    page.uri.into_static(),
                    cid.into_static(),
                )
            };

            let title = page_data.title.clone();
            let tags = page_data.tags.clone();

            // Fetch contributors (evidence-based authors) for this page
            let contributor_dids = self.find_contributors_for_resource(&page_uri).await?;
            let mut authors = Vec::new();
            for (index, did) in contributor_dids.iter().enumerate() {
                let (profile_uri, profile_view) = self.hydrate_profile_view(did).await?;
                authors.push(
                    AuthorListView::new()
                        .maybe_uri(profile_uri)
                        .record(profile_view)
                        .index(index as i64)
                        .build(),
                );
            }

            Ok(EntryView::new()
                .cid(final_cid)
                .uri(final_uri)
                .indexed_at(jacquard::types::string::Datetime::now())
                .record(to_data(&page_data).map_err(|_| {
                    AgentError::from(ClientError::invalid_request("Failed to serialize page"))
                })?)
                .maybe_tags(tags)
                .title(title)
                .authors(authors)
                .permissions(permissions)
                .build())
        }
    }

    /// Find the notebook that contains a given entry using constellation backlinks.
    ///
    /// Queries constellation for `sh.weaver.notebook.book` records that reference
    /// the given entry URI via the `.entryList[].uri` path.
    fn find_notebook_for_entry(
        &self,
        entry_uri: &AtUri<'_>,
    ) -> impl Future<Output = Result<Option<RecordId<'static>>, WeaverError>>
    where
        Self: Sized,
    {
        async move {
            let (_, first) = self.find_notebooks_for_entry(entry_uri).await?;
            Ok(first)
        }
    }

    /// Find notebooks containing an entry, returning count and optionally the first one.
    ///
    /// Uses constellation backlinks to reverse lookup. Returns:
    /// - total count of notebooks containing this entry
    /// - The first notebook RecordId (if any exist)
    fn find_notebooks_for_entry(
        &self,
        entry_uri: &AtUri<'_>,
    ) -> impl Future<Output = Result<(u64, Option<RecordId<'static>>), WeaverError>>
    where
        Self: Sized,
    {
        async move {
            let constellation_url = Url::parse(CONSTELLATION_URL).map_err(|e| {
                AgentError::from(ClientError::invalid_request(format!(
                    "Invalid constellation URL: {}",
                    e
                )))
            })?;

            // Query with limit 2 - we only need to know if there's more than 1
            let query = GetBacklinksQuery {
                subject: Uri::At(entry_uri.clone().into_static()),
                source: "sh.weaver.notebook.book:.entryList[].uri".into(),
                cursor: None,
                did: vec![],
                limit: 2,
            };

            let response = self
                .xrpc(constellation_url)
                .send(&query)
                .await
                .map_err(|e| {
                    AgentError::from(ClientError::invalid_request(format!(
                        "Constellation query failed: {}",
                        e
                    )))
                })?;

            let output = response.into_output().map_err(|e| {
                AgentError::from(ClientError::invalid_request(format!(
                    "Failed to parse constellation response: {}",
                    e
                )))
            })?;

            Ok((
                output.total,
                output.records.into_iter().next().map(|r| r.into_static()),
            ))
        }
    }

    /// Fetch an entry directly by its rkey, returning the EntryView and raw Entry.
    ///
    /// This bypasses notebook context entirely - useful for standalone entries
    /// or when you have the rkey but not the notebook.
    #[cfg(feature = "use-index")]
    fn fetch_entry_by_rkey(
        &self,
        ident: &jacquard::types::ident::AtIdentifier<'_>,
        rkey: &str,
    ) -> impl Future<Output = Result<(EntryView<'static>, entry::Entry<'static>), WeaverError>>
    where
        Self: Sized,
    {
        async move {
            use jacquard::types::collection::Collection;
            use weaver_api::sh_weaver::notebook::get_entry::GetEntry;

            // Build entry URI from ident + rkey
            let entry_uri_str = format!("at://{}/{}/{}", ident, entry::Entry::NSID, rkey);
            let entry_uri = AtUri::new(&entry_uri_str)
                .map_err(|_| AgentError::from(ClientError::invalid_request("Invalid entry URI")))?
                .into_static();

            let resp = self
                .send(GetEntry::new().uri(entry_uri).build())
                .await
                .map_err(|e| AgentError::from(ClientError::from(e)))?;

            let output = resp.into_output().map_err(|e| {
                AgentError::from(ClientError::invalid_request(format!(
                    "Failed to get entry: {}",
                    e
                )))
            })?;

            // Clone the record for deserialization so we can consume output.value
            let record_clone = output.value.record.clone();

            // Deserialize Entry from the cloned record
            let entry_value: entry::Entry = jacquard::from_data(&record_clone).map_err(|e| {
                AgentError::from(ClientError::invalid_request(format!(
                    "Failed to deserialize entry record: {}",
                    e
                )))
            })?;

            Ok((output.value.into_static(), entry_value.into_static()))
        }
    }

    /// Fetch an entry directly by its rkey, returning the EntryView and raw Entry.
    ///
    /// This bypasses notebook context entirely - useful for standalone entries
    /// or when you have the rkey but not the notebook.
    #[cfg(not(feature = "use-index"))]
    fn fetch_entry_by_rkey(
        &self,
        ident: &jacquard::types::ident::AtIdentifier<'_>,
        rkey: &str,
    ) -> impl Future<Output = Result<(EntryView<'static>, entry::Entry<'static>), WeaverError>>
    where
        Self: Sized,
    {
        async move {
            use jacquard::to_data;
            use jacquard::types::collection::Collection;

            // Resolve DID from ident
            let repo_did = match ident {
                jacquard::types::ident::AtIdentifier::Did(did) => did.clone(),
                jacquard::types::ident::AtIdentifier::Handle(handle) => {
                    let (did, _pds) = self.pds_for_handle(handle).await.map_err(|e| {
                        AgentError::from(
                            ClientError::from(e).with_context("Failed to resolve handle"),
                        )
                    })?;
                    did
                }
            };

            // Build entry URI for contributor/permission queries
            let entry_uri_str = format!("at://{}/{}/{}", repo_did, entry::Entry::NSID, rkey);
            let entry_uri = AtUri::new(&entry_uri_str)
                .map_err(|_| AgentError::from(ClientError::invalid_request("Invalid entry URI")))?
                .into_static();

            // Get collaborators for version lookup
            let collaborators = self
                .find_collaborators_for_resource(&entry_uri)
                .await
                .unwrap_or_default();
            let all_dids: Vec<Did<'static>> = std::iter::once(repo_did.clone().into_static())
                .chain(collaborators.into_iter())
                .collect();

            // Find all versions across collaborators, get latest by updatedAt
            let versions = self
                .find_all_versions(entry::Entry::NSID, rkey, &all_dids)
                .await
                .unwrap_or_default();

            // Use latest version if found, otherwise fetch directly from original ident
            let (entry_value, final_uri, final_cid) = if let Some(latest) = versions.first() {
                let entry: entry::Entry = jacquard::from_data(&latest.value).map_err(|e| {
                    AgentError::from(ClientError::invalid_request(format!(
                        "Failed to deserialize latest entry: {}",
                        e
                    )))
                })?;
                (entry.into_static(), latest.uri.clone(), latest.cid.clone())
            } else {
                // Fallback: fetch directly via slingshot
                let record = self.fetch_record_slingshot(&entry_uri).await?;

                let entry: entry::Entry = jacquard::from_data(&record.value).map_err(|e| {
                    AgentError::from(ClientError::invalid_request(format!(
                        "Failed to deserialize entry: {}",
                        e
                    )))
                })?;

                let cid = record.cid.ok_or_else(|| {
                    AgentError::from(ClientError::invalid_request("Entry missing CID"))
                })?;

                (
                    entry.into_static(),
                    record.uri.into_static(),
                    cid.into_static(),
                )
            };

            // Fetch contributors (evidence-based authors)
            let contributor_dids = self.find_contributors_for_resource(&entry_uri).await?;
            let mut authors = Vec::new();
            for (index, did) in contributor_dids.iter().enumerate() {
                let (profile_uri, profile_view) = self.hydrate_profile_view(did).await?;
                authors.push(
                    AuthorListView::new()
                        .maybe_uri(profile_uri)
                        .record(profile_view)
                        .index(index as i64)
                        .build(),
                );
            }

            // Fetch permissions
            let permissions = self.get_permissions_for_resource(&entry_uri).await?;

            let entry_view = EntryView::new()
                .cid(final_cid)
                .uri(final_uri)
                .indexed_at(jacquard::types::string::Datetime::now())
                .record(to_data(&entry_value).map_err(|_| {
                    AgentError::from(ClientError::invalid_request("Failed to serialize entry"))
                })?)
                .maybe_tags(entry_value.tags.clone())
                .title(entry_value.title.clone())
                .path(entry_value.path.clone())
                .authors(authors)
                .permissions(permissions)
                .build()
                .into_static();

            Ok((entry_view, entry_value.into_static()))
        }
    }

    /// Find an entry's index within a notebook by rkey.
    ///
    /// Scans the notebook's entry_list comparing rkeys extracted from URIs.
    /// When found, builds BookEntryView with prev/next navigation.
    fn entry_in_notebook_by_rkey<'a>(
        &self,
        notebook: &NotebookView<'a>,
        entries: &[StrongRef<'_>],
        rkey: &str,
    ) -> impl Future<Output = Result<Option<BookEntryView<'a>>, WeaverError>> {
        async move {
            use weaver_api::sh_weaver::notebook::BookEntryRef;

            // Find the entry index by comparing rkeys
            let mut found_index = None;
            for (index, entry_ref) in entries.iter().enumerate() {
                // Extract rkey from the entry URI
                if let Ok(uri) = AtUri::new(entry_ref.uri.as_ref()) {
                    if let Some(entry_rkey) = uri.rkey() {
                        if entry_rkey.0.as_str() == rkey {
                            found_index = Some(index);
                            break;
                        }
                    }
                }
            }

            let index = match found_index {
                Some(i) => i,
                None => return Ok(None),
            };

            // Build BookEntryView with prev/next navigation
            let entry_ref = &entries[index];
            let entry = self.fetch_entry_view(notebook, entry_ref).await?;

            let prev_entry = if index > 0 {
                let prev_entry_ref = &entries[index - 1];
                self.fetch_entry_view(notebook, prev_entry_ref).await.ok()
            } else {
                None
            }
            .map(|e| BookEntryRef::new().entry(e).build());

            let next_entry = if index < entries.len() - 1 {
                let next_entry_ref = &entries[index + 1];
                self.fetch_entry_view(notebook, next_entry_ref).await.ok()
            } else {
                None
            }
            .map(|e| BookEntryRef::new().entry(e).build());

            Ok(Some(
                BookEntryView::new()
                    .entry(entry)
                    .maybe_next(next_entry)
                    .maybe_prev(prev_entry)
                    .index(index as i64)
                    .build(),
            ))
        }
    }

    /// Find valid collaborators for a resource.
    ///
    /// Queries Constellation for invite/accept record pairs:
    /// 1. Find all invites targeting this resource URI
    /// 2. For each invite, check if there's a matching accept record
    /// 3. Return DIDs that have both invite AND accept
    fn find_collaborators_for_resource(
        &self,
        resource_uri: &AtUri<'_>,
    ) -> impl Future<Output = Result<Vec<Did<'static>>, WeaverError>>
    where
        Self: Sized,
    {
        async move {
            use weaver_api::sh_weaver::collab::invite::Invite;

            const INVITE_NSID: &str = "sh.weaver.collab.invite";
            const ACCEPT_NSID: &str = "sh.weaver.collab.accept";

            let constellation_url = Url::parse(CONSTELLATION_URL).map_err(|e| {
                AgentError::from(ClientError::invalid_request(format!(
                    "Invalid constellation URL: {}",
                    e
                )))
            })?;

            // Step 1: Find all invites for this resource
            let invite_query = GetBacklinksQuery {
                subject: Uri::At(resource_uri.clone().into_static()),
                source: format!("{}:resource.uri", INVITE_NSID).into(),
                cursor: None,
                did: vec![],
                limit: 100,
            };

            let response = self
                .xrpc(constellation_url.clone())
                .send(&invite_query)
                .await
                .map_err(|e| {
                    AgentError::from(ClientError::invalid_request(format!(
                        "Constellation query failed: {}",
                        e
                    )))
                })?;

            let invite_output = response.into_output().map_err(|e| {
                AgentError::from(ClientError::invalid_request(format!(
                    "Failed to parse constellation response: {}",
                    e
                )))
            })?;

            let mut collaborators = Vec::new();

            // Step 2: For each invite, check for a matching accept
            for record_id in invite_output.records {
                let invite_uri_str = format!(
                    "at://{}/{}/{}",
                    record_id.did,
                    INVITE_NSID,
                    record_id.rkey.0.as_ref()
                );
                let Ok(invite_uri) = AtUri::new(&invite_uri_str) else {
                    continue;
                };

                // Fetch the invite to get the invitee DID
                let Ok(invite_resp) = self.get_record::<Invite>(&invite_uri).await else {
                    continue;
                };
                let Ok(invite_record) = invite_resp.into_output() else {
                    continue;
                };

                let invitee_did = invite_record.value.invitee.clone().into_static();

                // Query for accept records referencing this invite
                let accept_query = GetBacklinksQuery {
                    subject: Uri::At(invite_uri.into_static()),
                    source: format!("{}:invite.uri", ACCEPT_NSID).into(),
                    cursor: None,
                    did: vec![invitee_did.clone()],
                    limit: 1,
                };

                let Ok(accept_resp) = self
                    .xrpc(constellation_url.clone())
                    .send(&accept_query)
                    .await
                else {
                    continue;
                };

                let Ok(accept_output) = accept_resp.into_output() else {
                    continue;
                };

                if !accept_output.records.is_empty() {
                    // Both parties in a valid invite+accept pair are authorized
                    let inviter_did = record_id.did.clone().into_static();
                    collaborators.push(inviter_did);
                    collaborators.push(invitee_did);
                }
            }

            // Deduplicate (someone might appear in multiple pairs)
            collaborators.sort();
            collaborators.dedup();

            Ok(collaborators)
        }
    }

    /// Find all versions of a record across collaborator repositories.
    ///
    /// For each collaborator DID, attempts to fetch `at://{did}/{collection}/{rkey}`.
    /// Returns all found versions sorted by `updated_at` descending (latest first).
    fn find_all_versions<'a>(
        &'a self,
        collection: &'a str,
        rkey: &'a str,
        collaborators: &'a [Did<'_>],
    ) -> impl Future<Output = Result<Vec<CollaboratorVersion<'static>>, WeaverError>> + 'a
    where
        Self: Sized,
    {
        async move {
            use jacquard::Data;

            let mut versions = Vec::new();

            for collab_did in collaborators {
                // Build URI for this collaborator's version
                let uri_str = format!("at://{}/{}/{}", collab_did, collection, rkey);
                let Ok(uri) = AtUri::new(&uri_str) else {
                    continue;
                };

                // Fetch via slingshot (handles cross-PDS routing)
                let Ok(record) = self.fetch_record_slingshot(&uri).await else {
                    continue;
                };

                let Some(cid) = record.cid else {
                    continue;
                };

                let updated_at = record
                    .value
                    .query("...updatedAt")
                    .first()
                    .or_else(|| record.value.query("...createdAt").first())
                    .and_then(|v: &Data| v.as_str())
                    .and_then(|s| s.parse::<jacquard::types::string::Datetime>().ok());

                versions.push(CollaboratorVersion {
                    did: collab_did.clone().into_static(),
                    uri: record.uri.into_static(),
                    cid: cid.into_static(),
                    updated_at,
                    value: record.value.into_static(),
                });
            }

            // Sort by updated_at descending (latest first)
            versions.sort_by(|a, b| match (&b.updated_at, &a.updated_at) {
                (Some(b_time), Some(a_time)) => b_time.as_ref().cmp(a_time.as_ref()),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => std::cmp::Ordering::Equal,
            });

            Ok(versions)
        }
    }

    /// Check if a user can edit a resource based on collaboration records.
    ///
    /// Returns true if the user is the resource owner OR has valid invite+accept.
    fn can_user_edit_resource<'a>(
        &'a self,
        resource_uri: &'a AtUri<'_>,
        user_did: &'a Did<'_>,
    ) -> impl Future<Output = Result<bool, WeaverError>> + 'a
    where
        Self: Sized,
    {
        async move {
            // Check if user is the owner
            if let jacquard::types::ident::AtIdentifier::Did(owner_did) = resource_uri.authority() {
                if owner_did == user_did {
                    return Ok(true);
                }
            }

            // Check for valid collaboration
            let collaborators = self.find_collaborators_for_resource(resource_uri).await?;
            Ok(collaborators.iter().any(|c| c == user_did))
        }
    }

    /// Check if a user can edit an entry, considering notebook-level cascading.
    ///
    /// An entry is editable if user owns it, has entry-level collab, or has notebook-level collab.
    fn can_user_edit_entry<'a>(
        &'a self,
        entry_uri: &'a AtUri<'_>,
        user_did: &'a Did<'_>,
    ) -> impl Future<Output = Result<bool, WeaverError>> + 'a
    where
        Self: Sized,
    {
        async move {
            // Check entry-level access first
            if self.can_user_edit_resource(entry_uri, user_did).await? {
                return Ok(true);
            }

            // Check notebook-level access (cascade)
            if let Some(notebook_id) = self.find_notebook_for_entry(entry_uri).await? {
                let notebook_uri_str = format!(
                    "at://{}/{}/{}",
                    notebook_id.did,
                    notebook_id.collection,
                    notebook_id.rkey.0.as_ref()
                );
                if let Ok(notebook_uri) = AtUri::new(&notebook_uri_str) {
                    if self.can_user_edit_resource(&notebook_uri, user_did).await? {
                        return Ok(true);
                    }
                }
            }

            Ok(false)
        }
    }

    /// Get the full permissions state for a resource.
    ///
    /// Returns PermissionsState with all editors:
    /// - Resource authority (source = resource URI, grantedAt = createdAt)
    /// - Invited collaborators (source = invite URI, grantedAt = accept createdAt)
    /// - For entries: inherited notebook-level collaborators
    fn get_permissions_for_resource(
        &self,
        resource_uri: &AtUri<'_>,
    ) -> impl Future<Output = Result<PermissionsState<'static>, WeaverError>>
    where
        Self: Sized,
    {
        async move {
            use weaver_api::sh_weaver::collab::accept::Accept;
            use weaver_api::sh_weaver::collab::invite::Invite;

            const INVITE_NSID: &str = "sh.weaver.collab.invite";
            const ACCEPT_NSID: &str = "sh.weaver.collab.accept";

            let constellation_url = Url::parse(CONSTELLATION_URL).map_err(|e| {
                AgentError::from(ClientError::invalid_request(format!(
                    "Invalid constellation URL: {}",
                    e
                )))
            })?;

            let mut editors = Vec::new();

            // 1. Resource authority - creating the resource is its own grant
            let authority_did = match resource_uri.authority() {
                jacquard::types::ident::AtIdentifier::Did(did) => did.clone().into_static(),
                jacquard::types::ident::AtIdentifier::Handle(handle) => {
                    let (did, _) = self.pds_for_handle(handle).await.map_err(|e| {
                        AgentError::from(
                            ClientError::from(e).with_context("Failed to resolve handle"),
                        )
                    })?;
                    did.into_static()
                }
            };

            // Fetch the record to get createdAt (use untyped fetch to handle any collection)
            let record = self
                .fetch_record_slingshot(resource_uri)
                .await
                .map_err(|e| WeaverError::from(AgentError::from(e)))?;
            let authority_granted_at = record
                .value
                .query("createdAt")
                .first()
                .and_then(|v| v.as_str())
                .and_then(|s| s.parse::<jacquard::types::string::Datetime>().ok())
                .ok_or_else(|| {
                    WeaverError::from(AgentError::from(ClientError::invalid_request(
                        "Record missing createdAt",
                    )))
                })?;

            editors.push(
                PermissionGrant::new()
                    .did(authority_did.clone())
                    .scope("direct")
                    .source(resource_uri.clone().into_static())
                    .granted_at(authority_granted_at)
                    .build()
                    .into_static(),
            );

            // 2. Find direct invites for this resource
            let invite_query = GetBacklinksQuery {
                subject: Uri::At(resource_uri.clone().into_static()),
                source: format!("{}:resource.uri", INVITE_NSID).into(),
                cursor: None,
                did: vec![],
                limit: 100,
            };

            let invite_response = self
                .xrpc(constellation_url.clone())
                .send(&invite_query)
                .await
                .map_err(|e| {
                    AgentError::from(ClientError::invalid_request(format!(
                        "Constellation invite query failed: {}",
                        e
                    )))
                })?;
            let invite_output = invite_response.into_output().map_err(|e| {
                AgentError::from(ClientError::invalid_request(format!(
                    "Failed to parse Constellation response: {}",
                    e
                )))
            })?;

            for record_id in invite_output.records {
                let invite_uri_str = format!(
                    "at://{}/{}/{}",
                    record_id.did,
                    INVITE_NSID,
                    record_id.rkey.0.as_ref()
                );
                let invite_uri = AtUri::new(&invite_uri_str).map_err(|_| {
                    AgentError::from(ClientError::invalid_request(
                        "Invalid invite URI from Constellation",
                    ))
                })?;

                // Fetch invite to get invitee DID
                let invite_record =
                    self.get_record::<Invite>(&invite_uri)
                        .await
                        .map_err(|e| WeaverError::from(AgentError::from(e)))?
                        .into_output()
                        .map_err(|e| {
                            WeaverError::from(AgentError::from(ClientError::invalid_request(
                                format!("Failed to parse invite record: {}", e),
                            )))
                        })?;

                let invitee_did = invite_record.value.invitee.clone().into_static();

                // Query for accept records referencing this invite
                let accept_query = GetBacklinksQuery {
                    subject: Uri::At(invite_uri.clone().into_static()),
                    source: format!("{}:invite.uri", ACCEPT_NSID).into(),
                    cursor: None,
                    did: vec![invitee_did.clone()],
                    limit: 1,
                };

                let accept_response = self
                    .xrpc(constellation_url.clone())
                    .send(&accept_query)
                    .await
                    .map_err(|e| {
                        AgentError::from(ClientError::invalid_request(format!(
                            "Constellation accept query failed: {}",
                            e
                        )))
                    })?;
                let accept_output = accept_response.into_output().map_err(|e| {
                    AgentError::from(ClientError::invalid_request(format!(
                        "Failed to parse Constellation accept response: {}",
                        e
                    )))
                })?;

                // No accept = pending invite, not an error - just skip
                let Some(accept_record_id) = accept_output.records.first() else {
                    continue;
                };

                let accept_uri_str = format!(
                    "at://{}/{}/{}",
                    accept_record_id.did,
                    ACCEPT_NSID,
                    accept_record_id.rkey.0.as_ref()
                );
                let accept_uri = AtUri::new(&accept_uri_str).map_err(|_| {
                    AgentError::from(ClientError::invalid_request(
                        "Invalid accept URI from Constellation",
                    ))
                })?;
                let accept_record =
                    self.get_record::<Accept>(&accept_uri)
                        .await
                        .map_err(|e| WeaverError::from(AgentError::from(e)))?
                        .into_output()
                        .map_err(|e| {
                            WeaverError::from(AgentError::from(ClientError::invalid_request(
                                format!("Failed to parse accept record: {}", e),
                            )))
                        })?;

                editors.push(
                    PermissionGrant::new()
                        .did(invitee_did)
                        .scope("direct")
                        .source(invite_uri.into_static())
                        .granted_at(accept_record.value.created_at)
                        .build()
                        .into_static(),
                );
            }

            // 3. For entries, check notebook-level invites (inherited)
            let is_entry = resource_uri
                .collection()
                .is_some_and(|c| c.as_ref() == "sh.weaver.notebook.entry");

            if is_entry {
                // Entry not in a notebook is fine - just no inherited permissions
                if let Some(notebook_id) = self.find_notebook_for_entry(resource_uri).await? {
                    let notebook_uri_str = format!(
                        "at://{}/{}/{}",
                        notebook_id.did,
                        notebook_id.collection,
                        notebook_id.rkey.0.as_ref()
                    );
                    let notebook_uri = AtUri::new(&notebook_uri_str).map_err(|_| {
                        AgentError::from(ClientError::invalid_request(
                            "Invalid notebook URI from Constellation",
                        ))
                    })?;

                    let notebook_invite_query = GetBacklinksQuery {
                        subject: Uri::At(notebook_uri.clone().into_static()),
                        source: format!("{}:resource.uri", INVITE_NSID).into(),
                        cursor: None,
                        did: vec![],
                        limit: 100,
                    };

                    let notebook_invite_response = self
                        .xrpc(constellation_url.clone())
                        .send(&notebook_invite_query)
                        .await
                        .map_err(|e| {
                            AgentError::from(ClientError::invalid_request(format!(
                                "Constellation notebook invite query failed: {}",
                                e
                            )))
                        })?;
                    let notebook_invite_output =
                        notebook_invite_response.into_output().map_err(|e| {
                            AgentError::from(ClientError::invalid_request(format!(
                                "Failed to parse Constellation response: {}",
                                e
                            )))
                        })?;

                    for record_id in notebook_invite_output.records {
                        let invite_uri_str = format!(
                            "at://{}/{}/{}",
                            record_id.did,
                            INVITE_NSID,
                            record_id.rkey.0.as_ref()
                        );
                        let invite_uri = AtUri::new(&invite_uri_str).map_err(|_| {
                            AgentError::from(ClientError::invalid_request(
                                "Invalid invite URI from Constellation",
                            ))
                        })?;

                        let invite_record = self
                            .get_record::<Invite>(&invite_uri)
                            .await
                            .map_err(|e| WeaverError::from(AgentError::from(e)))?
                            .into_output()
                            .map_err(|e| {
                                WeaverError::from(AgentError::from(ClientError::invalid_request(
                                    format!("Failed to parse invite record: {}", e),
                                )))
                            })?;

                        let invitee_did = invite_record.value.invitee.clone().into_static();

                        // Skip if already in direct grants (direct takes precedence)
                        if editors.iter().any(|g| g.did == invitee_did) {
                            continue;
                        }

                        let accept_query = GetBacklinksQuery {
                            subject: Uri::At(invite_uri.clone().into_static()),
                            source: format!("{}:.invite.uri", ACCEPT_NSID).into(),
                            cursor: None,
                            did: vec![invitee_did.clone()],
                            limit: 1,
                        };

                        let accept_response = self
                            .xrpc(constellation_url.clone())
                            .send(&accept_query)
                            .await
                            .map_err(|e| {
                                AgentError::from(ClientError::invalid_request(format!(
                                    "Constellation accept query failed: {}",
                                    e
                                )))
                            })?;
                        let accept_output = accept_response.into_output().map_err(|e| {
                            AgentError::from(ClientError::invalid_request(format!(
                                "Failed to parse Constellation accept response: {}",
                                e
                            )))
                        })?;

                        // No accept = pending invite, not an error - just skip
                        let Some(accept_record_id) = accept_output.records.first() else {
                            continue;
                        };

                        let accept_uri_str = format!(
                            "at://{}/{}/{}",
                            accept_record_id.did,
                            ACCEPT_NSID,
                            accept_record_id.rkey.0.as_ref()
                        );
                        let accept_uri = AtUri::new(&accept_uri_str).map_err(|_| {
                            AgentError::from(ClientError::invalid_request(
                                "Invalid accept URI from Constellation",
                            ))
                        })?;
                        let accept_record = self
                            .get_record::<Accept>(&accept_uri)
                            .await
                            .map_err(|e| WeaverError::from(AgentError::from(e)))?
                            .into_output()
                            .map_err(|e| {
                                WeaverError::from(AgentError::from(ClientError::invalid_request(
                                    format!("Failed to parse accept record: {}", e),
                                )))
                            })?;

                        editors.push(
                            PermissionGrant::new()
                                .did(invitee_did)
                                .scope("inherited")
                                .source(invite_uri.into_static())
                                .granted_at(accept_record.value.created_at)
                                .build()
                                .into_static(),
                        );
                    }
                }
            }

            Ok(PermissionsState::new()
                .editors(editors)
                .build()
                .into_static())
        }
    }

    // =========================================================================
    // Real-time Collaboration Session Management
    // =========================================================================

    /// Create a collaboration session record on the user's PDS.
    ///
    /// Called when joining a real-time editing session. The session record
    /// contains the iroh NodeId so other collaborators can discover and
    /// connect to this peer.
    ///
    /// Returns the AT-URI of the created session record.
    fn create_collab_session<'a>(
        &'a self,
        resource: &'a StrongRef<'a>,
        node_id: &'a str,
        relay_url: Option<&'a str>,
        ttl_minutes: Option<u32>,
    ) -> impl Future<Output = Result<AtUri<'static>, WeaverError>> + 'a {
        async move {
            use jacquard::types::string::Datetime;
            use weaver_api::sh_weaver::collab::session::Session;

            // Clean up any expired sessions first
            let _ = self.cleanup_expired_sessions().await;

            let now_chrono = chrono::Utc::now().fixed_offset();
            let now = Datetime::new(now_chrono);
            let expires_at = ttl_minutes.map(|mins| {
                let expires = now_chrono + chrono::Duration::minutes(mins as i64);
                Datetime::new(expires)
            });

            let relay_uri = relay_url
                .map(|url| jacquard::types::string::Uri::new(url))
                .transpose()
                .map_err(|_| AgentError::from(ClientError::invalid_request("Invalid relay URL")))?;

            let session = Session::new()
                .resource(resource.clone())
                .node_id(node_id)
                .created_at(now)
                .maybe_expires_at(expires_at)
                .maybe_relay_url(relay_uri)
                .build();

            let response = self.create_record(session, None).await?;
            Ok(response.uri.into_static())
        }
    }

    /// Delete a collaboration session record.
    ///
    /// Called when leaving a real-time editing session to clean up.
    fn delete_collab_session<'a>(
        &'a self,
        session_uri: &'a AtUri<'a>,
    ) -> impl Future<Output = Result<(), WeaverError>> + 'a {
        async move {
            use weaver_api::sh_weaver::collab::session::Session;

            let rkey = session_uri.rkey().ok_or_else(|| {
                AgentError::from(ClientError::invalid_request("Session URI missing rkey"))
            })?;
            self.delete_record::<Session>(rkey.clone()).await?;
            Ok(())
        }
    }

    /// Refresh a collaboration session's TTL.
    ///
    /// Called periodically to indicate the session is still active.
    fn refresh_collab_session<'a>(
        &'a self,
        session_uri: &'a AtUri<'a>,
        ttl_minutes: u32,
    ) -> impl Future<Output = Result<(), WeaverError>> + 'a {
        async move {
            use jacquard::types::string::Datetime;
            use weaver_api::sh_weaver::collab::session::Session;

            let now_chrono = chrono::Utc::now().fixed_offset();
            let expires = now_chrono + chrono::Duration::minutes(ttl_minutes as i64);
            let expires_at = Datetime::new(expires);

            self.update_record::<Session>(session_uri, |session| {
                session.expires_at = Some(expires_at);
            })
            .await?;
            Ok(())
        }
    }

    /// Update the relay URL in an existing session record.
    ///
    /// Called when the relay connection changes during a session.
    fn update_collab_session_relay<'a>(
        &'a self,
        session_uri: &'a AtUri<'a>,
        relay_url: Option<&'a str>,
    ) -> impl Future<Output = Result<(), WeaverError>> + 'a {
        async move {
            use weaver_api::sh_weaver::collab::session::Session;

            let relay_uri = relay_url
                .map(|url| jacquard::types::string::Uri::new(url))
                .transpose()
                .map_err(|_| AgentError::from(ClientError::invalid_request("Invalid relay URL")))?;

            self.update_record::<Session>(session_uri, |session| {
                session.relay_url = relay_uri.clone();
            })
            .await?;
            Ok(())
        }
    }

    /// Delete all expired session records for the current user.
    ///
    /// Called before creating a new session to clean up stale records.
    fn cleanup_expired_sessions<'a>(&'a self) -> impl Future<Output = Result<u32, WeaverError>> + 'a
    where
        Self: Sized,
    {
        async move {
            use jacquard::types::nsid::Nsid;
            use weaver_api::com_atproto::repo::list_records::ListRecords;
            use weaver_api::sh_weaver::collab::session::Session;

            let (did, _) = self.session_info().await.ok_or_else(|| {
                AgentError::from(ClientError::invalid_request("No active session"))
            })?;
            let now = chrono::Utc::now();
            let mut deleted = 0u32;

            // List all our session records
            let collection =
                Nsid::new("sh.weaver.collab.session").map_err(WeaverError::AtprotoString)?;
            let request = ListRecords::new()
                .repo(did.clone())
                .collection(collection)
                .limit(100)
                .build();

            let response = self.send(request).await.map_err(AgentError::from)?;
            let output = response.into_output().map_err(|e| {
                AgentError::from(ClientError::invalid_request(format!(
                    "Failed to list sessions: {}",
                    e
                )))
            })?;

            for record in output.records {
                if let Ok(session) = jacquard::from_data::<Session>(&record.value) {
                    // Check if expired
                    if let Some(ref expires_at) = session.expires_at {
                        let expires_str = expires_at.as_str();
                        if let Ok(expires) = chrono::DateTime::parse_from_rfc3339(expires_str) {
                            if expires.with_timezone(&chrono::Utc) < now {
                                // Delete expired session
                                if let Some(rkey) = record.uri.rkey() {
                                    if let Err(e) =
                                        self.delete_record::<Session>(rkey.clone()).await
                                    {
                                        tracing::warn!("Failed to delete expired session: {}", e);
                                    } else {
                                        deleted += 1;
                                    }
                                }
                            }
                        }
                    }
                }
            }

            if deleted > 0 {
                tracing::info!("Cleaned up {} expired session records", deleted);
            }

            Ok(deleted)
        }
    }

    /// Find active collaboration sessions for a resource.
    ///
    /// Queries Constellation for session records referencing the given resource,
    /// then fetches each to extract peer connection info.
    ///
    /// Returns peers with unexpired sessions (or no expiry set).
    fn find_session_peers<'a>(
        &'a self,
        resource_uri: &'a AtUri<'a>,
    ) -> impl Future<Output = Result<Vec<SessionPeer<'static>>, WeaverError>> + 'a
    where
        Self: Sized,
    {
        async move {
            use jacquard::types::string::Datetime;
            use weaver_api::sh_weaver::collab::session::Session;

            const SESSION_NSID: &str = "sh.weaver.collab.session";

            // Get authorized collaborators (owner is checked separately via URI authority)
            let collaborators: std::collections::HashSet<Did<'static>> = self
                .find_collaborators_for_resource(resource_uri)
                .await
                .unwrap_or_default()
                .into_iter()
                .collect();

            let constellation_url = Url::parse(CONSTELLATION_URL).map_err(|e| {
                AgentError::from(ClientError::invalid_request(format!(
                    "Invalid constellation URL: {}",
                    e
                )))
            })?;

            // Query for session records referencing this resource
            let query = GetBacklinksQuery {
                subject: Uri::At(resource_uri.clone().into_static()),
                source: format!("{}:resource.uri", SESSION_NSID).into(),
                cursor: None,
                did: vec![],
                limit: 100,
            };

            let response = self
                .xrpc(constellation_url)
                .send(&query)
                .await
                .map_err(|e| {
                    AgentError::from(ClientError::invalid_request(format!(
                        "Constellation query failed: {}",
                        e
                    )))
                })?;

            let output = response.into_output().map_err(|e| {
                AgentError::from(ClientError::invalid_request(format!(
                    "Failed to parse constellation response: {}",
                    e
                )))
            })?;

            let mut peers = Vec::new();
            let now = Datetime::now();

            for record_id in output.records {
                let session_uri_str = format!(
                    "at://{}/{}/{}",
                    record_id.did,
                    SESSION_NSID,
                    record_id.rkey.0.as_ref()
                );
                let Ok(session_uri) = AtUri::new(&session_uri_str) else {
                    continue;
                };

                // Fetch the session record
                let Ok(session_resp) = self.get_record::<Session>(&session_uri).await else {
                    continue;
                };
                let Ok(session_record) = session_resp.into_output() else {
                    continue;
                };

                // Check if session has expired (Datetime implements Ord)
                if let Some(ref expires_at) = session_record.value.expires_at {
                    if *expires_at < now {
                        continue; // Session expired
                    }
                }

                // Check if peer is authorized (has valid invite+accept pair)
                let peer_did = record_id.did.clone().into_static();
                if !collaborators.contains(&peer_did) {
                    tracing::debug!(
                        peer = %peer_did,
                        "Filtering out unauthorized session peer"
                    );
                    continue;
                }

                peers.push(SessionPeer {
                    did: record_id.did.into_static(),
                    node_id: session_record.value.node_id.as_ref().into(),
                    relay_url: session_record.value.relay_url.map(|u| u.as_ref().into()),
                    created_at: session_record.value.created_at,
                    expires_at: session_record.value.expires_at,
                });
            }

            Ok(peers)
        }
    }

    /// Find contributors (authors) for a resource based on evidence.
    ///
    /// Contributors are DIDs who have actually contributed to this resource:
    /// 1. Edit records (edit.root or edit.diff) referencing this resource
    /// 2. Published versions of the record in their repo (same rkey)
    ///
    /// This is separate from permissions - you can have edit permission without
    /// having contributed yet.
    fn find_contributors_for_resource(
        &self,
        resource_uri: &AtUri<'_>,
    ) -> impl Future<Output = Result<Vec<Did<'static>>, WeaverError>>
    where
        Self: Sized,
    {
        async move {
            const EDIT_ROOT_NSID: &str = "sh.weaver.edit.root";

            let constellation_url = Url::parse(CONSTELLATION_URL).map_err(|e| {
                AgentError::from(ClientError::invalid_request(format!(
                    "Invalid constellation URL: {}",
                    e
                )))
            })?;

            let mut contributors = std::collections::HashSet::new();

            // 1. Resource authority is always a contributor
            let authority_did = match resource_uri.authority() {
                jacquard::types::ident::AtIdentifier::Did(did) => did.clone().into_static(),
                jacquard::types::ident::AtIdentifier::Handle(handle) => {
                    let (did, _) = self.pds_for_handle(handle).await.map_err(|e| {
                        AgentError::from(
                            ClientError::from(e).with_context("Failed to resolve handle"),
                        )
                    })?;
                    did.into_static()
                }
            };
            contributors.insert(authority_did);

            // 2. Find DIDs with edit records for this resource
            let edit_query = GetBacklinksQuery {
                subject: Uri::At(resource_uri.clone().into_static()),
                source: format!("{}:doc.value.entry.uri", EDIT_ROOT_NSID).into(),
                cursor: None,
                did: vec![],
                limit: 100,
            };

            if let Ok(response) = self.xrpc(constellation_url.clone()).send(&edit_query).await {
                if let Ok(edit_output) = response.into_output() {
                    for record_id in edit_output.records {
                        contributors.insert(record_id.did.into_static());
                    }
                }
            }

            // 3. Find collaborators who have published versions (same rkey)
            let collaborators = self.find_collaborators_for_resource(resource_uri).await?;
            let rkey = resource_uri.rkey();
            let collection = resource_uri.collection();

            if let (Some(rkey), Some(collection)) = (rkey, collection) {
                for collab_did in collaborators {
                    // Try to fetch their version of the record via slingshot
                    let collab_uri_str = format!(
                        "at://{}/{}/{}",
                        collab_did.as_ref(),
                        collection,
                        rkey.as_ref()
                    );
                    if let Ok(collab_uri) = AtUri::new(&collab_uri_str) {
                        // Check if record actually exists
                        if self.fetch_record_slingshot(&collab_uri).await.is_ok() {
                            contributors.insert(collab_did);
                        }
                    }
                }
            }

            Ok(contributors.into_iter().collect())
        }
    }
}

/// A version of a record from a collaborator's repository.
#[derive(Debug, Clone)]
pub struct CollaboratorVersion<'a> {
    /// The DID of the collaborator who owns this version.
    pub did: Did<'a>,
    /// The full URI of this version.
    pub uri: AtUri<'a>,
    /// CID of this version.
    pub cid: jacquard::types::string::Cid<'a>,
    /// When this version was last updated.
    pub updated_at: Option<jacquard::types::string::Datetime>,
    /// The raw record value.
    pub value: jacquard::Data<'a>,
}

/// Information about a peer discovered from session records.
#[derive(Debug, Clone)]
pub struct SessionPeer<'a> {
    /// The peer's DID.
    pub did: Did<'a>,
    /// The peer's iroh NodeId (z-base32 encoded).
    pub node_id: SmolStr,
    /// Optional relay URL for browser clients.
    pub relay_url: Option<SmolStr>,
    /// When the session was created.
    pub created_at: jacquard::types::string::Datetime,
    /// When the session expires (if set).
    pub expires_at: Option<jacquard::types::string::Datetime>,
}

impl<T: AgentSession + IdentityResolver + XrpcExt> WeaverExt for T {}
