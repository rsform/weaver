use atrium_api::types::string::Did;
use atrium_common::store::memory::MemoryStore;
use atrium_identity::{
    did::{CommonDidResolver, CommonDidResolverConfig, DEFAULT_PLC_DIRECTORY_URL},
    handle::{AtprotoHandleResolver, AtprotoHandleResolverConfig},
};

#[cfg(not(feature = "dev"))]
use atrium_oauth::{AtprotoClientMetadata, GrantType};
use atrium_oauth::{
    AtprotoLocalhostClientMetadata, AuthorizeOptions, CallbackParams, OAuthSession,
    store::{session::SessionStore, state::StateStore},
};
use atrium_oauth::{
    DefaultHttpClient, KnownScope, OAuthClient, OAuthClientConfig, OAuthResolverConfig, Scope,
    store::{
        session::{MemorySessionStore, Session},
        state::{InternalStateData, MemoryStateStore},
    },
};

use std::sync::Arc;

use crate::{agent::WeaverHttpClient, resolver::HickoryDnsTxtResolver};

pub struct NativeOAuthClient {
    oauth: NativeBasicOAuthClient,
    pub http_client: Arc<WeaverHttpClient>,
}

pub type NativeBasicOAuthSession = OAuthSession<
    DefaultHttpClient,
    CommonDidResolver<WeaverHttpClient>,
    AtprotoHandleResolver<HickoryDnsTxtResolver, WeaverHttpClient>,
    MemoryStore<Did, Session>,
>;

pub struct NativeWeaverOAuthClient<STATE, SESS>
where
    STATE: StateStore + Send + Sync + 'static,
    SESS: SessionStore + Send + Sync + 'static,
    STATE::Error: std::error::Error + Send + Sync + 'static,
    SESS::Error: std::error::Error + Send + Sync + 'static,
{
    oauth: WeaverOAuthClient<STATE, SESS>,
    pub http_client: Arc<WeaverHttpClient>,
}
impl NativeOAuthClient {
    pub async fn authorize(
        &self,
        input: impl AsRef<str>,
        options: AuthorizeOptions,
    ) -> Result<String, atrium_oauth::Error> {
        self.oauth.authorize(input, options).await
    }

    pub async fn callback(
        &self,
        params: CallbackParams,
    ) -> Result<(NativeBasicOAuthSession, Option<String>), atrium_oauth::Error> {
        self.oauth.callback(params).await
    }
}

#[cfg(all(feature = "native", feature = "dev"))]
impl
    NativeWeaverOAuthClient<
        crate::filestore::SimpleJsonFileSessionStore<std::path::PathBuf>,
        crate::filestore::SimpleJsonFileSessionStore<std::path::PathBuf>,
    >
{
    pub async fn authorize(
        &self,
        input: impl AsRef<str>,
        options: AuthorizeOptions,
    ) -> Result<String, atrium_oauth::Error> {
        self.oauth.authorize(input, options).await
    }

    pub async fn callback(
        &self,
        params: CallbackParams,
    ) -> Result<(WeaverOAuthSession, Option<String>), atrium_oauth::Error> {
        self.oauth.callback(params).await
    }

    pub async fn restore(&self, did: &Did) -> Result<WeaverOAuthSession, atrium_oauth::Error> {
        self.oauth.restore(did).await
    }
}

#[cfg(all(feature = "native", feature = "dev"))]
pub type WeaverOAuthSession = OAuthSession<
    DefaultHttpClient,
    CommonDidResolver<WeaverHttpClient>,
    AtprotoHandleResolver<HickoryDnsTxtResolver, WeaverHttpClient>,
    crate::filestore::SimpleJsonFileSessionStore<std::path::PathBuf>,
>;

pub type NativeBasicOAuthClient = atrium_oauth::OAuthClient<
    MemoryStore<String, InternalStateData>,
    MemoryStore<Did, Session>,
    CommonDidResolver<WeaverHttpClient>,
    AtprotoHandleResolver<HickoryDnsTxtResolver, WeaverHttpClient>,
    DefaultHttpClient,
