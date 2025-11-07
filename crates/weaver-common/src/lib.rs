//! Weaver common library - thin wrapper around jacquard with notebook-specific conveniences

pub mod constellation;
pub mod error;
pub mod view;
pub mod worker_rt;

// Re-export jacquard for convenience
pub use jacquard;
use jacquard::error::ClientError;
use jacquard::types::ident::AtIdentifier;
use jacquard::{CowStr, IntoStatic, xrpc};

pub use error::WeaverError;
use jacquard::types::tid::{Ticker, Tid};

use jacquard::bytes::Bytes;
use jacquard::client::{Agent, AgentError, AgentErrorKind, AgentSession, AgentSessionExt};
use jacquard::prelude::*;
use jacquard::types::blob::{BlobRef, MimeType};
use jacquard::types::string::{AtUri, Cid, Did, Handle, RecordKey};
use jacquard::xrpc::Response;
use mime_sniffer::MimeTypeSniffer;
use std::path::Path;
use std::sync::LazyLock;
use tokio::sync::Mutex;
use weaver_api::com_atproto::repo::get_record::GetRecordResponse;
use weaver_api::com_atproto::repo::strong_ref::StrongRef;
use weaver_api::sh_weaver::notebook::entry;
use weaver_api::sh_weaver::publish::blob::Blob as PublishedBlob;

static W_TICKER: LazyLock<Mutex<Ticker>> = LazyLock::new(|| Mutex::new(Ticker::new()));

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
//#[trait_variant::make(Send)]
pub trait WeaverExt: AgentSessionExt {
    /// Publish a notebook directory to the user's PDS
    ///
    /// Multi-step workflow:
    /// 1. Parse markdown files in directory
    /// 2. Extract and upload images/assets → BlobRefs
    /// 3. Transform markdown refs to point at uploaded blobs
    /// 4. Create entry records for each file
    /// 5. Create book record with entry refs
    ///
    /// Returns the AT-URI of the published book
    fn publish_notebook(
        &self,
        path: &Path,
    ) -> impl Future<Output = Result<PublishResult<'_>, WeaverError>>;

    /// Publish a blob to the user's PDS
    ///
    /// Multi-step workflow:
    /// 1. Upload blob to PDS
    /// 2. Create blob record with CID
    ///
    /// Returns the AT-URI of the published blob
    fn publish_blob<'a>(
        &self,
        blob: Bytes,
        url_path: &'a str,
        prev: Option<Tid>,
    ) -> impl Future<Output = Result<(StrongRef<'a>, PublishedBlob<'a>), WeaverError>>;

    fn confirm_record_ref(
        &self,
        uri: &AtUri<'_>,
    ) -> impl Future<Output = Result<StrongRef<'_>, WeaverError>>;

    /// Find or create a notebook by title, returning its URI and entry list
    ///
    /// If the notebook doesn't exist, creates it with the given DID as author.
    fn upsert_notebook(
        &self,
        title: &str,
        author_did: &Did<'_>,
    ) -> impl Future<Output = Result<(AtUri<'static>, Vec<StrongRef<'static>>), WeaverError>>;

    /// Find or create an entry within a notebook by title
    ///
    /// Multi-step workflow:
    /// 1. Find the notebook by title
    /// 2. Check notebook's entry_list for entry with matching title
    /// 3. If found: update the entry with new content
    /// 4. If not found: create new entry and append to notebook's entry_list
    ///
    /// Returns (entry_uri, was_created)
    fn upsert_entry(
        &self,
        notebook_title: &str,
        entry_title: &str,
        entry: entry::Entry<'_>,
    ) -> impl Future<Output = Result<(AtUri<'static>, bool), WeaverError>>;

    /// View functions - generic versions that work with any Agent

    /// Fetch a notebook and construct NotebookView with author profiles
    fn view_notebook(
        &self,
        uri: &AtUri<'_>,
    ) -> impl Future<Output = Result<(view::NotebookView<'static>, Vec<StrongRef<'static>>), WeaverError>>;

    /// Fetch an entry and construct EntryView
    fn fetch_entry_view<'a>(
        &self,
        notebook: &view::NotebookView<'a>,
        entry_ref: &StrongRef<'_>,
    ) -> impl Future<Output = Result<view::EntryView<'a>, WeaverError>>;

    /// Search for an entry by title within a notebook's entry list
    fn entry_by_title<'a>(
        &self,
        notebook: &view::NotebookView<'a>,
        entries: &[StrongRef<'_>],
        title: &str,
    ) -> impl Future<Output = Result<Option<(view::BookEntryView<'a>, entry::Entry<'a>)>, WeaverError>>;

    /// Search for a notebook by title for a given DID or handle
    fn notebook_by_title(
        &self,
        ident: &jacquard::types::ident::AtIdentifier<'_>,
        title: &str,
    ) -> impl Future<
        Output = Result<
            Option<(view::NotebookView<'static>, Vec<StrongRef<'static>>)>,
            WeaverError,
        >,
    >;
}

