use crate::auth::AuthStore;
use crate::cache_impl;
use dioxus::Result;
use jacquard::AuthorizationToken;
use jacquard::CowStr;
use jacquard::IntoStatic;
use jacquard::client::Agent;
use jacquard::client::AgentError;
use jacquard::client::AgentKind;
use jacquard::error::ClientError;
use jacquard::error::XrpcResult;
use jacquard::identity::JacquardResolver;
use jacquard::identity::lexicon_resolver::{
    LexiconResolutionError, LexiconSchemaResolver, ResolvedLexiconSchema,
};
use jacquard::identity::resolver::DidDocResponse;
use jacquard::identity::resolver::IdentityError;
use jacquard::identity::resolver::ResolverOptions;
use jacquard::oauth::client::OAuthClient;
use jacquard::oauth::client::OAuthSession;
use jacquard::prelude::*;
use jacquard::types::string::Did;
use jacquard::types::string::Handle;
use jacquard::types::string::Nsid;
use jacquard::xrpc::XrpcResponse;
use jacquard::xrpc::*;
use jacquard::{
    smol_str::{SmolStr, format_smolstr},
    types::aturi::AtUri,
    types::ident::AtIdentifier,
};
use serde::{Deserialize, Serialize};
use std::future::Future;
use std::{sync::Arc, time::Duration};
use tokio::sync::RwLock;
use weaver_api::app_bsky::actor::get_profile::GetProfile;
use weaver_api::app_bsky::actor::profile::Profile as BskyProfile;
use weaver_api::sh_weaver::actor::ProfileDataViewInner;
use weaver_api::sh_weaver::notebook::EntryView;
use weaver_api::{
    com_atproto::repo::strong_ref::StrongRef,
    sh_weaver::{
        actor::ProfileDataView,
        notebook::{BookEntryView, NotebookView, entry::Entry},
    },
};
use weaver_common::WeaverError;
use weaver_common::WeaverExt;

#[derive(Debug, Clone, Deserialize, Serialize)]
struct UfosRecord {
    collection: String,
    did: String,
    record: serde_json::Value,
    rkey: String,
    time_us: u64,
}

/// Data for a standalone entry (may or may not have notebook context)
#[derive(Clone, PartialEq)]
pub struct StandaloneEntryData {
    pub entry: Entry<'static>,
    pub entry_view: EntryView<'static>,
    /// Present if entry is in exactly one notebook
    pub notebook_context: Option<NotebookContext>,
}

/// Notebook context for an entry
#[derive(Clone, PartialEq)]
pub struct NotebookContext {
    pub notebook: NotebookView<'static>,
    /// BookEntryView with prev/next navigation
    pub book_entry_view: BookEntryView<'static>,
}

pub struct Client {
    pub oauth_client: Arc<OAuthClient<JacquardResolver, AuthStore>>,
    pub session: RwLock<Option<Arc<Agent<OAuthSession<JacquardResolver, AuthStore>>>>>,
}

impl Client {
    pub fn new(oauth_client: OAuthClient<JacquardResolver, AuthStore>) -> Self {
        Self {
            oauth_client: Arc::new(oauth_client),
            session: RwLock::new(None),
        }
    }
}

impl HttpClient for Client {
    type Error = IdentityError;

    #[cfg(not(target_arch = "wasm32"))]
    fn send_http(
        &self,
        request: http::Request<Vec<u8>>,
    ) -> impl Future<Output = core::result::Result<http::Response<Vec<u8>>, Self::Error>> + Send
    {
        self.oauth_client.send_http(request)
    }

    #[cfg(target_arch = "wasm32")]
    fn send_http(
        &self,
        request: http::Request<Vec<u8>>,
    ) -> impl Future<Output = core::result::Result<http::Response<Vec<u8>>, Self::Error>> {
        self.oauth_client.send_http(request)
    }
}

impl XrpcClient for Client {
    #[doc = " Get the base URI for the client."]
    fn base_uri(&self) -> impl Future<Output = CowStr<'static>> + Send {
        async {
            let guard = self.session.read().await;
            if let Some(session) = guard.clone() {
                session.base_uri().await
            } else {
                // When unauthenticated, use index if configured
                #[cfg(feature = "use-index")]
                if !crate::env::WEAVER_INDEXER_URL.is_empty() {
                    return CowStr::from(crate::env::WEAVER_INDEXER_URL);
                }
                self.oauth_client.base_uri().await
            }
        }
    }

    #[doc = " Send an XRPC request and parse the response"]
    #[cfg(not(target_arch = "wasm32"))]
    fn send<R>(&self, request: R) -> impl Future<Output = XrpcResult<XrpcResponse<R>>> + Send
    where
        R: XrpcRequest + Send + Sync,
        <R as XrpcRequest>::Response: Send + Sync,
        Self: Sync,
    {
        async {
            let guard = self.session.read().await;
            if let Some(session) = guard.clone() {
                session.send(request).await
            } else {
                self.oauth_client.send(request).await
            }
        }
    }

    #[doc = " Send an XRPC request and parse the response"]
    #[cfg(not(target_arch = "wasm32"))]
    fn send_with_opts<R>(
        &self,
        request: R,
        opts: CallOptions<'_>,
    ) -> impl Future<Output = XrpcResult<XrpcResponse<R>>> + Send
    where
        R: XrpcRequest + Send + Sync,
        <R as XrpcRequest>::Response: Send + Sync,
        Self: Sync,
    {
        async {
            let guard = self.session.read().await;
            if let Some(session) = guard.clone() {
                session.send_with_opts(request, opts).await
            } else {
                self.oauth_client.send_with_opts(request, opts).await
            }
        }
    }

    #[doc = " Send an XRPC request and parse the response"]
    #[cfg(target_arch = "wasm32")]
    fn send<R>(&self, request: R) -> impl Future<Output = XrpcResult<XrpcResponse<R>>>
    where
        R: XrpcRequest + Send + Sync,
        <R as XrpcRequest>::Response: Send + Sync,
    {
        async {
            let guard = self.session.read().await;
            if let Some(session) = guard.clone() {
                session.send(request).await
            } else {
                self.oauth_client.send(request).await
            }
        }
    }

    #[doc = " Send an XRPC request and parse the response"]
    #[cfg(target_arch = "wasm32")]
    fn send_with_opts<R>(
        &self,
        request: R,
        opts: CallOptions<'_>,
    ) -> impl Future<Output = XrpcResult<XrpcResponse<R>>>
    where
        R: XrpcRequest + Send + Sync,
        <R as XrpcRequest>::Response: Send + Sync,
    {
        async {
            let guard = self.session.read().await;
            if let Some(session) = guard.clone() {
                session.send_with_opts(request, opts).await
            } else {
                self.oauth_client.send_with_opts(request, opts).await
            }
        }
    }

    #[doc = " Set the base URI for the client."]
    fn set_base_uri(&self, url: jacquard::url::Url) -> impl Future<Output = ()> + Send {
        async {
            let guard = self.session.read().await;
            if let Some(session) = guard.clone() {
                session.set_base_uri(url).await
            } else {
                self.oauth_client.set_base_uri(url).await
            }
        }
    }

    #[doc = " Get the call options for the client."]
    fn opts(&self) -> impl Future<Output = CallOptions<'_>> + Send {
        async {
            let guard = self.session.read().await;
            if let Some(session) = guard.clone() {
                session.opts().await.into_static()
            } else {
                self.oauth_client.opts().await
            }
        }
    }

    #[doc = " Set the call options for the client."]
    fn set_opts(&self, opts: CallOptions) -> impl Future<Output = ()> + Send {
        async {
            let guard = self.session.read().await;
            if let Some(session) = guard.clone() {
                session.set_opts(opts).await
            } else {
                self.oauth_client.set_opts(opts).await
            }
        }
    }
}

