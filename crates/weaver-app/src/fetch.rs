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
use jacquard::{smol_str::SmolStr, types::ident::AtIdentifier};
use serde::{Deserialize, Serialize};
use std::future::Future;
use std::{sync::Arc, time::Duration};
use tokio::sync::RwLock;
use weaver_api::app_bsky::actor::get_profile::GetProfile;
use weaver_api::app_bsky::actor::profile::Profile as BskyProfile;
use weaver_api::sh_weaver::actor::ProfileDataViewInner;
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
        self.oauth_client.client.send_http(request)
    }

    #[cfg(target_arch = "wasm32")]
    fn send_http(
        &self,
        request: http::Request<Vec<u8>>,
    ) -> impl Future<Output = core::result::Result<http::Response<Vec<u8>>, Self::Error>> {
        self.oauth_client.client.send_http(request)
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

//#[cfg(not(feature = "server"))]
#[derive(Clone)]
pub struct Fetcher {
    pub client: Arc<Client>,
}

//#[cfg(not(feature = "server"))]
impl Fetcher {
    pub fn new(client: OAuthClient<JacquardResolver, AuthStore>) -> Self {
        Self {
            client: Arc::new(Client::new(client)),
        }
    }

    pub async fn upgrade_to_authenticated(
        &self,
        session: OAuthSession<JacquardResolver, crate::auth::AuthStore>,
    ) {
        let mut session_slot = self.client.session.write().await;
        *session_slot = Some(Arc::new(Agent::new(session)));
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
        let client = self.get_client();
        if let Some((notebook, entries)) = client
            .notebook_by_title(&ident, &title)
            .await
            .map_err(|e| dioxus::CapturedError::from_display(e))?
        {
            let stored = Arc::new((notebook, entries));
            Ok(Some(stored))
        } else {
            Err(dioxus::CapturedError::from_display("Notebook not found"))
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
            let client = self.get_client();
            if let Some(entry) = client
                .entry_by_title(notebook, entries.as_ref(), &entry_title)
                .await
                .map_err(|e| dioxus::CapturedError::from_display(e))?
            {
                let stored = Arc::new(entry);
                Ok(Some(stored))
            } else {
                Err(dioxus::CapturedError::from_display("Entry not found"))
            }
        } else {
            Err(dioxus::CapturedError::from_display("Notebook not found"))
        }
    }

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
            let uri_str = format!(
                "at://{}/{}/{}",
                ufos_record.did, ufos_record.collection, ufos_record.rkey
            );
            let uri = AtUri::new_owned(uri_str)
                .map_err(|e| dioxus::CapturedError::from_display(format!("Invalid URI: {}", e)))?;

            // Fetch the full notebook view (which hydrates authors)
            match client.view_notebook(&uri).await {
                Ok((notebook, entries)) => {
                    let ident = uri.authority().clone().into_static();
                    let title = notebook
                        .title
                        .as_ref()
                        .map(|t| SmolStr::new(t.as_ref()))
                        .unwrap_or_else(|| SmolStr::new("Untitled"));

                    let result = Arc::new((notebook, entries));
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
        let resp = client
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
                match client.view_notebook(&record.uri).await {
                    Ok((notebook, entries)) => {
                        let result = Arc::new((notebook, entries));
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
            let client = self.get_client();

            for index in 0..entries.len() {
                match client.view_entry(notebook, entries, index).await {
                    Ok(book_entry) => book_entries.push(book_entry),
                    Err(_) => continue, // Skip entries that fail to load
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

        Ok(Arc::new(profile_view))
    }
}

// #[cfg(feature = "server")]
// #[derive(Clone)]
// pub struct Fetcher {
//     pub client: Arc<Client>,
//     book_cache: cache_impl::Cache<
//         (AtIdentifier<'static>, SmolStr),
//         Arc<(NotebookView<'static>, Vec<StrongRef<'static>>)>,
//     >,
//     entry_cache: cache_impl::Cache<
//         (AtIdentifier<'static>, SmolStr),
//         Arc<(BookEntryView<'static>, Entry<'static>)>,
//     >,
//     profile_cache: cache_impl::Cache<AtIdentifier<'static>, Arc<ProfileDataView<'static>>>,
// }

// // /// SAFETY: This isn't thread-safe on WASM, but we aren't multithreaded on WASM
// //#[cfg(feature = "server")]
// unsafe impl Sync for Fetcher {}

// // /// SAFETY: This isn't thread-safe on WASM, but we aren't multithreaded on WASM
// //#[cfg(feature = "server")]
// unsafe impl Send for Fetcher {}

// #[cfg(feature = "server")]
// impl Fetcher {
//     pub fn new(client: OAuthClient<JacquardResolver, AuthStore>) -> Self {
//         Self {
//             client: Arc::new(Client::new(client)),
//             book_cache: cache_impl::new_cache(100, Duration::from_secs(30)),
//             entry_cache: cache_impl::new_cache(100, Duration::from_secs(30)),
//             profile_cache: cache_impl::new_cache(100, Duration::from_secs(1800)),
//         }
//     }

//     pub async fn upgrade_to_authenticated(
//         &self,
//         session: OAuthSession<JacquardResolver, crate::auth::AuthStore>,
//     ) {
//         let mut session_slot = self.client.session.write().await;
//         *session_slot = Some(Arc::new(Agent::new(session)));
//     }

//     pub async fn downgrade_to_unauthenticated(&self) {
//         let mut session_slot = self.client.session.write().await;
//         if let Some(session) = session_slot.take() {
//             session.inner().logout().await.ok();
//         }
//     }

//     #[allow(dead_code)]
//     pub async fn current_did(&self) -> Option<Did<'static>> {
//         let session_slot = self.client.session.read().await;
//         if let Some(session) = session_slot.as_ref() {
//             session.info().await.map(|(d, _)| d)
//         } else {
//             None
//         }
//     }

//     pub fn get_client(&self) -> Arc<Client> {
//         self.client.clone()
//     }

//     pub async fn get_notebook(
//         &self,
//         ident: AtIdentifier<'static>,
//         title: SmolStr,
//     ) -> Result<Option<Arc<(NotebookView<'static>, Vec<StrongRef<'static>>)>>> {
//         if let Some(entry) = cache_impl::get(&self.book_cache, &(ident.clone(), title.clone())) {
//             Ok(Some(entry))
//         } else {
//             let client = self.get_client();
//             if let Some((notebook, entries)) = client
//                 .notebook_by_title(&ident, &title)
//                 .await
//                 .map_err(|e| dioxus::CapturedError::from_display(e))?
//             {
//                 let stored = Arc::new((notebook, entries));
//                 cache_impl::insert(&self.book_cache, (ident, title), stored.clone());
//                 Ok(Some(stored))
//             } else {
//                 Ok(None)
//             }
//         }
//     }

//     pub async fn get_entry(
//         &self,
//         ident: AtIdentifier<'static>,
//         book_title: SmolStr,
//         entry_title: SmolStr,
//     ) -> Result<Option<Arc<(BookEntryView<'static>, Entry<'static>)>>> {
//         if let Some(result) = self.get_notebook(ident.clone(), book_title).await? {
//             let (notebook, entries) = result.as_ref();
//             if let Some(entry) =
//                 cache_impl::get(&self.entry_cache, &(ident.clone(), entry_title.clone()))
//             {
//                 Ok(Some(entry))
//             } else {
//                 let client = self.get_client();
//                 if let Some(entry) = client
//                     .entry_by_title(notebook, entries.as_ref(), &entry_title)
//                     .await
//                     .map_err(|e| dioxus::CapturedError::from_display(e))?
//                 {
//                     let stored = Arc::new(entry);
//                     cache_impl::insert(&self.entry_cache, (ident, entry_title), stored.clone());
//                     Ok(Some(stored))
//                 } else {
//                     Ok(None)
//                 }
//             }
//         } else {
//             Ok(None)
//         }
//     }

//     pub async fn fetch_notebooks_from_ufos(
//         &self,
//     ) -> Result<Vec<Arc<(NotebookView<'static>, Vec<StrongRef<'static>>)>>> {
//         use jacquard::{IntoStatic, types::aturi::AtUri};

//         let url = "https://ufos-api.microcosm.blue/records?collection=sh.weaver.notebook.book";
//         let response = reqwest::get(url)
//             .await
//             .map_err(|e| dioxus::CapturedError::from_display(e))?;

//         let records: Vec<UfosRecord> = response
//             .json()
//             .await
//             .map_err(|e| dioxus::CapturedError::from_display(e))?;

//         let mut notebooks = Vec::new();
//         let client = self.get_client();

//         for ufos_record in records {
//             // Construct URI
//             let uri_str = format!(
//                 "at://{}/{}/{}",
//                 ufos_record.did, ufos_record.collection, ufos_record.rkey
//             );
//             let uri = AtUri::new_owned(uri_str)
//                 .map_err(|e| dioxus::CapturedError::from_display(format!("Invalid URI: {}", e)))?;

//             // Fetch the full notebook view (which hydrates authors)
//             match client.view_notebook(&uri).await {
//                 Ok((notebook, entries)) => {
//                     let ident = uri.authority().clone().into_static();
//                     let title = notebook
//                         .title
//                         .as_ref()
//                         .map(|t| SmolStr::new(t.as_ref()))
//                         .unwrap_or_else(|| SmolStr::new("Untitled"));

//                     let result = Arc::new((notebook, entries));
//                     // Cache it
//                     cache_impl::insert(&self.book_cache, (ident, title), result.clone());
//                     notebooks.push(result);
//                 }
//                 Err(_) => continue, // Skip notebooks that fail to load
//             }
//         }

//         Ok(notebooks)
//     }

//     pub async fn fetch_notebooks_for_did(
//         &self,
//         ident: &AtIdentifier<'_>,
//     ) -> Result<Vec<Arc<(NotebookView<'static>, Vec<StrongRef<'static>>)>>> {
//         use jacquard::{
//             IntoStatic,
//             types::{collection::Collection, nsid::Nsid},
//             xrpc::XrpcExt,
//         };
//         use weaver_api::{
//             com_atproto::repo::list_records::ListRecords, sh_weaver::notebook::book::Book,
//         };

//         let client = self.get_client();

//         // Resolve DID and PDS
//         let (repo_did, pds_url) = match ident {
//             AtIdentifier::Did(did) => {
//                 let pds = client
//                     .pds_for_did(did)
//                     .await
//                     .map_err(|e| dioxus::CapturedError::from_display(e))?;
//                 (did.clone(), pds)
//             }
//             AtIdentifier::Handle(handle) => client
//                 .pds_for_handle(handle)
//                 .await
//                 .map_err(|e| dioxus::CapturedError::from_display(e))?,
//         };

//         // Fetch all notebook records for this repo
//         let resp = client
//             .xrpc(pds_url)
//             .send(
//                 &ListRecords::new()
//                     .repo(repo_did)
//                     .collection(Nsid::raw(Book::NSID))
//                     .limit(100)
//                     .build(),
//             )
//             .await
//             .map_err(|e| dioxus::CapturedError::from_display(e))?;

//         let mut notebooks = Vec::new();

//         if let Ok(list) = resp.parse() {
//             for record in list.records {
//                 // View the notebook (which hydrates authors)
//                 match client.view_notebook(&record.uri).await {
//                     Ok((notebook, entries)) => {
//                         let ident = record.uri.authority().clone().into_static();
//                         let title = notebook
//                             .title
//                             .as_ref()
//                             .map(|t| SmolStr::new(t.as_ref()))
//                             .unwrap_or_else(|| SmolStr::new("Untitled"));

//                         let result = Arc::new((notebook, entries));
//                         // Cache it
//                         cache_impl::insert(&self.book_cache, (ident, title), result.clone());
//                         notebooks.push(result);
//                     }
//                     Err(_) => continue, // Skip notebooks that fail to load
//                 }
//             }
//         }

//         Ok(notebooks)
//     }

//     pub async fn list_notebook_entries(
//         &self,
//         ident: AtIdentifier<'static>,
//         book_title: SmolStr,
//     ) -> Result<Option<Vec<BookEntryView<'static>>>> {
//         if let Some(result) = self.get_notebook(ident.clone(), book_title).await? {
//             let (notebook, entries) = result.as_ref();
//             let mut book_entries = Vec::new();
//             let client = self.get_client();

//             for index in 0..entries.len() {
//                 match client.view_entry(notebook, entries, index).await {
//                     Ok(book_entry) => book_entries.push(book_entry),
//                     Err(_) => continue, // Skip entries that fail to load
//                 }
//             }

//             Ok(Some(book_entries))
//         } else {
//             Ok(None)
//         }
//     }

//     pub async fn fetch_profile(
//         &self,
//         ident: &AtIdentifier<'_>,
//     ) -> Result<Arc<ProfileDataView<'static>>> {
//         use jacquard::IntoStatic;

//         let ident_static = ident.clone().into_static();

//         if let Some(cached) = cache_impl::get(&self.profile_cache, &ident_static) {
//             return Ok(cached);
//         }

//         let client = self.get_client();

//         let did = match ident {
//             AtIdentifier::Did(d) => d.clone(),
//             AtIdentifier::Handle(h) => client
//                 .resolve_handle(h)
//                 .await
//                 .map_err(|e| dioxus::CapturedError::from_display(e))?,
//         };

//         let (_uri, profile_view) = client
//             .hydrate_profile_view(&did)
//             .await
//             .map_err(|e| dioxus::CapturedError::from_display(e))?;

//         let result = Arc::new(profile_view);
//         cache_impl::insert(&self.profile_cache, ident_static, result.clone());

//         Ok(result)
//     }
// }

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
