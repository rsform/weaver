use atrium_api::types::string::Did;
use atrium_common::store::memory::MemoryStore;
use atrium_identity::{
    did::{CommonDidResolver, CommonDidResolverConfig, DEFAULT_PLC_DIRECTORY_URL},
    handle::{AtprotoHandleResolver, AtprotoHandleResolverConfig},
};
#[cfg(not(feature = "dev"))]
use atrium_oauth::AtprotoClientMetadata;
#[cfg(feature = "dev")]
use atrium_oauth::AtprotoLocalhostClientMetadata;
use atrium_oauth::{
    DefaultHttpClient, KnownScope, OAuthClient, OAuthClientConfig, OAuthResolverConfig, Scope,
    store::{
        session::{MemorySessionStore, Session},
        state::{InternalStateData, MemoryStateStore},
    },
};

use std::sync::Arc;

use crate::resolver::HickoryDnsTxtResolver;

pub fn default_oauth_client(
    url: impl AsRef<str>,
) -> Result<
    atrium_oauth::OAuthClient<
        MemoryStore<String, InternalStateData>,
        MemoryStore<Did, Session>,
        CommonDidResolver<DefaultHttpClient>,
        AtprotoHandleResolver<HickoryDnsTxtResolver, DefaultHttpClient>,
        DefaultHttpClient,
    >,
    atrium_oauth::Error,
> {
    let http_client = Arc::new(atrium_oauth::DefaultHttpClient::default());
    let keys = if cfg!(feature = "dev") { None } else { todo!() };
    let config = OAuthClientConfig {
        client_metadata: default_client_metadata(url.as_ref()),
        keys,
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
        client_id: host.to_string(),
        redirect_uris: make_redirect_uris(host),
        scopes: make_scopes(),
        token_endpoint_auth_method: AuthMethod::PrivateKeyJwt,
        grant_types: vec![GrantType::AuthorizationCode, GrantType::RefreshToken],
        token_endpoint_auth_signing_alg: Some(String::from("ES256")),
    }
}

#[inline]
fn make_redirect_uris(url: &str) -> Option<Vec<String>> {
    Some(vec![format!("{}/oauth/callback", url)])
}

#[inline]
fn make_scopes() -> Option<Vec<Scope>> {
    Some(vec![
        Scope::Known(KnownScope::Atproto),
        Scope::Known(KnownScope::TransitionGeneric),
    ])
}
