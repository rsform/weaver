use crate::oauth::AppviewOAuthClient;
use dashmap::DashMap;

use crate::config::Config;
use crate::db::Db;
use crate::oauth::{AppviewOAuthSession, DBSessionStore, DBStateStore};
use std::sync::Arc;

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
        let oauth_client = weaver_common::oauth::oauth_client(
            &cfg.core.appview_host,
            Some(cfg.oauth.jwks.clone()),
            DBSessionStore::new(&db),
            DBStateStore::new(&db),
        )
        .unwrap();
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