impl IdentityResolver for Client {
    #[doc = " Access options for validation decisions in default methods"]
    fn options(&self) -> &ResolverOptions {
        self.oauth_client.client.options()
    }

    #[doc = " Resolve handle"]
    #[cfg(not(target_arch = "wasm32"))]
    fn resolve_handle(
        &self,
        handle: &Handle<'_>,
    ) -> impl Future<Output = core::result::Result<Did<'static>, IdentityError>> + Send
    where
        Self: Sync,
    {
        self.oauth_client.client.resolve_handle(handle)
    }

    #[doc = " Resolve DID document"]
    #[cfg(not(target_arch = "wasm32"))]
    fn resolve_did_doc(
        &self,
        did: &Did<'_>,
    ) -> impl Future<Output = core::result::Result<DidDocResponse, IdentityError>> + Send
    where
        Self: Sync,
    {
        self.oauth_client.client.resolve_did_doc(did)
    }

    #[doc = " Resolve handle"]
    #[cfg(target_arch = "wasm32")]
    fn resolve_handle(
        &self,
        handle: &Handle<'_>,
    ) -> impl Future<Output = core::result::Result<Did<'static>, IdentityError>> {
        self.oauth_client.client.resolve_handle(handle)
    }

    #[doc = " Resolve DID document"]
    #[cfg(target_arch = "wasm32")]
    fn resolve_did_doc(
        &self,
        did: &Did<'_>,
    ) -> impl Future<Output = core::result::Result<DidDocResponse, IdentityError>> {
        self.oauth_client.client.resolve_did_doc(did)
    }
}

impl LexiconSchemaResolver for Client {
    #[cfg(not(target_arch = "wasm32"))]
    async fn resolve_lexicon_schema(
        &self,
        nsid: &Nsid<'_>,
    ) -> std::result::Result<ResolvedLexiconSchema<'static>, LexiconResolutionError> {
        self.oauth_client.client.resolve_lexicon_schema(nsid).await
    }

    #[cfg(target_arch = "wasm32")]
    async fn resolve_lexicon_schema(
        &self,
        nsid: &Nsid<'_>,
    ) -> std::result::Result<ResolvedLexiconSchema<'static>, LexiconResolutionError> {
        self.oauth_client.client.resolve_lexicon_schema(nsid).await
    }
}

impl AgentSession for Client {
    #[doc = " Identify the kind of session."]
    fn session_kind(&self) -> AgentKind {
        self.oauth_client.session_kind()
    }

    #[doc = " Return current DID and an optional session id (always Some for OAuth)."]
    async fn session_info(&self) -> Option<(Did<'static>, Option<CowStr<'static>>)> {
        let guard = self.session.read().await;
        if let Some(session) = guard.clone() {
            session.info().await
        } else {
            None
        }
    }

    #[doc = " Current base endpoint."]
    async fn endpoint(&self) -> CowStr<'static> {
        let guard = self.session.read().await;
        if let Some(session) = guard.clone() {
            session.endpoint().await
        } else {
            self.oauth_client.endpoint().await
        }
    }

    #[doc = " Override per-session call options."]
    async fn set_options<'a>(&'a self, opts: CallOptions<'a>) {
        let guard = self.session.read().await;
        if let Some(session) = guard.clone() {
            session.set_options(opts).await
        } else {
            self.oauth_client.set_options(opts).await
        }
    }

    #[doc = " Refresh the session and return a fresh AuthorizationToken."]
    async fn refresh(&self) -> XrpcResult<AuthorizationToken<'static>> {
        let guard = self.session.read().await;
        if let Some(session) = guard.clone() {
            session.refresh().await
        } else {
            Err(ClientError::auth(
                jacquard::error::AuthError::NotAuthenticated,
            ))
        }
    }
}

