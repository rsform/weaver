use weaver_api::app_bsky::actor::get_profile::GetProfile;
// Re-export view types for use elsewhere
pub use weaver_api::sh_weaver::notebook::{
    AuthorListView, BookEntryRef, BookEntryView, EntryView, NotebookView,
};

// Re-export jacquard for convenience
use crate::constellation::{GetBacklinksQuery, RecordId};
use crate::error::WeaverError;
pub use jacquard;
use jacquard::bytes::Bytes;
use jacquard::client::{AgentError, AgentErrorKind, AgentSession, AgentSessionExt};
use jacquard::error::ClientError;
use jacquard::prelude::*;
use jacquard::types::blob::{BlobRef, MimeType};
use jacquard::types::string::{AtUri, Did, RecordKey, Rkey};
use jacquard::types::tid::Tid;
use jacquard::types::uri::Uri;
use jacquard::url::Url;
use jacquard::xrpc::Response;
use jacquard::{CowStr, IntoStatic, xrpc};
use mime_sniffer::MimeTypeSniffer;
use std::path::Path;
use weaver_api::com_atproto::repo::get_record::GetRecordResponse;
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
//#[cfg_attr(not(target_arch = "wasm32"), trait_variant::make(Send))]
pub trait WeaverExt: AgentSessionExt + XrpcExt + Send + Sync {
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
            let rkey = uri.rkey().ok_or_else(|| {
                AgentError::from(
                    ClientError::invalid_request("AtUri missing rkey")
                        .with_help("ensure the URI includes a record key after the collection"),
                )
            })?;

            // Resolve authority (DID or handle) to get DID and PDS
            use jacquard::types::ident::AtIdentifier;
            let (repo_did, pds_url) = match uri.authority() {
                AtIdentifier::Did(did) => {
                    let pds =
                        self.pds_for_did(did).await.map_err(|e| {
                            AgentError::from(ClientError::from(e).with_context(
                                "DID document resolution failed during record retrieval",
                            ))
                        })?;
                    (did.clone(), pds)
                }
                AtIdentifier::Handle(handle) => self.pds_for_handle(handle).await.map_err(|e| {
                    AgentError::from(
                        ClientError::from(e)
                            .with_context("handle resolution failed during record retrieval"),
                    )
                })?,
            };

            // Make stateless XRPC call to that PDS (no auth required for public records)
            use weaver_api::com_atproto::repo::get_record::GetRecord;
            let request = GetRecord::new()
                .repo(AtIdentifier::Did(repo_did))
                .collection(
                    uri.collection()
                        .expect("collection should exist if rkey does")
                        .clone(),
                )
                .rkey(rkey.clone())
                .build();

