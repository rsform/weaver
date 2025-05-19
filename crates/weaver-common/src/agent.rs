use crate::app::bsky::actor::defs::ProfileViewDetailedData;
use crate::error::GenericXrpcError;
use crate::resolver::HickoryDnsTxtResolver;
use crate::sh::weaver::actor::defs::ProfileDataViewInnerRefs;
use atrium_api::agent::{CloneWithProxy, Configure};
use atrium_api::types::string::{Cid, Did, Handle, Nsid, RecordKey};
use atrium_api::types::{Collection, Union, Unknown};
use atrium_api::{agent::SessionManager, types::string::AtIdentifier};
use atrium_common::resolver::Resolver;
use atrium_identity::did::{CommonDidResolver, CommonDidResolverConfig, DEFAULT_PLC_DIRECTORY_URL};
use atrium_identity::handle::{AtprotoHandleResolver, AtprotoHandleResolverConfig};
use atrium_identity::identity_resolver::IdentityResolver;
use atrium_xrpc::{Error as XrpcError, HttpClient, OutputDataOrBytes, XrpcClient, XrpcRequest};
use http::{Request, Response};
use serde::{Serialize, de::DeserializeOwned};
use std::{fmt::Debug, ops::Deref, sync::Arc};

use crate::Error;
use crate::client::Service;

pub struct WeaverHttpClient {
    pub client: reqwest::Client,
}

impl atrium_xrpc::HttpClient for WeaverHttpClient {
    async fn send_http(
        &self,
        request: atrium_xrpc::http::Request<Vec<u8>>,
    ) -> core::result::Result<
        atrium_xrpc::http::Response<Vec<u8>>,
        Box<dyn std::error::Error + Send + Sync + 'static>,
    > {
        let response = self.client.execute(request.try_into()?).await?;
        let mut builder = atrium_xrpc::http::Response::builder().status(response.status());
        for (k, v) in response.headers() {
            builder = builder.header(k, v);
        }
        builder
            .body(response.bytes().await?.to_vec())
            .map_err(Into::into)
    }
}

impl Default for WeaverHttpClient {
    fn default() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }
}

pub struct WeaverAgent<M>
where
    M: SessionManager + Send + Sync,
{
    session_manager: Arc<Wrapper<M>>,
    resolver: Arc<
        IdentityResolver<
            CommonDidResolver<WeaverHttpClient>,
            AtprotoHandleResolver<HickoryDnsTxtResolver, WeaverHttpClient>,
        >,
    >,
    pub api: Service<Wrapper<M>>,
}

impl<M> WeaverAgent<M>
where
    M: SessionManager + Send + Sync,
{
    /// Creates a new agent with the given session manager.

    pub fn new(session_manager: M) -> Self {
        let session_manager = Arc::new(Wrapper::new(session_manager));
        let api = Service::new(session_manager.clone());
        let http_client = Arc::new(WeaverHttpClient::default());
        let resolver_config = atrium_identity::identity_resolver::IdentityResolverConfig {
            did_resolver: CommonDidResolver::new(CommonDidResolverConfig {
                plc_directory_url: DEFAULT_PLC_DIRECTORY_URL.to_string(),
                http_client: Arc::clone(&http_client),
            }),
            handle_resolver: AtprotoHandleResolver::new(AtprotoHandleResolverConfig {
                dns_txt_resolver: HickoryDnsTxtResolver::default(),
                http_client: Arc::clone(&http_client),
            }),
        };
        Self {
            session_manager,
            resolver: Arc::new(IdentityResolver::new(resolver_config)),
            api,
        }
    }

    /// Returns the DID of the current session.

    pub async fn did(&self) -> Option<Did> {
        self.session_manager.did().await
    }
}