>;

pub type BasicOAuthClient = atrium_oauth::OAuthClient<
    MemoryStore<String, InternalStateData>,
    MemoryStore<Did, Session>,
    CommonDidResolver<WeaverHttpClient>,
    AtprotoHandleResolver<HickoryDnsTxtResolver, WeaverHttpClient>,
    DefaultHttpClient,
>;

pub type WeaverOAuthClient<STATE, SESS> = atrium_oauth::OAuthClient<
    STATE,
    SESS,
    CommonDidResolver<WeaverHttpClient>,
    AtprotoHandleResolver<HickoryDnsTxtResolver, WeaverHttpClient>,
    DefaultHttpClient,
>;

pub fn oauth_client<STATE, SESS>(
    url: impl AsRef<str>,
    jwks: Option<Vec<jose_jwk::Jwk>>,
    session_store: SESS,
    state_store: STATE,
) -> Result<WeaverOAuthClient<STATE, SESS>, atrium_oauth::Error>
where
    STATE: StateStore + Send + Sync + 'static,
    SESS: SessionStore + Send + Sync + 'static,
    STATE::Error: std::error::Error + Send + Sync + 'static,
    SESS::Error: std::error::Error + Send + Sync + 'static,
{
    let http_client = Arc::new(WeaverHttpClient::default());
    let config = OAuthClientConfig {
        client_metadata: default_client_metadata(url.as_ref()),
        keys: jwks.into(),
        resolver: OAuthResolverConfig {
            did_resolver: CommonDidResolver::new(CommonDidResolverConfig {
                plc_directory_url: DEFAULT_PLC_DIRECTORY_URL.to_string(),
                http_client: Arc::clone(&http_client),
            }),
            handle_resolver: AtprotoHandleResolver::new(AtprotoHandleResolverConfig {
                dns_txt_resolver: HickoryDnsTxtResolver::default(),
                http_client: Arc::clone(&http_client),
            }),
            authorization_server_metadata: Default::default(),
            protected_resource_metadata: Default::default(),
        },
        state_store,
        session_store,
    };
    let client = OAuthClient::new(config)?;
    Ok(client)
}

pub fn default_oauth_client(
    url: impl AsRef<str>,
    jwks: Option<Vec<jose_jwk::Jwk>>,
) -> Result<BasicOAuthClient, atrium_oauth::Error> {
    let http_client = Arc::new(WeaverHttpClient::default());
    let config = OAuthClientConfig {
        client_metadata: default_client_metadata(url.as_ref()),
        keys: jwks.into(),
        resolver: OAuthResolverConfig {
            did_resolver: CommonDidResolver::new(CommonDidResolverConfig {
                plc_directory_url: DEFAULT_PLC_DIRECTORY_URL.to_string(),
                http_client: Arc::clone(&http_client),
            }),
            handle_resolver: AtprotoHandleResolver::new(AtprotoHandleResolverConfig {
                dns_txt_resolver: HickoryDnsTxtResolver::default(),
                http_client: Arc::clone(&http_client),
            }),
            authorization_server_metadata: Default::default(),
            protected_resource_metadata: Default::default(),
        },
        state_store: MemoryStateStore::default(),
        session_store: MemorySessionStore::default(),
    };
    let client = OAuthClient::new(config)?;
    Ok(client)
}

#[cfg(all(feature = "native", feature = "dev"))]
pub fn test_native_oauth_client() -> Result<
    NativeWeaverOAuthClient<
        crate::filestore::SimpleJsonFileSessionStore<std::path::PathBuf>,
        crate::filestore::SimpleJsonFileSessionStore<std::path::PathBuf>,
    >,
    atrium_oauth::Error,