#[derive(Clone)]
pub struct Fetcher {
    pub client: Arc<Client>,
    #[cfg(feature = "server")]
    book_cache: cache_impl::Cache<
        (AtIdentifier<'static>, SmolStr),
        Arc<(NotebookView<'static>, Vec<StrongRef<'static>>)>,
    >,
    /// Maps notebook title OR path to ident (book_cache accepts either as key)
    #[cfg(feature = "server")]
    notebook_key_cache: cache_impl::Cache<SmolStr, AtIdentifier<'static>>,
    #[cfg(feature = "server")]
    entry_cache: cache_impl::Cache<
        (AtIdentifier<'static>, SmolStr),
        Arc<(BookEntryView<'static>, Entry<'static>)>,
    >,
    #[cfg(feature = "server")]
    profile_cache: cache_impl::Cache<AtIdentifier<'static>, Arc<ProfileDataView<'static>>>,
    #[cfg(feature = "server")]
    standalone_entry_cache:
        cache_impl::Cache<(AtIdentifier<'static>, SmolStr), Arc<StandaloneEntryData>>,
}

impl Fetcher {
    pub fn new(client: OAuthClient<JacquardResolver, AuthStore>) -> Self {
        // Set indexer URL for unauthenticated requests
        #[cfg(feature = "use-index")]
        if !crate::env::WEAVER_INDEXER_URL.is_empty() {
            if let Ok(url) = jacquard::url::Url::parse(crate::env::WEAVER_INDEXER_URL) {
                if let Ok(mut guard) = client.endpoint.try_write() {
                    use jacquard::cowstr::ToCowStr;

                    *guard = Some(url.to_cowstr().into_static());
                }
            }
        }

        Self {
            client: Arc::new(Client::new(client)),
            #[cfg(feature = "server")]
            book_cache: cache_impl::new_cache(100, std::time::Duration::from_secs(30)),
            #[cfg(feature = "server")]
            notebook_key_cache: cache_impl::new_cache(500, std::time::Duration::from_secs(30)),
            #[cfg(feature = "server")]
            entry_cache: cache_impl::new_cache(100, std::time::Duration::from_secs(30)),
            #[cfg(feature = "server")]
            profile_cache: cache_impl::new_cache(100, std::time::Duration::from_secs(1800)),
            #[cfg(feature = "server")]
            standalone_entry_cache: cache_impl::new_cache(100, std::time::Duration::from_secs(30)),
        }
    }

    pub async fn upgrade_to_authenticated(
        &self,
        session: OAuthSession<JacquardResolver, crate::auth::AuthStore>,
    ) {
        let agent = Arc::new(Agent::new(session));

        // When use-index is enabled, set the atproto_proxy header for service proxying
        #[cfg(feature = "use-index")]
        if !crate::env::WEAVER_INDEXER_DID.is_empty() {
            let proxy_value = format!("{}#atproto_index", crate::env::WEAVER_INDEXER_DID);
            let mut opts = agent.opts().await;
            opts.atproto_proxy = Some(CowStr::from(proxy_value));
            agent.set_opts(opts).await;
        }

        let mut session_slot = self.client.session.write().await;
        *session_slot = Some(agent);
    }

    pub async fn downgrade_to_unauthenticated(&self) {
        let mut session_slot = self.client.session.write().await;
        if let Some(session) = session_slot.take() {
            session.inner().logout().await.ok();
        }
    }

    #[allow(dead_code)]
    pub async fn current_did(&self) -> Option<Did<'static>> {
        let session_slot = self.client.session.read().await;
        if let Some(session) = session_slot.as_ref() {
            session.info().await.map(|(d, _)| d)
        } else {
            None
        }
    }

    pub fn get_client(&self) -> Arc<Client> {
        self.client.clone()
    }

    pub async fn get_notebook(
        &self,
        ident: AtIdentifier<'static>,
        title: SmolStr,
    ) -> Result<Option<Arc<(NotebookView<'static>, Vec<StrongRef<'static>>)>>> {
        #[cfg(feature = "server")]
        if let Some(cached) = cache_impl::get(&self.book_cache, &(ident.clone(), title.clone())) {
            return Ok(Some(cached));
        }

        let client = self.get_client();
        if let Some((notebook, entries)) = client
            .notebook_by_title(&ident, &title)
            .await
            .map_err(|e| dioxus::CapturedError::from_display(e))?
        {
            let stored = Arc::new((notebook, entries));
            #[cfg(feature = "server")]
            {
                // Cache by title
                cache_impl::insert(&self.notebook_key_cache, title.clone(), ident.clone());
                cache_impl::insert(&self.book_cache, (ident.clone(), title), stored.clone());
                // Also cache by path if available
                if let Some(path) = stored.0.path.as_ref() {
                    let path: SmolStr = path.as_ref().into();
                    cache_impl::insert(&self.notebook_key_cache, path.clone(), ident.clone());
                    cache_impl::insert(&self.book_cache, (ident, path), stored.clone());
                }
            }
            Ok(Some(stored))
        } else {
            Err(dioxus::CapturedError::from_display("Notebook not found"))
        }
    }

    /// Get notebook by title or path (for image resolution without knowing owner).
    /// Checks notebook_key_cache first, falls back to UFOS discovery.
    #[cfg(feature = "server")]
    pub async fn get_notebook_by_key(
        &self,
        key: &str,
    ) -> Result<Option<Arc<(NotebookView<'static>, Vec<StrongRef<'static>>)>>> {
        let key: SmolStr = key.into();

        // Check cache first (key could be title or path)
        if let Some(ident) = cache_impl::get(&self.notebook_key_cache, &key) {
            return self.get_notebook(ident, key).await;
        }

        // Fallback: query UFOS and populate caches
        let notebooks = self.fetch_notebooks_from_ufos().await?;
        Ok(notebooks.into_iter().find(|arc| {
            let (view, _) = arc.as_ref();
            view.title.as_deref() == Some(key.as_str())
                || view.path.as_deref() == Some(key.as_str())
        }))
    }

    pub async fn get_entry(
        &self,
        ident: AtIdentifier<'static>,
        book_title: SmolStr,
        entry_title: SmolStr,
    ) -> Result<Option<Arc<(BookEntryView<'static>, Entry<'static>)>>> {
        #[cfg(feature = "server")]
        if let Some(cached) =
            cache_impl::get(&self.entry_cache, &(ident.clone(), entry_title.clone()))
        {
            return Ok(Some(cached));
        }

        if let Some(result) = self.get_notebook(ident.clone(), book_title).await? {
            let (notebook, entries) = result.as_ref();
            let client = self.get_client();
            if let Some(entry) = client
                .entry_by_title(notebook, entries.as_ref(), &entry_title)
                .await
                .map_err(|e| dioxus::CapturedError::from_display(e))?
            {
                let stored = Arc::new(entry);
                #[cfg(feature = "server")]
                cache_impl::insert(&self.entry_cache, (ident, entry_title), stored.clone());
                Ok(Some(stored))
            } else {
                Err(dioxus::CapturedError::from_display("Entry not found"))
            }
        } else {
            Err(dioxus::CapturedError::from_display("Notebook not found"))
        }
    }

    #[cfg(feature = "use-index")]
    pub async fn fetch_notebooks_from_ufos(
        &self,
    ) -> Result<Vec<Arc<(NotebookView<'static>, Vec<StrongRef<'static>>)>>> {
        use weaver_api::sh_weaver::notebook::book::Book;
        use weaver_api::sh_weaver::notebook::get_notebook_feed::GetNotebookFeed;

        let client = self.get_client();

        let resp = client
            .send(GetNotebookFeed::new().limit(100).build())
            .await
            .map_err(|e| dioxus::CapturedError::from_display(e))?;

        let output = resp
            .into_output()
            .map_err(|e| dioxus::CapturedError::from_display(e))?;

        let mut notebooks = Vec::new();

        for notebook in output.notebooks {
            // Extract entry_list from the record
            let book: Book = jacquard::from_data(&notebook.record)
                .map_err(|e| dioxus::CapturedError::from_display(e))?;
            let book = book.into_static();

            let entries: Vec<StrongRef<'static>> = book
                .entry_list
                .into_iter()
                .map(IntoStatic::into_static)
                .collect();

            let ident = notebook.uri.authority().clone().into_static();
            let title = notebook
                .title
                .as_ref()
                .map(|t| SmolStr::new(t.as_ref()))
                .unwrap_or_else(|| SmolStr::new("Untitled"));

            let result = Arc::new((notebook.into_static(), entries));
            #[cfg(feature = "server")]
            {
                cache_impl::insert(&self.notebook_key_cache, title.clone(), ident.clone());
                cache_impl::insert(&self.book_cache, (ident.clone(), title), result.clone());
                if let Some(path) = result.0.path.as_ref() {
                    let path: SmolStr = path.as_ref().into();
                    cache_impl::insert(&self.notebook_key_cache, path.clone(), ident.clone());
                    cache_impl::insert(&self.book_cache, (ident, path), result.clone());
                }
            }
            notebooks.push(result);
        }

        Ok(notebooks)
    }

    #[cfg(not(feature = "use-index"))]
    pub async fn fetch_notebooks_from_ufos(
        &self,
    ) -> Result<Vec<Arc<(NotebookView<'static>, Vec<StrongRef<'static>>)>>> {
        use jacquard::{IntoStatic, types::aturi::AtUri};

        let url = "https://ufos-api.microcosm.blue/records?collection=sh.weaver.notebook.book";
        let response = reqwest::get(url)
            .await
            .map_err(|e| dioxus::CapturedError::from_display(e))?;

        let records: Vec<UfosRecord> = response
            .json()
            .await
            .map_err(|e| dioxus::CapturedError::from_display(e))?;

        let mut notebooks = Vec::new();
        let client = self.get_client();

        for ufos_record in records {
            // Construct URI
            let uri_str = format_smolstr!(
                "at://{}/{}/{}",
                ufos_record.did,
                ufos_record.collection,
                ufos_record.rkey
            );
            let uri = AtUri::new_owned(uri_str).map_err(|e| {
                dioxus::CapturedError::from_display(format_smolstr!("Invalid URI: {}", e).as_str())
            })?;
            match client.view_notebook(&uri).await {
                Ok((notebook, entries)) => {
                    let ident = uri.authority().clone().into_static();
                    let title = notebook
                        .title
                        .as_ref()
                        .map(|t| SmolStr::new(t.as_ref()))
                        .unwrap_or_else(|| SmolStr::new("Untitled"));

                    let result = Arc::new((notebook, entries));
                    #[cfg(feature = "server")]
                    {
                        // Cache by title
                        cache_impl::insert(&self.notebook_key_cache, title.clone(), ident.clone());
                        cache_impl::insert(
                            &self.book_cache,
                            (ident.clone(), title),
                            result.clone(),
                        );
                        // Also cache by path if available
                        if let Some(path) = result.0.path.as_ref() {
                            let path: SmolStr = path.as_ref().into();
                            cache_impl::insert(
                                &self.notebook_key_cache,
                                path.clone(),
                                ident.clone(),
                            );
                            cache_impl::insert(&self.book_cache, (ident, path), result.clone());
                        }
                    }
                    notebooks.push(result);
                }
                Err(_) => continue, // Skip notebooks that fail to load
            }
        }

        Ok(notebooks)
    }

    /// Fetch entries from index feed (reverse chronological)
    #[cfg(feature = "use-index")]
    pub async fn fetch_entries_from_ufos(
        &self,
    ) -> Result<Vec<Arc<(EntryView<'static>, Entry<'static>, u64)>>> {
        use jacquard::IntoStatic;
        use weaver_api::sh_weaver::notebook::entry::Entry;
        use weaver_api::sh_weaver::notebook::get_entry_feed::GetEntryFeed;

        let client = self.get_client();

        let resp = client
            .send(GetEntryFeed::new().limit(100).build())
            .await
            .map_err(|e| dioxus::CapturedError::from_display(e))?;

        let output = resp
            .into_output()
            .map_err(|e| dioxus::CapturedError::from_display(e))?;

        let mut entries = Vec::new();

        for feed_entry in output.feed {
            let entry_view = feed_entry.entry;
            // indexed_at is ISO datetime, parse to get millisecond timestamp
            let timestamp = chrono::DateTime::parse_from_rfc3339(entry_view.indexed_at.as_str())
                .map(|dt| dt.timestamp_millis() as u64)
                .unwrap_or(0);

            let entry: Entry = jacquard::from_data(&entry_view.record)
                .map_err(|e| dioxus::CapturedError::from_display(e))?;
            let entry = entry.into_static();

            entries.push(Arc::new((entry_view.into_static(), entry, timestamp)));
        }

        Ok(entries)
    }

    /// Fetch entries from UFOS discovery service (reverse chronological)
    #[cfg(not(feature = "use-index"))]
    pub async fn fetch_entries_from_ufos(
        &self,
    ) -> Result<Vec<Arc<(EntryView<'static>, Entry<'static>, u64)>>> {
        use jacquard::{IntoStatic, types::aturi::AtUri, types::ident::AtIdentifier};

        let url = "https://ufos-api.microcosm.blue/records?collection=sh.weaver.notebook.entry";

        let response = reqwest::get(url).await.map_err(|e| {
            tracing::error!("[fetch_entries_from_ufos] request failed: {:?}", e);
            dioxus::CapturedError::from_display(e)
        })?;

        let mut records: Vec<UfosRecord> = response.json().await.map_err(|e| {
            tracing::error!("[fetch_entries_from_ufos] json parse failed: {:?}", e);
            dioxus::CapturedError::from_display(e)
        })?;
        records.sort_by(|a, b| b.time_us.cmp(&a.time_us));

        let mut entries = Vec::new();
        let client = self.get_client();

        for ufos_record in records {
            let did = match Did::new(&ufos_record.did) {
                Ok(d) => d.into_static(),
                Err(e) => {
                    tracing::warn!(
                        "[fetch_entries_from_ufos] invalid DID {}: {:?}",
                        ufos_record.did,
                        e
                    );
                    continue;
                }
            };
            let ident = AtIdentifier::Did(did);
            match client.fetch_entry_by_rkey(&ident, &ufos_record.rkey).await {
                Ok((entry_view, entry)) => {
                    entries.push(Arc::new((
                        entry_view.into_static(),
                        entry.into_static(),
                        ufos_record.time_us,
                    )));
                }
                Err(e) => {
                    tracing::warn!(
                        "[fetch_entries_from_ufos] failed to load entry {}: {:?}",
                        ufos_record.rkey,
                        e
                    );
                    continue;
                }
            }
        }

        Ok(entries)
    }

    #[cfg(feature = "use-index")]
    pub async fn fetch_notebooks_for_did(
        &self,
        ident: &AtIdentifier<'_>,
    ) -> Result<Vec<Arc<(NotebookView<'static>, Vec<StrongRef<'static>>)>>> {
        use weaver_api::sh_weaver::actor::get_actor_notebooks::GetActorNotebooks;
        use weaver_api::sh_weaver::notebook::book::Book;

        let client = self.get_client();

        let resp = client
            .send(
                GetActorNotebooks::new()
                    .actor(ident.clone())
                    .limit(100)
                    .build(),
            )
            .await
            .map_err(|e| dioxus::CapturedError::from_display(e))?;

        let output = resp
            .into_output()
            .map_err(|e| dioxus::CapturedError::from_display(e))?;

        let mut notebooks = Vec::new();

        for notebook in output.notebooks {
            // Extract entry_list from the record
            let book: Book = jacquard::from_data(&notebook.record)
                .map_err(|e| dioxus::CapturedError::from_display(e))?;
            let book = book.into_static();

            let entries: Vec<StrongRef<'static>> = book
                .entry_list
                .into_iter()
                .map(IntoStatic::into_static)
                .collect();

            let ident_static = notebook.uri.authority().clone().into_static();
            let title = notebook
                .title
                .as_ref()
                .map(|t| SmolStr::new(t.as_ref()))
                .unwrap_or_else(|| SmolStr::new("Untitled"));

            let result = Arc::new((notebook.into_static(), entries));
            #[cfg(feature = "server")]
            {
                cache_impl::insert(
                    &self.notebook_key_cache,
                    title.clone(),
                    ident_static.clone(),
                );
                cache_impl::insert(
                    &self.book_cache,
                    (ident_static.clone(), title),
                    result.clone(),
                );
                if let Some(path) = result.0.path.as_ref() {
                    let path: SmolStr = path.as_ref().into();
                    cache_impl::insert(
                        &self.notebook_key_cache,
                        path.clone(),
                        ident_static.clone(),
                    );
                    cache_impl::insert(&self.book_cache, (ident_static, path), result.clone());
                }
            }
            notebooks.push(result);
        }

        Ok(notebooks)
    }

    #[cfg(not(feature = "use-index"))]
    pub async fn fetch_notebooks_for_did(
        &self,
        ident: &AtIdentifier<'_>,
    ) -> Result<Vec<Arc<(NotebookView<'static>, Vec<StrongRef<'static>>)>>> {
        use jacquard::{
            IntoStatic,
            types::{collection::Collection, nsid::Nsid},
            xrpc::XrpcExt,
        };
        use weaver_api::{
            com_atproto::repo::list_records::ListRecords, sh_weaver::notebook::book::Book,
        };

        let client = self.get_client();

        // Resolve DID and PDS
        let (repo_did, pds_url) = match ident {
            AtIdentifier::Did(did) => {
                let pds = client
                    .pds_for_did(did)
                    .await
                    .map_err(|e| dioxus::CapturedError::from_display(e))?;
                (did.clone(), pds)
            }
            AtIdentifier::Handle(handle) => client
                .pds_for_handle(handle)
                .await
                .map_err(|e| dioxus::CapturedError::from_display(e))?,
        };

        // Fetch all notebook records for this repo
        tracing::info!(
            "fetch_notebooks_for_did: pds_url={}, repo_did={}",
            pds_url,
            repo_did
        );

        let resp = client
            .xrpc(pds_url.clone())
            .send(
                &ListRecords::new()
                    .repo(repo_did)
                    .collection(Nsid::raw(Book::NSID))
                    .limit(100)
                    .build(),
            )
            .await
            .map_err(|e| {
                tracing::error!(
                    "fetch_notebooks_for_did: xrpc failed: {} pds url {}",
                    e,
                    pds_url
                );
                dioxus::CapturedError::from_display(e)
            })?;

        let mut notebooks = Vec::new();

        if let Ok(list) = resp.parse() {
            for record in list.records {
                // View the notebook (which hydrates authors)
                match client.view_notebook(&record.uri).await {
                    Ok((notebook, entries)) => {
                        let ident = record.uri.authority().clone().into_static();
                        let title = notebook
                            .title
                            .as_ref()
                            .map(|t| SmolStr::new(t.as_ref()))
                            .unwrap_or_else(|| SmolStr::new("Untitled"));

                        let result = Arc::new((notebook, entries));
                        #[cfg(feature = "server")]
                        {
                            // Cache by title
                            cache_impl::insert(
                                &self.notebook_key_cache,
                                title.clone(),
                                ident.clone(),
                            );
                            cache_impl::insert(
                                &self.book_cache,
                                (ident.clone(), title),
                                result.clone(),
                            );
                            // Also cache by path if available
                            if let Some(path) = result.0.path.as_ref() {
                                let path: SmolStr = path.as_ref().into();
                                cache_impl::insert(
                                    &self.notebook_key_cache,
                                    path.clone(),
                                    ident.clone(),
                                );
                                cache_impl::insert(&self.book_cache, (ident, path), result.clone());
                            }
                        }
                        notebooks.push(result);
                    }
                    Err(e) => {
                        tracing::warn!(
                            "fetch_notebooks_for_did: view_notebook failed for {}: {}",
                            record.uri,
                            e
                        );
                        continue;
                    }
                }
            }
        }
        Ok(notebooks)
    }

    /// Fetch all entries for a DID (for profile timeline)
    #[cfg(feature = "use-index")]
    pub async fn fetch_entries_for_did(
        &self,
        ident: &AtIdentifier<'_>,
    ) -> Result<Vec<Arc<(EntryView<'static>, Entry<'static>)>>> {
        use weaver_api::sh_weaver::actor::get_actor_entries::GetActorEntries;

        let client = self.get_client();

        let resp = client
            .send(
                GetActorEntries::new()
                    .actor(ident.clone())
                    .limit(100)
                    .build(),
            )
            .await
            .map_err(|e| dioxus::CapturedError::from_display(e))?;

        let output = resp
            .into_output()
            .map_err(|e| dioxus::CapturedError::from_display(e))?;

        let mut entries = Vec::new();

        for entry_view in output.entries {
            // Deserialize Entry from the record field
            let entry: Entry = jacquard::from_data(&entry_view.record)
                .map_err(|e| dioxus::CapturedError::from_display(e))?;
            let entry = entry.into_static();

            entries.push(Arc::new((entry_view.into_static(), entry)));
        }

        Ok(entries)
    }

    /// Fetch all entries for a DID (for profile timeline)
    #[cfg(not(feature = "use-index"))]
    pub async fn fetch_entries_for_did(
        &self,
        ident: &AtIdentifier<'_>,
    ) -> Result<Vec<Arc<(EntryView<'static>, Entry<'static>)>>> {
        use jacquard::{
            IntoStatic,
            types::{collection::Collection, nsid::Nsid},
            xrpc::XrpcExt,
        };
        use weaver_api::com_atproto::repo::list_records::ListRecords;

        let client = self.get_client();

        // Resolve DID and PDS
        let (repo_did, pds_url) = match ident {
            AtIdentifier::Did(did) => {
                let pds = client
                    .pds_for_did(did)
                    .await
                    .map_err(|e| dioxus::CapturedError::from_display(e))?;
                (did.clone(), pds)
            }
            AtIdentifier::Handle(handle) => client
                .pds_for_handle(handle)
                .await
                .map_err(|e| dioxus::CapturedError::from_display(e))?,
        };

        // Fetch all entry records for this repo
        let resp = client
            .xrpc(pds_url)
            .send(
                &ListRecords::new()
                    .repo(repo_did)
                    .collection(Nsid::raw(Entry::NSID))
                    .limit(100)
                    .build(),
            )
            .await
            .map_err(|e| dioxus::CapturedError::from_display(e))?;

        let mut entries = Vec::new();
        let ident_static = ident.clone().into_static();

        if let Ok(list) = resp.parse() {
            for record in list.records {
                // Extract rkey from URI
                let rkey = record.uri.rkey().map(|r| r.0.as_str()).unwrap_or_default();

                // Fetch the entry with hydration
                match client.fetch_entry_by_rkey(&ident_static, rkey).await {
                    Ok((entry_view, entry)) => {
                        entries.push(Arc::new((entry_view.into_static(), entry.into_static())));
                    }
                    Err(e) => {
                        tracing::warn!(
                            "[fetch_entries_for_did] failed to load entry {}: {:?}",
                            rkey,
                            e
                        );
                        continue;
                    }
                }
            }
        }

        Ok(entries)
    }

    pub async fn list_notebook_entries(
        &self,
        ident: AtIdentifier<'static>,
        book_title: SmolStr,
    ) -> Result<Option<Vec<BookEntryView<'static>>>> {
        use jacquard::types::aturi::AtUri;

        if let Some(result) = self.get_notebook(ident.clone(), book_title).await? {
            let (notebook, entry_refs) = result.as_ref();
            let mut book_entries = Vec::new();
            let client = self.get_client();

            for (index, entry_ref) in entry_refs.iter().enumerate() {
                // Try to extract rkey from URI
                let rkey = AtUri::new(entry_ref.uri.as_ref())
                    .ok()
                    .and_then(|uri| uri.rkey().map(|r| SmolStr::new(r.as_ref())));

                // Check cache first
                #[cfg(feature = "server")]
                if let Some(ref rkey) = rkey {
                    if let Some(cached) =
                        cache_impl::get(&self.entry_cache, &(ident.clone(), rkey.clone()))
                    {
                        book_entries.push(cached.0.clone());
                        continue;
                    }
                }

                // Fetch if not cached
                if let Ok(book_entry) = client.view_entry(notebook, entry_refs, index).await {
                    // Try to populate cache by deserializing Entry from the view's record
                    #[cfg(feature = "server")]
                    if let Some(rkey) = rkey {
                        use jacquard::IntoStatic;
                        use weaver_api::sh_weaver::notebook::entry::Entry;
                        if let Ok(entry) =
                            jacquard::from_data::<Entry<'_>>(&book_entry.entry.record)
                        {
                            let cached =
                                Arc::new((book_entry.clone().into_static(), entry.into_static()));
                            cache_impl::insert(&self.entry_cache, (ident.clone(), rkey), cached);
                        }
                    }
                    book_entries.push(book_entry);
                }
            }

            Ok(Some(book_entries))
        } else {
            Err(dioxus::CapturedError::from_display("Notebook not found"))
        }
    }

    pub async fn fetch_profile(
        &self,
        ident: &AtIdentifier<'_>,
    ) -> Result<Arc<ProfileDataView<'static>>> {
        use jacquard::IntoStatic;

        let ident_static = ident.clone().into_static();

        #[cfg(feature = "server")]
        if let Some(cached) = cache_impl::get(&self.profile_cache, &ident_static) {
            return Ok(cached);
        }

        let client = self.get_client();

        let did = match ident {
            AtIdentifier::Did(d) => d.clone(),
            AtIdentifier::Handle(h) => client
                .resolve_handle(h)
                .await
                .map_err(|e| dioxus::CapturedError::from_display(e))?,
        };

        let (_uri, profile_view) = client
            .hydrate_profile_view(&did)
            .await
            .map_err(|e| dioxus::CapturedError::from_display(e))?;

        let result = Arc::new(profile_view);
        #[cfg(feature = "server")]
        cache_impl::insert(&self.profile_cache, ident_static, result.clone());

        Ok(result)
    }