impl<M> WeaverAgent<M>
where
    M: SessionManager + Configure + Send + Sync,
{
    pub async fn get_bsky_profile(
        &self,
        actor: AtIdentifier,
    ) -> Result<ProfileViewDetailedData, Error> {
        use crate::app::bsky::actor::get_profile::*;
        let result = self
            .api
            .app
            .bsky
            .actor
            .get_profile(ParametersData { actor }.into())
            .await?;
        Ok(result.data)
    }

    pub async fn get_weaver_profile_pds(
        &self,
        actor: AtIdentifier,
    ) -> Result<crate::sh::weaver::actor::defs::ProfileViewData, Error> {
        use crate::sh::weaver::actor::*;
        let identity = self
            .resolver
            .resolve(actor.as_ref())
            .await
            .expect("valid identifier");
        let did = Did::new(identity.did).expect("valid did");
        let record = self
            .get_record(
                Nsid::new(Profile::NSID.into()).expect("valid nsid"),
                RecordKey::new(Profile::NSID.to_string()).expect("valid key"),
                AtIdentifier::Did(did.clone()),
                None,
            )
            .await?;
        let profile_record = profile::RecordData::from(record.value);

        Ok(defs::ProfileViewData {
            avatar: profile_record
                .avatar
                .map(|avatar| crate::avatar_cdn_url(&did, &avatar)),
            created_at: profile_record.created_at,
            did,
            display_name: profile_record.display_name,
            handle: Handle::new(actor.as_ref().to_string()).expect("valid handle"),
            labels: None,
            links: profile_record.links,
            location: profile_record.location,
            description: profile_record.description,
            pronouns: profile_record.pronouns,
            indexed_at: None,
        })
    }

    pub async fn get_profile_pds(
        &self,
        actor: AtIdentifier,
    ) -> Result<crate::sh::weaver::actor::defs::ProfileDataViewInnerRefs, Error> {
        use crate::sh::weaver::actor::defs::ProfileDataViewInnerRefs::*;
        let maybe_profile = self.get_weaver_profile_pds(actor.clone()).await;
        match maybe_profile {
            Ok(profile) => Ok(ShWeaverActorDefsProfileView(Box::new(profile.into()))),
            Err(e1) => {
                let maybe_profile = self.get_bsky_profile(actor.clone()).await;
                match maybe_profile {
                    Ok(profile) => Ok(AppBskyActorDefsProfileViewDetailed(Box::new(
                        profile.into(),
                    ))),
                    Err(e2) => Err(e2.with_errors(e1)),
                }
            }
        }
    }

    pub async fn put_record(
        &self,
        collection: Nsid,
        record: Unknown,
        repo: AtIdentifier,
        rkey: RecordKey,
    ) -> Result<crate::com::atproto::repo::put_record::OutputData, Error> {
        use crate::com::atproto::repo::put_record::*;
        let result = self
            .api
            .com
            .atproto
            .repo
            .put_record(
                InputData {
                    collection,
                    record,
                    repo,
                    rkey,
                    swap_commit: None,
                    swap_record: None,
                    validate: None,
                }
                .into(),
            )
            .await?;
        Ok(result.data)
    }

    pub async fn get_record(
        &self,
        collection: Nsid,
        rkey: RecordKey,
        repo: AtIdentifier,
        cid: Option<Cid>,
    ) -> Result<crate::com::atproto::repo::get_record::OutputData, Error> {
        use crate::com::atproto::repo::get_record::*;
        let result = self
            .api
            .com
            .atproto
            .repo
            .get_record(
                ParametersData {
                    collection,
                    rkey,
                    repo,
                    cid,
                }
                .into(),
            )
            .await?;
        Ok(result.data)
    }

    pub async fn get_blob(&self, did: Did, cid: Cid) -> Result<Vec<u8>, Error> {
        use crate::com::atproto::sync::get_blob::*;
        let result = self
            .api
            .com
            .atproto
            .sync
            .get_blob(ParametersData { did, cid }.into())
            .await?;
        Ok(result)
    }

    pub async fn upload_blob(
        &self,
        input: Vec<u8>,
    ) -> Result<crate::com::atproto::repo::upload_blob::OutputData, Error> {
        let result = self.api.com.atproto.repo.upload_blob(input.into()).await?;
        Ok(result.data)
    }

    pub async fn upload_artifact(
        &self,
        content: String,
        mime_type: Option<String>,
    ) -> Result<crate::com::atproto::repo::upload_blob::OutputData, Error> {
        let encoding = if let Some(mime_type) = mime_type {
            Some(mime_type)
        } else {
            Some(String::from("*/*"))
        };
        let input = content.into_bytes();
        let response = self
            .session_manager
            .inner
            .send_xrpc::<(), Vec<u8>, crate::com::atproto::repo::upload_blob::OutputData, crate::com::atproto::repo::upload_blob::Error>(
                &atrium_xrpc::XrpcRequest {
                    method: http::Method::POST,
                    nsid: crate::com::atproto::repo::upload_blob::NSID.into(),
                    parameters: None,
                    input: Some(atrium_xrpc::InputDataOrBytes::Bytes(input)),
                    encoding,
                },
            )
            .await.map_err(Error::from)?;
        Ok(match response {
            atrium_xrpc::OutputDataOrBytes::Data(data) => Ok(data),
            _ => Err(GenericXrpcError::Other(
                "Unexpected Respose Type".to_owned(),
            )),
        }?)
    }

    pub async fn delete_record(
        &self,
        collection: Nsid,
        rkey: RecordKey,
        repo: AtIdentifier,
    ) -> Result<crate::com::atproto::repo::delete_record::OutputData, Error> {
        use crate::com::atproto::repo::delete_record::*;
        let result = self
            .api
            .com
            .atproto
            .repo
            .delete_record(
                InputData {
                    collection,
                    rkey,
                    repo,
                    swap_commit: None,
                    swap_record: None,
                }
                .into(),
            )
            .await?;
        Ok(result.data)
    }
}