> {
    use crate::filestore::SimpleJsonFileSessionStore;

    let http_client = Arc::new(WeaverHttpClient::default());
    let config = OAuthClientConfig {
        client_metadata: default_native_client_metadata(),
        keys: None,
        resolver: OAuthResolverConfig {
            did_resolver: CommonDidResolver::new(CommonDidResolverConfig {
                plc_directory_url: DEFAULT_PLC_DIRECTORY_URL.to_string(),
                http_client: Arc::clone(&http_client),
            }),
            handle_resolver: AtprotoHandleResolver::new(AtprotoHandleResolverConfig {
                dns_txt_resolver: HickoryDnsTxtResolver::default(),
                http_client: Arc::clone(&http_client),
            }),
            authorization_server_metadata: Default::default(),
            protected_resource_metadata: Default::default(),
        },
        state_store: SimpleJsonFileSessionStore::default(),
        session_store: SimpleJsonFileSessionStore::default(),
    };
    let client = NativeWeaverOAuthClient {
        oauth: OAuthClient::new(config)?,
        http_client: Arc::clone(&http_client),
    };
    Ok(client)
}

pub fn default_native_oauth_client() -> Result<NativeOAuthClient, atrium_oauth::Error> {
    let http_client = Arc::new(WeaverHttpClient::default());
    let config = OAuthClientConfig {
        client_metadata: default_native_client_metadata(),
        keys: None,
        resolver: OAuthResolverConfig {
            did_resolver: CommonDidResolver::new(CommonDidResolverConfig {
                plc_directory_url: DEFAULT_PLC_DIRECTORY_URL.to_string(),
                http_client: Arc::clone(&http_client),
            }),
            handle_resolver: AtprotoHandleResolver::new(AtprotoHandleResolverConfig {
                dns_txt_resolver: HickoryDnsTxtResolver::default(),
                http_client: Arc::clone(&http_client),
            }),
            authorization_server_metadata: Default::default(),
            protected_resource_metadata: Default::default(),
        },
        state_store: MemoryStateStore::default(),
        session_store: MemorySessionStore::default(),
    };
    let client = NativeOAuthClient {
        oauth: OAuthClient::new(config)?,
        http_client: Arc::clone(&http_client),
    };
    Ok(client)
}

#[cfg(feature = "dev")]
pub fn default_client_metadata(_host: &str) -> AtprotoLocalhostClientMetadata {
    AtprotoLocalhostClientMetadata {
        redirect_uris: make_redirect_uris("http://127.0.0.1:4000"),
        scopes: make_scopes(),
    }
}

#[cfg(not(feature = "dev"))]
pub fn default_client_metadata(host: &str) -> AtprotoClientMetadata {
    AtprotoClientMetadata {
        client_id: format!("{}/oauth/client-metadata.json", host),
        client_uri: Some(host.to_string()),
        jwks_uri: Some(format!("{}/oauth/jwks.json", host)),
        redirect_uris: make_redirect_uris(host).unwrap(),
        scopes: make_scopes().unwrap(),
        token_endpoint_auth_method: atrium_oauth::AuthMethod::PrivateKeyJwt,
        grant_types: vec![GrantType::AuthorizationCode, GrantType::RefreshToken],
        token_endpoint_auth_signing_alg: Some(String::from("ES256")),
    }
}

pub fn default_native_client_metadata() -> AtprotoLocalhostClientMetadata {
    AtprotoLocalhostClientMetadata {
        redirect_uris: make_redirect_uris("http://127.0.0.1:4000"),
        scopes: make_scopes(),
    }
}

#[inline]
fn make_redirect_uris(url: &str) -> Option<Vec<String>> {
    Some(vec![format!("{}/oauth/callback", url)])
}

#[inline]
pub fn make_scopes() -> Option<Vec<Scope>> {
    Some(vec![
        Scope::Known(KnownScope::Atproto),
        Scope::Known(KnownScope::TransitionGeneric),
    ])
}
