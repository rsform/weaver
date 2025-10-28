use dashmap::DashMap;
use jacquard::identity::JacquardResolver;
use jacquard::oauth::atproto::{AtprotoClientMetadata, GrantType};
use jacquard::oauth::client::OAuthClient;
use jacquard::oauth::keyset::Keyset;
use jacquard::oauth::scopes::Scope;
use jacquard::oauth::session::ClientData;
use std::sync::Arc;
use url::Url;

use crate::config::Config;
use crate::db::Db;
use crate::oauth::DBAuthStore;

pub type AppviewOAuthClient = OAuthClient<JacquardResolver, DBAuthStore>;
pub type AppviewOAuthSession = jacquard::oauth::client::OAuthSession<JacquardResolver, DBAuthStore>;

pub struct AppStateInner {
    pub cfg: Config,
    pub oauth_client: AppviewOAuthClient,
    pub active_sessions: DashMap<String, AppviewOAuthSession>,
}

#[derive(Clone)]
pub struct AppState {
    pub db: Db,
    inner: Arc<AppStateInner>,
}

impl AppState {
    pub fn new(cfg: Config, db: Db) -> Self {
        let store = DBAuthStore::new(&db);

        // Build keyset from config JWKs
        let keyset = Some(
            Keyset::try_from(cfg.oauth.jwks.clone()).expect("failed to create keyset from JWKs"),
        );

        // Build AT Protocol client metadata
        let client_id =
            Url::parse(&cfg.core.appview_host).expect("appview_host must be a valid URL");

        let redirect_uris = vec![
            Url::parse(&format!("{}/oauth/callback", cfg.core.appview_host))
                .expect("failed to build redirect URI"),
        ];

        let scopes =
            Scope::parse_multiple("atproto transition:generic").expect("failed to parse scopes");

        let config = AtprotoClientMetadata::new(
            client_id.clone(),
            Some(client_id),
            redirect_uris,
            vec![GrantType::AuthorizationCode, GrantType::RefreshToken],
            scopes,
            None, // jwks_uri - None means jwks will be embedded
        );

        let client_data = ClientData { keyset, config };

        let oauth_client = OAuthClient::new(store, client_data);

        Self {
            db,
            inner: Arc::new(AppStateInner {
                cfg,
                oauth_client,
                active_sessions: DashMap::new(),
            }),
        }
    }

    pub fn cfg(&self) -> &Config {
        &self.inner.as_ref().cfg
    }

    pub fn oauth_client(&self) -> &AppviewOAuthClient {
        &self.inner.as_ref().oauth_client
    }

    pub fn active_sessions(&self) -> &DashMap<String, AppviewOAuthSession> {
        &self.inner.as_ref().active_sessions
    }
}