            let response: Response<GetRecordResponse> = {
                let http_request = xrpc::build_http_request(&pds_url, &request, &self.opts().await)
                    .map_err(|e| AgentError::from(ClientError::transport(e)))?;

                let http_response = self
                    .send_http(http_request)
                    .await
                    .map_err(|e| AgentError::from(ClientError::transport(e)))?;

                xrpc::process_response(http_response)
            }
            .map_err(|e| AgentError::new(AgentErrorKind::Client, Some(e.into())))?;
            let record = response.parse().map_err(|e| AgentError::xrpc(e))?;
            let strong_ref = StrongRef::new()
                .uri(record.uri)
                .cid(record.cid.expect("when does this NOT have a CID?"))
                .build();
            Ok(strong_ref.into_static())
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
    fn view_notebook(
        &self,
        uri: &AtUri<'_>,
    ) -> impl Future<Output = Result<(NotebookView<'static>, Vec<StrongRef<'static>>), WeaverError>>
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
    ) -> impl Future<Output = Result<EntryView<'a>, WeaverError>> {
        async move {
            use jacquard::to_data;
            use weaver_api::sh_weaver::notebook::entry::Entry;

            let entry_uri = Entry::uri(entry_ref.uri.clone())
                .map_err(|_| AgentError::from(ClientError::invalid_request("Invalid entry URI")))?;
            let entry = self.fetch_record(&entry_uri).await?;

            let title = entry.value.title.clone();
            let path = entry.value.path.clone();
            let tags = entry.value.tags.clone();

            Ok(EntryView::new()
                .cid(entry.cid.ok_or_else(|| {
                    AgentError::from(ClientError::invalid_request("Entry missing CID"))
                })?)
                .uri(entry.uri)
                .indexed_at(jacquard::types::string::Datetime::now())
                .record(to_data(&entry.value).map_err(|_| {
                    AgentError::from(ClientError::invalid_request("Failed to serialize entry"))
                })?)
                .maybe_tags(tags)
                .title(title)
                .path(path)
                .authors(notebook.authors.clone())
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

                        return Ok(Some((
                            NotebookView::new()
                                .cid(record.cid)
                                .uri(record.uri)
                                .indexed_at(jacquard::types::string::Datetime::now())
                                .title(matched)
                                .maybe_path(path)
                                .maybe_tags(tags)
                                .authors(authors)
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
            // let weaver_future = async {
            //     if let Ok(weaver_record) = self.fetch_record(&weaver_uri).await {
            //         // Convert blobs to CDN URLs
            //         let avatar = weaver_record
            //             .value
            //             .avatar
            //             .as_ref()
            //             .map(|blob| {
            //                 let cid = blob.blob().cid();
            //                 jacquard::types::string::Uri::new_owned(format!(
            //                     "https://cdn.bsky.app/img/avatar/plain/{}/{}@jpeg",
            //                     did, cid
            //                 ))
            //             })
            //             .transpose()
            //             .map_err(|_| {
            //                 AgentError::from(ClientError::invalid_request("Invalid avatar URI"))
            //             })?;
            //         let banner = weaver_record
            //             .value
            //             .banner
            //             .as_ref()
            //             .map(|blob| {
            //                 let cid = blob.blob().cid();
            //                 jacquard::types::string::Uri::new_owned(format!(
            //                     "https://cdn.bsky.app/img/banner/plain/{}/{}@jpeg",
            //                     did, cid
            //                 ))
            //             })
            //             .transpose()
            //             .map_err(|_| {
            //                 AgentError::from(ClientError::invalid_request("Invalid banner URI"))
            //             })?;

            //         let profile_view = ProfileView::new()
            //             .did(did.clone())
            //             .handle(handle.clone())
            //             .maybe_display_name(weaver_record.value.display_name.clone())
            //             .maybe_description(weaver_record.value.description.clone())
            //             .maybe_avatar(avatar)
            //             .maybe_banner(banner)
            //             .maybe_bluesky(weaver_record.value.bluesky)
            //             .maybe_tangled(weaver_record.value.tangled)
            //             .maybe_streamplace(weaver_record.value.streamplace)
            //             .maybe_location(weaver_record.value.location.clone())
            //             .maybe_links(weaver_record.value.links.clone())
            //             .maybe_pronouns(weaver_record.value.pronouns.clone())
            //             .maybe_pinned(weaver_record.value.pinned.clone())
            //             .indexed_at(jacquard::types::string::Datetime::now())
            //             .maybe_created_at(weaver_record.value.created_at)
            //             .build();

            //         Ok((
            //             Some(weaver_uri.as_uri().clone().into_static()),
            //             ProfileDataView::new()
            //                 .inner(ProfileDataViewInner::ProfileView(Box::new(profile_view)))
            //                 .build()
            //                 .into_static(),
            //         ))
            //     } else {
            //         Err(WeaverError::Agent(
            //             ClientError::invalid_request("Invalid weaver profile URI").into(),
            //         ))
            //     }
            // };
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
            // Fallback: fetch bsky profile record directly and construct minimal ProfileViewDetailed
            let bsky_uri = BskyProfile::uri(format!("at://{}/app.bsky.actor.profile/self", did))
                .map_err(|_| {
                    AgentError::from(ClientError::invalid_request("Invalid bsky profile URI"))
                })?;

            // let bsky_future = async {
            //     let bsky_record = self.fetch_record(&bsky_uri).await?;

            //     let avatar = bsky_record
            //         .value
            //         .avatar
            //         .as_ref()
            //         .map(|blob| {
            //             let cid = blob.blob().cid();
            //             jacquard::types::string::Uri::new_owned(format!(
            //                 "https://cdn.bsky.app/img/avatar/plain/{}/{}@jpeg",
            //                 did, cid
            //             ))
            //         })
            //         .transpose()
            //         .map_err(|_| {
            //             AgentError::from(ClientError::invalid_request("Invalid avatar URI"))
            //         })?;
            //     let banner = bsky_record
            //         .value
            //         .banner
            //         .as_ref()
            //         .map(|blob| {
            //             let cid = blob.blob().cid();
            //             jacquard::types::string::Uri::new_owned(format!(
            //                 "https://cdn.bsky.app/img/banner/plain/{}/{}@jpeg",
            //                 did, cid
            //             ))
            //         })
            //         .transpose()
            //         .map_err(|_| {
            //             AgentError::from(ClientError::invalid_request("Invalid banner URI"))
            //         })?;

            //     let profile_detailed = ProfileViewDetailed::new()
            //         .did(did.clone())
            //         .handle(handle.clone())
            //         .maybe_display_name(bsky_record.value.display_name.clone())
            //         .maybe_description(bsky_record.value.description.clone())
            //         .maybe_avatar(avatar)
            //         .maybe_banner(banner)
            //         .indexed_at(jacquard::types::string::Datetime::now())
            //         .maybe_created_at(bsky_record.value.created_at)
            //         .build();

            //     Ok((
            //         Some(bsky_uri.as_uri().clone().into_static()),
            //         ProfileDataView::new()
            //             .inner(ProfileDataViewInner::ProfileViewDetailed(Box::new(
            //                 profile_detailed,
            //             )))
            //             .build()
            //             .into_static(),
            //     ))
            // };

            // n0_future::future::or(
            //     weaver_future,
            //     n0_future::future::or(bsky_appview_future, bsky_future),
            // )
            // .await
            bsky_appview_future.await
        }
    }

    /// View an entry at a specific index with prev/next navigation
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
    ) -> impl Future<Output = Result<EntryView<'a>, WeaverError>> {
        async move {
            use jacquard::to_data;
            use weaver_api::sh_weaver::notebook::page::Page;

            let entry_uri = Page::uri(entry_ref.uri.clone())
                .map_err(|_| AgentError::from(ClientError::invalid_request("Invalid page URI")))?;
            let entry = self.fetch_record(&entry_uri).await?;

            let title = entry.value.title.clone();
            let tags = entry.value.tags.clone();

            Ok(EntryView::new()
                .cid(entry.cid.ok_or_else(|| {
                    AgentError::from(ClientError::invalid_request("Page missing CID"))
                })?)
                .uri(entry.uri)
                .indexed_at(jacquard::types::string::Datetime::now())
                .record(to_data(&entry.value).map_err(|_| {
                    AgentError::from(ClientError::invalid_request("Failed to serialize page"))
                })?)
                .maybe_tags(tags)
                .title(title)
                .authors(notebook.authors.clone())
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
            use weaver_api::com_atproto::repo::get_record::GetRecord;

            // Resolve DID and PDS from ident
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

            // Fetch the entry record
            let request = GetRecord::new()
                .repo(jacquard::types::ident::AtIdentifier::Did(repo_did.clone()))
                .collection(jacquard::types::nsid::Nsid::raw(entry::Entry::NSID))
                .rkey(RecordKey::any(rkey)?)
                .build();

            let response: Response<GetRecordResponse> = {
                let http_request = xrpc::build_http_request(&pds_url, &request, &self.opts().await)
                    .map_err(|e| AgentError::from(ClientError::transport(e)))?;

                let http_response = self
                    .send_http(http_request)
                    .await
                    .map_err(|e| AgentError::from(ClientError::transport(e)))?;

                xrpc::process_response(http_response)
            }
            .map_err(|e| AgentError::new(AgentErrorKind::Client, Some(e.into())))?;

            let record = response.into_output().map_err(|e| {
                AgentError::from(ClientError::invalid_request(format!(
                    "Failed to parse entry record: {}",
                    e
                )))
            })?;

            // Parse the entry value
            let entry_value: entry::Entry = jacquard::from_data(&record.value).map_err(|e| {
                AgentError::from(ClientError::invalid_request(format!(
                    "Failed to deserialize entry: {}",
                    e
                )))
            })?;

            // Build EntryView - without notebook authors, just the entry author
            let mut authors = Vec::new();
            let (profile_uri, profile_view) = self.hydrate_profile_view(&repo_did).await?;
            authors.push(
                AuthorListView::new()
                    .maybe_uri(profile_uri)
                    .record(profile_view)
                    .index(0)
                    .build(),
            );

            let entry_view = EntryView::new()
                .cid(record.cid.ok_or_else(|| {
                    AgentError::from(ClientError::invalid_request("Entry missing CID"))
                })?)
                .uri(record.uri)
                .indexed_at(jacquard::types::string::Datetime::now())
                .record(to_data(&entry_value).map_err(|_| {
                    AgentError::from(ClientError::invalid_request("Failed to serialize entry"))
                })?)
                .maybe_tags(entry_value.tags.clone())
                .title(entry_value.title.clone())
                .path(entry_value.path.clone())
                .authors(authors)
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
}

impl<T: AgentSession + IdentityResolver + XrpcExt> WeaverExt for T {}
