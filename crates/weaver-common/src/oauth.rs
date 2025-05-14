use atrium_api::types::string::Did;
use atrium_common::store::memory::MemoryStore;
use atrium_identity::{
    did::{CommonDidResolver, CommonDidResolverConfig, DEFAULT_PLC_DIRECTORY_URL},
    handle::{AtprotoHandleResolver, AtprotoHandleResolverConfig},
};
use atrium_oauth::{
    AtprotoLocalhostClientMetadata, DefaultHttpClient, KnownScope, OAuthClient, OAuthClientConfig,
    OAuthResolverConfig, Scope,
    store::{
        session::{MemorySessionStore, Session},
        state::{InternalStateData, MemoryStateStore},
    },
};

use std::sync::Arc;

use crate::HickoryDnsTxtResolver;

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
    let config = OAuthClientConfig {
        client_metadata: AtprotoLocalhostClientMetadata {
            redirect_uris: Some(vec![url.as_ref().to_string()]),
            scopes: Some(vec![
                Scope::Known(KnownScope::Atproto),
                Scope::Known(KnownScope::TransitionGeneric),
            ]),
        },
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
    let client = OAuthClient::new(config)?;
    Ok(client)
}