    /// Fetch an entry by rkey with optional notebook context lookup.
    pub async fn get_entry_by_rkey(
        &self,
        ident: AtIdentifier<'static>,
        rkey: SmolStr,
    ) -> Result<Option<Arc<StandaloneEntryData>>> {
        use jacquard::types::aturi::AtUri;

        #[cfg(feature = "server")]
        if let Some(cached) =
            cache_impl::get(&self.standalone_entry_cache, &(ident.clone(), rkey.clone()))
        {
            return Ok(Some(cached));
        }

        let client = self.get_client();

        // Fetch entry directly by rkey
        let (entry_view, entry) = client
            .fetch_entry_by_rkey(&ident, &rkey)
            .await
            .map_err(|e| dioxus::CapturedError::from_display(e))?;

        // Try to find notebook context via constellation
        let entry_uri = entry_view.uri.clone();
        let at_uri = AtUri::new(entry_uri.as_ref()).map_err(|e| {
            dioxus::CapturedError::from_display(
                format_smolstr!("Invalid entry URI: {}", e).as_str(),
            )
        })?;

        let (total, first_notebook) = client
            .find_notebooks_for_entry(&at_uri)
            .await
            .map_err(|e| dioxus::CapturedError::from_display(e))?;

        // Only provide notebook context if entry is in exactly one notebook
        let notebook_context = if total == 1 {
            if let Some(notebook_id) = first_notebook {
                // Construct notebook URI from RecordId
                let notebook_uri_str = format_smolstr!(
                    "at://{}/{}/{}",
                    notebook_id.did.as_str(),
                    notebook_id.collection.as_str(),
                    notebook_id.rkey.0.as_str()
                );
                let notebook_uri = AtUri::new_owned(notebook_uri_str).map_err(|e| {
                    dioxus::CapturedError::from_display(
                        format_smolstr!("Invalid notebook URI: {}", e).as_str(),
                    )
                })?;

                // Fetch notebook and find entry position
                if let Ok((notebook, entries)) = client.view_notebook(&notebook_uri).await {
                    if let Ok(Some(book_entry_view)) = client
                        .entry_in_notebook_by_rkey(&notebook, &entries, &rkey)
                        .await
                    {
                        Some(NotebookContext {
                            notebook: notebook.into_static(),
                            book_entry_view: book_entry_view.into_static(),
                        })
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        let result = Arc::new(StandaloneEntryData {
            entry,
            entry_view,
            notebook_context,
        });
        #[cfg(feature = "server")]
        cache_impl::insert(&self.standalone_entry_cache, (ident, rkey), result.clone());

        Ok(Some(result))
    }

    /// Fetch an entry by rkey within a specific notebook context.
    ///
    /// The book_title parameter provides the notebook context.
    /// Returns BookEntryView without prev/next if entry is in multiple notebooks.
    pub async fn get_notebook_entry_by_rkey(
        &self,
        ident: AtIdentifier<'static>,
        book_title: SmolStr,
        rkey: SmolStr,
    ) -> Result<Option<Arc<(BookEntryView<'static>, Entry<'static>)>>> {
        use jacquard::types::aturi::AtUri;

        #[cfg(feature = "server")]
        if let Some(cached) = cache_impl::get(&self.entry_cache, &(ident.clone(), rkey.clone())) {
            return Ok(Some(cached));
        }

        let client = self.get_client();

        // Fetch entry directly by rkey
        let (entry_view, entry) = client
            .fetch_entry_by_rkey(&ident, &rkey)
            .await
            .map_err(|e| dioxus::CapturedError::from_display(e))?;

        // Fetch notebook by title
        let notebook_result = client
            .notebook_by_title(&ident, &book_title)
            .await
            .map_err(|e| dioxus::CapturedError::from_display(e))?;

        let (notebook, entries) = match notebook_result {
            Some((n, e)) => (n, e),
            None => return Err(dioxus::CapturedError::from_display("Notebook not found")),
        };

        // Find entry position in notebook
        let book_entry_view = client
            .entry_in_notebook_by_rkey(&notebook, &entries, &rkey)
            .await
            .map_err(|e| dioxus::CapturedError::from_display(e))?;

        let mut book_entry_view = match book_entry_view {
            Some(bev) => bev,
            None => {
                // Entry not in this notebook's entry list - return basic view without nav
                use weaver_api::sh_weaver::notebook::BookEntryView;
                BookEntryView::new().entry(entry_view).index(0).build()
            }
        };

        // Check if entry is in multiple notebooks - if so, clear prev/next
        let entry_uri = book_entry_view.entry.uri.clone();
        let at_uri = AtUri::new(entry_uri.as_ref()).map_err(|e| {
            dioxus::CapturedError::from_display(
                format_smolstr!("Invalid entry URI: {}", e).as_str(),
            )
        })?;

        let (total, _) = client
            .find_notebooks_for_entry(&at_uri)
            .await
            .map_err(|e| dioxus::CapturedError::from_display(e))?;

        if total >= 2 {
            // Entry is in multiple notebooks - clear prev/next to avoid ambiguity
            book_entry_view = BookEntryView::new()
                .entry(book_entry_view.entry)
                .index(book_entry_view.index)
                .build();
        }

        let result = Arc::new((book_entry_view.into_static(), entry));
        #[cfg(feature = "server")]
        cache_impl::insert(&self.entry_cache, (ident, rkey), result.clone());

        Ok(Some(result))
    }
}

impl HttpClient for Fetcher {
    type Error = IdentityError;

    #[cfg(not(target_arch = "wasm32"))]
    fn send_http(
        &self,
        request: http::Request<Vec<u8>>,
    ) -> impl Future<Output = core::result::Result<http::Response<Vec<u8>>, Self::Error>> + Send
    {
        async {
            let client = self.get_client();
            client.send_http(request).await
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn send_http(
        &self,
        request: http::Request<Vec<u8>>,
    ) -> impl Future<Output = core::result::Result<http::Response<Vec<u8>>, Self::Error>> {
        async {
            let client = self.get_client();
            client.send_http(request).await
        }
    }
}

impl XrpcClient for Fetcher {
    #[doc = " Get the base URI for the client."]
    fn base_uri(&self) -> impl Future<Output = CowStr<'static>> + Send {
        self.client.base_uri()
    }

    #[doc = " Send an XRPC request and parse the response"]
    #[cfg(not(target_arch = "wasm32"))]
    fn send<R>(&self, request: R) -> impl Future<Output = XrpcResult<XrpcResponse<R>>> + Send
    where
        R: XrpcRequest + Send + Sync,
        <R as XrpcRequest>::Response: Send + Sync,
        Self: Sync,
    {
        self.client.send(request)
    }

    #[doc = " Send an XRPC request and parse the response"]
    #[cfg(not(target_arch = "wasm32"))]
    fn send_with_opts<R>(
        &self,
        request: R,
        opts: CallOptions<'_>,
    ) -> impl Future<Output = XrpcResult<XrpcResponse<R>>> + Send
    where
        R: XrpcRequest + Send + Sync,
        <R as XrpcRequest>::Response: Send + Sync,
        Self: Sync,
    {
        self.client.send_with_opts(request, opts)
    }

    #[doc = " Send an XRPC request and parse the response"]
    #[cfg(target_arch = "wasm32")]
    fn send<R>(&self, request: R) -> impl Future<Output = XrpcResult<XrpcResponse<R>>>
    where
        R: XrpcRequest + Send + Sync,
        <R as XrpcRequest>::Response: Send + Sync,
    {
        self.client.send(request)
    }

    #[doc = " Send an XRPC request and parse the response"]
    #[cfg(target_arch = "wasm32")]
    fn send_with_opts<R>(
        &self,
        request: R,
        opts: CallOptions<'_>,
    ) -> impl Future<Output = XrpcResult<XrpcResponse<R>>>
    where
        R: XrpcRequest + Send + Sync,
        <R as XrpcRequest>::Response: Send + Sync,
    {
        self.client.send_with_opts(request, opts)
    }

    #[doc = " Set the base URI for the client."]
    fn set_base_uri(&self, url: jacquard::url::Url) -> impl Future<Output = ()> + Send {
        self.client.set_base_uri(url)
    }

    #[doc = " Get the call options for the client."]
    fn opts(&self) -> impl Future<Output = CallOptions<'_>> + Send {
        self.client.opts()
    }

    #[doc = " Set the call options for the client."]
    fn set_opts(&self, opts: CallOptions) -> impl Future<Output = ()> + Send {
        self.client.set_opts(opts)
    }
}

impl IdentityResolver for Fetcher {
    #[doc = " Access options for validation decisions in default methods"]
    fn options(&self) -> &ResolverOptions {
        self.client.options()
    }

    #[doc = " Resolve handle"]
    #[cfg(not(target_arch = "wasm32"))]
    fn resolve_handle(
        &self,
        handle: &Handle<'_>,
    ) -> impl Future<Output = core::result::Result<Did<'static>, IdentityError>> + Send
    where
        Self: Sync,
    {
        self.client.resolve_handle(handle)
    }

    #[doc = " Resolve DID document"]
    #[cfg(not(target_arch = "wasm32"))]
    fn resolve_did_doc(
        &self,
        did: &Did<'_>,
    ) -> impl Future<Output = core::result::Result<DidDocResponse, IdentityError>> + Send
    where
        Self: Sync,
    {
        self.client.resolve_did_doc(did)
    }

    #[doc = " Resolve handle"]
    #[cfg(target_arch = "wasm32")]
    fn resolve_handle(
        &self,
        handle: &Handle<'_>,
    ) -> impl Future<Output = core::result::Result<Did<'static>, IdentityError>> {
        self.client.resolve_handle(handle)
    }

    #[doc = " Resolve DID document"]
    #[cfg(target_arch = "wasm32")]
    fn resolve_did_doc(
        &self,
        did: &Did<'_>,
    ) -> impl Future<Output = core::result::Result<DidDocResponse, IdentityError>> {
        self.client.resolve_did_doc(did)
    }
}

impl LexiconSchemaResolver for Fetcher {
    #[cfg(not(target_arch = "wasm32"))]
    async fn resolve_lexicon_schema(
        &self,
        nsid: &Nsid<'_>,
    ) -> std::result::Result<ResolvedLexiconSchema<'static>, LexiconResolutionError> {
        self.client.resolve_lexicon_schema(nsid).await
    }

    #[cfg(target_arch = "wasm32")]
    async fn resolve_lexicon_schema(
        &self,
        nsid: &Nsid<'_>,
    ) -> std::result::Result<ResolvedLexiconSchema<'static>, LexiconResolutionError> {
        self.client.resolve_lexicon_schema(nsid).await
    }
}

// ============================================================================
// Collaboration & Edit methods (use-index gated)
// ============================================================================

impl Fetcher {
    /// Get edit history for a resource from weaver-index.
    ///
    /// Returns edit roots and diffs for the given resource URI.
    #[cfg(feature = "use-index")]
    pub async fn get_edit_history(
        &self,
        resource_uri: &AtUri<'_>,
    ) -> Result<weaver_api::sh_weaver::edit::get_edit_history::GetEditHistoryOutput<'static>> {
        use weaver_api::sh_weaver::edit::get_edit_history::GetEditHistory;