impl<A: AgentSession + IdentityResolver> WeaverExt for Agent<A> {
    async fn publish_notebook(&self, _path: &Path) -> Result<PublishResult<'_>, WeaverError> {
        // TODO: Implementation
        todo!("publish_notebook not yet implemented")
    }

    async fn publish_blob<'a>(
        &self,
        blob: Bytes,
        url_path: &'a str,
        prev: Option<Tid>,
    ) -> Result<(StrongRef<'a>, PublishedBlob<'a>), WeaverError> {
        let mime_type =
            MimeType::new_owned(blob.sniff_mime_type().unwrap_or("application/octet-stream"));

        let blob = self.upload_blob(blob, mime_type).await?;
        let publish_record = PublishedBlob::new()
            .path(url_path)
            .upload(BlobRef::Blob(blob))
            .build();
        let tid = W_TICKER.lock().await.next(prev);
        let record = self
            .create_record(publish_record.clone(), Some(RecordKey::any(tid.as_str())?))
            .await?;
        let strong_ref = StrongRef::new().uri(record.uri).cid(record.cid).build();

        Ok((strong_ref, publish_record))
    }

    async fn upsert_notebook(
        &self,
        title: &str,
        author_did: &Did<'_>,
    ) -> Result<(AtUri<'static>, Vec<StrongRef<'static>>), WeaverError> {
        use jacquard::types::collection::Collection;
        use jacquard::types::nsid::Nsid;
        use jacquard::xrpc::XrpcExt;
        use weaver_api::com_atproto::repo::list_records::ListRecords;
        use weaver_api::sh_weaver::notebook::book::Book;

        // Find the PDS for this DID
        let pds_url = self.pds_for_did(author_did).await.map_err(|e| {
            AgentError::from(ClientError::from(e).with_context("Failed to resolve PDS for DID"))
        })?;

        // Search for existing notebook with this title
        let resp = self
            .xrpc(pds_url)
            .send(
                &ListRecords::new()
                    .repo(author_did.clone())
                    .collection(Nsid::raw(Book::NSID))
                    .limit(100)
                    .build(),
            )
            .await
            .map_err(|e| AgentError::from(ClientError::from(e)))?;

        if let Ok(list) = resp.parse() {
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
        }

        // Notebook doesn't exist, create it
        use weaver_api::sh_weaver::actor::Author;
        let author = Author::new().did(author_did.clone()).build();
        let book = Book::new()
            .authors(vec![author])
            .entry_list(vec![])
            .maybe_title(Some(title.into()))
            .maybe_created_at(Some(jacquard::types::string::Datetime::now()))
            .build();

        let response = self.create_record(book, None).await?;
        Ok((response.uri, Vec::new()))
    }

    async fn upsert_entry(
        &self,
        notebook_title: &str,
        entry_title: &str,
        entry: entry::Entry<'_>,
    ) -> Result<(AtUri<'static>, bool), WeaverError> {
        // Get our own DID
        let (did, _) = self.info().await.ok_or_else(|| {
            AgentError::from(ClientError::invalid_request("No session info available"))
        })?;

        // Find or create notebook
        let (notebook_uri, entry_refs) = self.upsert_notebook(notebook_title, &did).await?;

        // Check if entry with this title exists in the notebook
        for entry_ref in &entry_refs {
            let existing = self
                .get_record::<entry::Entry>(&entry_ref.uri)
                .await
                .map_err(|e| AgentError::from(ClientError::from(e)))?;
            if let Ok(existing_entry) = existing.parse() {
                if existing_entry.value.title == entry_title {
                    // Update existing entry
                    self.update_record::<entry::Entry>(&entry_ref.uri, |e| {
                        e.content = entry.content.clone();
                        e.embeds = entry.embeds.clone();
                        e.tags = entry.tags.clone();
                    })
                    .await?;
                    return Ok((entry_ref.uri.clone().into_static(), false));
                }
            }
        }

        // Entry doesn't exist, create it
        let response = self.create_record(entry, None).await?;
        let entry_uri = response.uri.clone();

        // Add to notebook's entry_list
        use weaver_api::sh_weaver::notebook::book::Book;
        let new_ref = StrongRef::new().uri(response.uri).cid(response.cid).build();

        self.update_record::<Book>(&notebook_uri, |book| {
            book.entry_list.push(new_ref);
        })
        .await?;

        Ok((entry_uri, true))
    }

    async fn view_notebook(
        &self,
        uri: &AtUri<'_>,
    ) -> Result<(view::NotebookView<'static>, Vec<StrongRef<'static>>), WeaverError> {
        use jacquard::to_data;
        use weaver_api::app_bsky::actor::profile::Profile as BskyProfile;
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

        let mut authors = Vec::new();

        for (index, author) in notebook.value.authors.iter().enumerate() {
            let author_uri =
                BskyProfile::uri(format!("at://{}/app.bsky.actor.profile/self", author.did))
                    .map_err(|_| {
                        AgentError::from(ClientError::invalid_request("Invalid author profile URI"))
                    })?;
            let author_profile = self.fetch_record(&author_uri).await?;

            authors.push(
                AuthorListView::new()
                    .uri(author_uri.as_uri().clone())
                    .record(to_data(&author_profile).map_err(|_| {
                        AgentError::from(ClientError::invalid_request(
                            "Failed to serialize author profile",
                        ))
                    })?)
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
            view::NotebookView::new()
                .cid(notebook.cid.ok_or_else(|| {
                    AgentError::from(ClientError::invalid_request("Notebook missing CID"))
                })?)
                .uri(notebook.uri)
                .indexed_at(jacquard::types::string::Datetime::now())
                .maybe_title(title)
                .maybe_tags(tags)
                .authors(authors)
                .record(to_data(&notebook.value).map_err(|_| {
                    AgentError::from(ClientError::invalid_request("Failed to serialize notebook"))
                })?)
                .build(),
            entries,
        ))
    }

    async fn fetch_entry_view<'a>(
        &self,
        notebook: &view::NotebookView<'a>,
        entry_ref: &StrongRef<'_>,
    ) -> Result<view::EntryView<'a>, WeaverError> {
        use jacquard::to_data;
        use weaver_api::sh_weaver::notebook::entry::Entry;

        let entry_uri = Entry::uri(entry_ref.uri.clone())
            .map_err(|_| AgentError::from(ClientError::invalid_request("Invalid entry URI")))?;
        let entry = self.fetch_record(&entry_uri).await?;

        let title = entry.value.title.clone();
        let tags = entry.value.tags.clone();

        Ok(view::EntryView::new()
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
            .authors(notebook.authors.clone())
            .build())
    }

    async fn entry_by_title<'a>(
        &self,
        notebook: &view::NotebookView<'a>,
        entries: &[StrongRef<'_>],
        title: &str,
    ) -> Result<Option<(view::BookEntryView<'a>, entry::Entry<'a>)>, WeaverError> {
        use weaver_api::sh_weaver::notebook::BookEntryRef;
        use weaver_api::sh_weaver::notebook::entry::Entry;

        for (index, entry_ref) in entries.iter().enumerate() {
            let resp = self
                .get_record::<Entry>(&entry_ref.uri)
                .await
                .map_err(|e| AgentError::from(e))?;
            if let Ok(entry) = resp.parse() {
                if entry.value.title == title {
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

                    let book_entry_view = view::BookEntryView::new()
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

    async fn notebook_by_title(
        &self,
        ident: &jacquard::types::ident::AtIdentifier<'_>,
        title: &str,
    ) -> Result<Option<(view::NotebookView<'static>, Vec<StrongRef<'static>>)>, WeaverError> {
        use jacquard::to_data;
        use jacquard::types::collection::Collection;
        use jacquard::types::nsid::Nsid;
        use jacquard::xrpc::XrpcExt;
        use weaver_api::app_bsky::actor::profile::Profile as BskyProfile;
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
                    AgentError::from(ClientError::from(e).with_context("Failed to resolve handle"))
                })?
            }
        };

        // TODO: use the cursor to search through all records with this NSID for the repo
        let resp = self
            .xrpc(pds_url)
            .send(
                &ListRecords::new()
                    .repo(repo_did)
                    .collection(Nsid::raw(Book::NSID))
                    .limit(100)
                    .build(),
            )
            .await
            .map_err(|e| AgentError::from(ClientError::from(e)))?;

        if let Ok(list) = resp.parse() {
            for record in list.records {
                let notebook: Book = jacquard::from_data(&record.value).map_err(|_| {
                    AgentError::from(ClientError::invalid_request(
                        "Failed to parse notebook record",
                    ))
                })?;
                if let Some(book_title) = notebook.title
                    && book_title == title
                {
                    let tags = notebook.tags.clone();

                    let mut authors = Vec::new();

                    for (index, author) in notebook.authors.iter().enumerate() {
                        let author_uri = BskyProfile::uri(format!(
                            "at://{}/app.bsky.actor.profile/self",
                            author.did
                        ))
                        .map_err(|_| {
                            AgentError::from(ClientError::invalid_request(
                                "Invalid author profile URI",
                            ))
                        })?;
                        let author_profile = self.fetch_record(&author_uri).await?;

                        authors.push(
                            AuthorListView::new()
                                .uri(author_uri.as_uri().clone())
                                .record(to_data(&author_profile).map_err(|_| {
                                    AgentError::from(ClientError::invalid_request(
                                        "Failed to serialize author profile",
                                    ))
                                })?)
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
                        view::NotebookView::new()
                            .cid(record.cid)
                            .uri(record.uri)
                            .indexed_at(jacquard::types::string::Datetime::now())
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

    async fn confirm_record_ref(&self, uri: &AtUri<'_>) -> Result<StrongRef<'_>, WeaverError> {
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
                let pds = self.pds_for_did(did).await.map_err(|e| {
                    AgentError::from(
                        ClientError::from(e)
                            .with_context("DID document resolution failed during record retrieval"),
                    )
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

/// Result of publishing a notebook
#[derive(Debug, Clone)]
pub struct PublishResult<'a> {
    /// AT-URI of the published book
    pub uri: AtUri<'a>,
    /// CID of the book record
    pub cid: Cid<'a>,
    /// URIs of published entries
    pub entries: Vec<StrongRef<'a>>,
}

/// too many cows, so we have conversions
pub fn mcow_to_cow(cow: CowStr<'_>) -> std::borrow::Cow<'_, str> {
    match cow {
        CowStr::Borrowed(s) => std::borrow::Cow::Borrowed(s),
        CowStr::Owned(s) => std::borrow::Cow::Owned(s.to_string()),
    }
}

/// too many cows, so we have conversions
pub fn cow_to_mcow(cow: std::borrow::Cow<'_, str>) -> CowStr<'_> {
    match cow {
        std::borrow::Cow::Borrowed(s) => CowStr::Borrowed(s),
        std::borrow::Cow::Owned(s) => CowStr::Owned(s.into()),
    }
}

/// too many cows, so we have conversions
pub fn mdcow_to_cow(cow: markdown_weaver::CowStr<'_>) -> std::borrow::Cow<'_, str> {
    match cow {
        markdown_weaver::CowStr::Borrowed(s) => std::borrow::Cow::Borrowed(s),
        markdown_weaver::CowStr::Boxed(s) => std::borrow::Cow::Owned(s.to_string()),
        markdown_weaver::CowStr::Inlined(s) => std::borrow::Cow::Owned(s.as_ref().to_owned()),
    }
}

/// Utility: Generate CDN URL for avatar blob
pub fn avatar_cdn_url(did: &Did, cid: &Cid) -> String {
    format!(
        "https://cdn.bsky.app/img/avatar/plain/{}/{}",
        did.as_str(),
        cid
    )
}

/// Utility: Generate PDS URL for blob retrieval
pub fn blob_url(did: &Did, pds: &str, cid: &Cid) -> String {
    format!(
        "https://{}/xrpc/com.atproto.repo.getBlob?did={}&cid={}",
        pds,
        did.as_str(),
        cid
    )
}

pub fn match_identifier(maybe_identifier: &str) -> Option<&str> {
    if jacquard::types::string::AtIdentifier::new(maybe_identifier).is_ok() {
        Some(maybe_identifier)
    } else {
        None
    }
}

pub fn match_nsid(maybe_nsid: &str) -> Option<&str> {
    if jacquard::types::string::Nsid::new(maybe_nsid).is_ok() {
        Some(maybe_nsid)
    } else {
        None
    }
}

/// Convert an ATURI to a HTTP URL
/// Currently has some failure modes and should restrict the NSIDs to a known subset
pub fn aturi_to_http<'s>(aturi: &'s str, appview: &'s str) -> Option<markdown_weaver::CowStr<'s>> {
    use markdown_weaver::CowStr;

    if aturi.starts_with("at://") {
        let rest = aturi.strip_prefix("at://").unwrap();
        let mut split = rest.splitn(2, '/');
        let maybe_identifier = split.next()?;
        let maybe_nsid = split.next()?;
        let maybe_rkey = split.next()?;

        // https://atproto.com/specs/handle#handle-identifier-syntax
        let identifier = match_identifier(maybe_identifier)?;

        let nsid = if let Some(nsid) = match_nsid(maybe_nsid) {
            // Last part of the nsid is generally the middle component of the URL
            // TODO: check for bsky ones specifically, because those are the ones where this is valid
            nsid.rsplitn(1, '.').next()?
        } else {
            return None;
        };
        Some(CowStr::Boxed(
            format!(
                "https://{}/profile/{}/{}/{}",
                appview, identifier, nsid, maybe_rkey
            )
            .into_boxed_str(),
        ))
    } else {
        Some(CowStr::Borrowed(aturi))
    }
}

pub enum LinkUri<'a> {
    AtRecord(AtUri<'a>),
    AtIdent(Did<'a>, Handle<'a>),
    Web(jacquard::url::Url),
    Path(markdown_weaver::CowStr<'a>),
    Heading(markdown_weaver::CowStr<'a>),
    Footnote(markdown_weaver::CowStr<'a>),
}

impl<'a> LinkUri<'a> {
    pub async fn resolve<A>(dest_url: &'a str, agent: &Agent<A>) -> LinkUri<'a>
    where
        A: AgentSession + IdentityResolver,
    {
        if dest_url.starts_with('@') {
            if let Ok(handle) = Handle::new(dest_url) {
                if let Ok(did) = agent.resolve_handle(&handle).await {
                    return Self::AtIdent(did, handle);
                }
            }
        } else if dest_url.starts_with("did:") {
            if let Ok(did) = Did::new(dest_url) {
                if let Ok(doc) = agent.resolve_did_doc(&did).await {
                    if let Ok(doc) = doc.parse_validated() {
                        if let Some(handle) = doc.handles().first() {
                            return Self::AtIdent(did, handle.clone());
                        }
                    }
                }
            }
        } else if dest_url.starts_with('#') {
            // local fragment
            return Self::Heading(markdown_weaver::CowStr::Borrowed(dest_url));
        } else if dest_url.starts_with('^') {
            // footnote
            return Self::Footnote(markdown_weaver::CowStr::Borrowed(dest_url));
        }
        if let Ok(url) = jacquard::url::Url::parse(dest_url) {
            if let Some(uri) = jacquard::richtext::extract_at_uri_from_url(
                url.as_str(),
                jacquard::richtext::DEFAULT_EMBED_DOMAINS,
            ) {
                if let AtIdentifier::Handle(handle) = uri.authority() {
                    if let Ok(did) = agent.resolve_handle(handle).await {
                        let mut aturi = format!("at://{did}");
                        if let Some(collection) = uri.collection() {
                            aturi.push_str(&format!("/{}", collection));
                            if let Some(record) = uri.rkey() {
                                aturi.push_str(&format!("/{}", record.0));
                            }
                        }
                        if let Ok(aturi) = AtUri::new_owned(aturi) {
                            return Self::AtRecord(aturi);
                        }
                    }
                    return Self::AtRecord(uri);
                } else {
                    return Self::AtRecord(uri);
                }
            } else if url.scheme() == "http" || url.scheme() == "https" {
                return Self::Web(url);
            }
        }

        LinkUri::Path(markdown_weaver::CowStr::Borrowed(dest_url))
    }
}
