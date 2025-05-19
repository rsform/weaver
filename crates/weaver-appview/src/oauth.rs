use atrium_api::types::string::Did;
use atrium_identity::did::CommonDidResolver;
use atrium_identity::handle::AtprotoHandleResolver;
use atrium_oauth::DefaultHttpClient;
use atrium_oauth::store::session::Session;
use atrium_oauth::store::state::InternalStateData;
use diesel::prelude::*;
use diesel_async::RunQueryDsl;
use jose_jwk::Key;
use miette::IntoDiagnostic;
use miette::Result;
use miette::miette;
use weaver_common::agent::WeaverHttpClient;
use weaver_common::oauth::WeaverOAuthClient;
use weaver_common::resolver::HickoryDnsTxtResolver;

use crate::api_error::ApiError;
use crate::models::OauthRequest;
use crate::models::OauthSession;

/// Basic implementation of the atrium-common Store trait for a database using diesel-async and deadpool to store OAuth sessions.
pub struct DBSessionStore {
    pub db: crate::db::Db,
}

impl DBSessionStore {
    pub fn new(db: &crate::db::Db) -> Self {
        Self { db: db.clone() }
    }

    pub async fn get(&self, user_did: &Did) -> Result<Option<Session>> {
        use crate::schema::oauth_sessions::dsl::*;
        let mut conn = self.db.pool.get().await.into_diagnostic()?;

        let results: Vec<OauthSession> = oauth_sessions
            .filter(did.eq(user_did.as_str()))
            .limit(1)
            .select(OauthSession::as_select())
            .load(&mut conn)
            .await
            .into_diagnostic()?;

        if let Some(sess) = results.get(0) {
            let sess: Option<Session> = serde_json::from_value(sess.session.clone()).ok();
            Ok(sess)
        } else {
            Ok(None)
        }
    }

    async fn set(&self, key: Did, value: Session) -> Result<()> {
        use crate::schema::oauth_sessions::dsl::*;
        let mut conn = self.db.pool.get().await.into_diagnostic()?;
        // do an upsert or similar here?
        let sess = OauthSession {
            id: 0,
            did: key.as_str().to_string(),
            pds_url: value.token_set.aud.clone(),
            session: serde_json::to_value(&value).unwrap(),
            expiry: value.token_set.expires_at.map(|t| t.as_str().to_string()),
        };

        diesel::insert_into(oauth_sessions)
            .values(&sess)
            .execute(&mut conn)
            .await
            .into_diagnostic()?;

        Ok(())
    }

    async fn del(&self, key: &Did) -> Result<()> {
        use crate::schema::oauth_sessions::dsl::*;
        let mut conn = self.db.pool.get().await.into_diagnostic()?;
        let query = diesel::delete(oauth_sessions.filter(did.eq(key.as_str())));
        query.execute(&mut conn).await.into_diagnostic()?;
        Ok(())
    }

    async fn clear(&self) -> Result<()> {
        Err(miette!(
            "this would clear the whole fucking table in the database so nope!"
        ))
    }
}

impl atrium_common::store::Store<Did, Session> for DBSessionStore {
    type Error = ApiError;

    async fn get(&self, did: &Did) -> Result<Option<Session>, Self::Error> {
        Ok(self.get(did).await?)
    }

    async fn set(&self, key: Did, value: Session) -> Result<(), Self::Error> {
        Ok(self.set(key, value).await?)
    }

    async fn del(&self, key: &Did) -> Result<(), Self::Error> {
        Ok(self.del(key).await?)
    }

    async fn clear(&self) -> Result<(), Self::Error> {
        Ok(self.clear().await?)
    }
}

impl atrium_oauth::store::session::SessionStore for DBSessionStore {}

pub struct DBStateStore {
    pub db: crate::db::Db,
}

impl DBStateStore {
    pub fn new(db: &crate::db::Db) -> Self {
        Self { db: db.clone() }
    }

    pub async fn get(&self, key: &str) -> Result<Option<InternalStateData>> {
        use crate::schema::oauth_requests::dsl::*;
        let mut conn = self.db.pool.get().await.into_diagnostic()?;

        let results: Vec<OauthRequest> = oauth_requests
            .filter(state.eq(key))
            .limit(1)
            .select(OauthRequest::as_select())
            .load(&mut conn)
            .await
            .into_diagnostic()?;

        if let Some(req) = results.get(0) {
            if let Ok(key) = serde_json::from_value::<Key>(req.dpop_key.clone()) {
                Ok(Some(InternalStateData {
                    iss: req.auth_server_iss.clone(),
                    dpop_key: key,
                    verifier: req.pkce_verifier.clone(),
                    app_state: req.state.clone(),
                }))
            } else {
                Ok(None)
            }
        } else {
            Ok(None)
        }
    }
}

impl atrium_common::store::Store<String, InternalStateData> for DBStateStore {
    type Error = ApiError;

    async fn get(&self, key: &String) -> Result<Option<InternalStateData>, Self::Error> {
        Ok(self.get(key).await?)
    }

    async fn set(&self, key: String, value: InternalStateData) -> Result<(), Self::Error> {
        todo!()
    }

    async fn del(&self, key: &String) -> Result<(), Self::Error> {
        todo!()
    }

    async fn clear(&self) -> Result<(), Self::Error> {
        todo!()
    }
}

impl atrium_oauth::store::state::StateStore for DBStateStore {}

pub type AppviewOAuthSession = atrium_oauth::OAuthSession<
    DefaultHttpClient,
    CommonDidResolver<WeaverHttpClient>,
    AtprotoHandleResolver<HickoryDnsTxtResolver, WeaverHttpClient>,
    DBSessionStore,
>;

pub type AppviewOAuthClient = WeaverOAuthClient<DBStateStore, DBSessionStore>;