        let client = self.get_client();
        let resp = client
            .send(GetEditHistory::new().resource(resource_uri.clone()).build())
            .await
            .map_err(|e| dioxus::CapturedError::from_display(e))?;

        resp.into_output()
            .map(|o| o.into_static())
            .map_err(|e| dioxus::CapturedError::from_display(e))
    }

    /// List drafts for an actor from weaver-index.
    #[cfg(feature = "use-index")]
    pub async fn list_drafts(
        &self,
        actor: &AtIdentifier<'_>,
    ) -> Result<weaver_api::sh_weaver::edit::list_drafts::ListDraftsOutput<'static>> {
        use weaver_api::sh_weaver::edit::list_drafts::ListDrafts;

        let client = self.get_client();
        let resp = client
            .send(ListDrafts::new().actor(actor.clone()).build())
            .await
            .map_err(|e| dioxus::CapturedError::from_display(e))?;

        resp.into_output()
            .map(|o| o.into_static())
            .map_err(|e| dioxus::CapturedError::from_display(e))
    }

    /// Get resource sessions from weaver-index.
    ///
    /// Returns active collaboration sessions for the given resource.
    #[cfg(feature = "use-index")]
    pub async fn get_resource_sessions(
        &self,
        resource_uri: &AtUri<'_>,
    ) -> Result<
        weaver_api::sh_weaver::collab::get_resource_sessions::GetResourceSessionsOutput<'static>,
    > {
        use weaver_api::sh_weaver::collab::get_resource_sessions::GetResourceSessions;

        let client = self.get_client();
        let resp = client
            .send(
                GetResourceSessions::new()
                    .resource(resource_uri.clone())
                    .build(),
            )
            .await
            .map_err(|e| dioxus::CapturedError::from_display(e))?;

        resp.into_output()
            .map(|o| o.into_static())
            .map_err(|e| dioxus::CapturedError::from_display(e))
    }

    /// Get resource participants from weaver-index.
    ///
    /// Returns owner and collaborators who can edit the resource.
    #[cfg(feature = "use-index")]
    pub async fn get_resource_participants(
        &self,
        resource_uri: &AtUri<'_>,
    ) -> Result<
        weaver_api::sh_weaver::collab::get_resource_participants::GetResourceParticipantsOutput<
            'static,
        >,
    > {
        use weaver_api::sh_weaver::collab::get_resource_participants::GetResourceParticipants;

        let client = self.get_client();
        let resp = client
            .send(
                GetResourceParticipants::new()
                    .resource(resource_uri.clone())
                    .build(),
            )
            .await
            .map_err(|e| dioxus::CapturedError::from_display(e))?;

        resp.into_output()
            .map(|o| o.into_static())
            .map_err(|e| dioxus::CapturedError::from_display(e))
    }

    /// Get contributors for a resource from weaver-index.
    #[cfg(feature = "use-index")]
    pub async fn get_contributors(
        &self,
        resource_uri: &AtUri<'_>,
    ) -> Result<weaver_api::sh_weaver::edit::get_contributors::GetContributorsOutput<'static>> {
        use weaver_api::sh_weaver::edit::get_contributors::GetContributors;

        let client = self.get_client();
        let resp = client
            .send(
                GetContributors::new()
                    .resource(resource_uri.clone())
                    .build(),
            )
            .await
            .map_err(|e| dioxus::CapturedError::from_display(e))?;

        resp.into_output()
            .map(|o| o.into_static())
            .map_err(|e| dioxus::CapturedError::from_display(e))
    }
}

impl AgentSession for Fetcher {
    #[doc = " Identify the kind of session."]
    fn session_kind(&self) -> AgentKind {
        self.client.session_kind()
    }

    #[doc = " Return current DID and an optional session id (always Some for OAuth)."]
    async fn session_info(&self) -> Option<(Did<'static>, Option<CowStr<'static>>)> {
        self.client.session_info().await
    }

    async fn endpoint(&self) -> CowStr<'static> {
        self.client.endpoint().await
    }

    async fn set_options<'a>(&'a self, opts: CallOptions<'a>) {
        self.client.set_options(opts).await
    }

    async fn refresh(&self) -> XrpcResult<AuthorizationToken<'static>> {
        self.client.refresh().await
    }
}