impl<M> WeaverAgent<M>
where
    M: CloneWithProxy + SessionManager + Send + Sync,
{
    /// Configures the atproto-proxy header to be applied on requests.

    ///

    /// Returns a new client service with the proxy header configured.

    pub fn api_with_proxy(&self, did: Did, service_type: impl AsRef<str>) -> Service<Wrapper<M>> {
        Service::new(Arc::new(
            self.session_manager.clone_with_proxy(did, service_type),
        ))
    }

    pub fn weaver_api(&self) -> Service<Wrapper<M>> {
        self.api_with_proxy(
            Did::new("did:web:appview.weaver.sh".into()).expect("valid did"),
            &"atproto_weaver",
        )
    }

    pub async fn get_profile_appview(
        &self,
        actor: AtIdentifier,
    ) -> Result<Union<ProfileDataViewInnerRefs>, Error> {
        use crate::sh::weaver::actor::get_profile::*;
        let result = self
            .weaver_api()
            .sh
            .weaver
            .actor
            .get_profile(ParametersData { actor }.into())
            .await?;
        Ok(result.inner.clone())
    }
}

impl<M> Configure for WeaverAgent<M>
where
    M: Configure + SessionManager + Send + Sync,
{
    fn configure_endpoint(&self, endpoint: String) {
        self.session_manager.configure_endpoint(endpoint);
    }

    fn configure_labelers_header(&self, labeler_dids: Option<Vec<(Did, bool)>>) {
        self.session_manager.configure_labelers_header(labeler_dids);
    }

    fn configure_proxy_header(&self, did: Did, service_type: impl AsRef<str>) {
        self.session_manager
            .configure_proxy_header(did, service_type);
    }
}

pub struct Wrapper<M> {
    inner: Arc<M>,
}

impl<M> Wrapper<M>
where
    M: SessionManager + Send + Sync,
{
    pub fn new(inner: M) -> Self {
        Self {
            inner: Arc::new(inner),
        }
    }
}

impl<M> HttpClient for Wrapper<M>
where
    M: SessionManager + Send + Sync,
{
    async fn send_http(
        &self,
        request: Request<Vec<u8>>,
    ) -> Result<Response<Vec<u8>>, Box<dyn std::error::Error + Send + Sync + 'static>> {
        self.inner.send_http(request).await
    }
}

impl<M> XrpcClient for Wrapper<M>
where
    M: SessionManager + Send + Sync,
{
    fn base_uri(&self) -> String {
        self.inner.base_uri()
    }
    async fn send_xrpc<P, I, O, E>(
        &self,
        request: &XrpcRequest<P, I>,
    ) -> Result<OutputDataOrBytes<O>, XrpcError<E>>
    where
        P: Serialize + Send + Sync,
        I: Serialize + Send + Sync,
        O: DeserializeOwned + Send + Sync,
        E: DeserializeOwned + Send + Sync + Debug,
    {
        self.inner.send_xrpc(request).await
    }
}

impl<M> SessionManager for Wrapper<M>
where
    M: SessionManager + Send + Sync,
{
    async fn did(&self) -> Option<Did> {
        self.inner.did().await
    }
}

impl<M> Configure for Wrapper<M>
where
    M: Configure,
{
    fn configure_endpoint(&self, endpoint: String) {
        self.inner.configure_endpoint(endpoint);
    }
    fn configure_labelers_header(&self, labeler_dids: Option<Vec<(Did, bool)>>) {
        self.inner.configure_labelers_header(labeler_dids);
    }
    fn configure_proxy_header(&self, did: Did, service_type: impl AsRef<str>) {
        self.inner.configure_proxy_header(did, service_type);
    }
}

impl<M> CloneWithProxy for Wrapper<M>
where
    M: CloneWithProxy,
{
    fn clone_with_proxy(&self, did: Did, service_type: impl AsRef<str>) -> Self {
        Self {
            inner: Arc::new(self.inner.clone_with_proxy(did, service_type)),
        }
    }
}

impl<M> Clone for Wrapper<M>
where
    M: SessionManager + Send + Sync,
{
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl<M> Deref for Wrapper<M>
where
    M: SessionManager + Send + Sync,
{
    type Target = M;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}
