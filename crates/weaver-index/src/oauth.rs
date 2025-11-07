use diesel::prelude::*;
use diesel_async::RunQueryDsl;
use jacquard::oauth::authstore::ClientAuthStore;
use jacquard::oauth::session::{AuthRequestData, ClientSessionData};
use jacquard::session::SessionStoreError;
use jacquard::types::string::Did;
use miette::IntoDiagnostic;

use crate::models::{NewOauthAuthRequest, NewOauthSession, OauthAuthRequest, OauthSession};

/// Database-backed implementation of ClientAuthStore for jacquard OAuth
pub struct DBAuthStore {
    pub db: crate::db::Db,
}

impl DBAuthStore {
    pub fn new(db: &crate::db::Db) -> Self {
        Self { db: db.clone() }
    }
}

impl ClientAuthStore for DBAuthStore {
    async fn get_session(
        &self,
        did_param: &Did<'_>,
        session_id_param: &str,
    ) -> Result<Option<ClientSessionData<'_>>, SessionStoreError> {
        use crate::schema::oauth_sessions::dsl::*;

        let mut conn = self
            .db
            .pool
            .get()
            .await
            .map_err(|e| SessionStoreError::Other(Box::new(e)))?;

        let result: Vec<OauthSession> = oauth_sessions
            .filter(did.eq(did_param.as_str()))
            .filter(session_id.eq(session_id_param))
            .limit(1)
            .select(OauthSession::as_select())
            .load(&mut conn)
            .await
            .map_err(|e| SessionStoreError::Other(Box::new(e)))?;

        if let Some(sess) = result.get(0) {
            let data = jacquard::from_json_value::<ClientSessionData>(sess.session_data.clone())
                .map_err(|e| SessionStoreError::Other(Box::new(e)))?;
            Ok(Some(data))
        } else {
            Ok(None)
        }
    }

    async fn upsert_session(
        &self,
        session: ClientSessionData<'_>,
    ) -> Result<(), SessionStoreError> {
        use crate::schema::oauth_sessions::dsl::*;

        let mut conn = self
            .db
            .pool
            .get()
            .await
            .map_err(|e| SessionStoreError::Other(Box::new(e)))?;

        let session_json =
            serde_json::to_value(&session).map_err(|e| SessionStoreError::Other(Box::new(e)))?;

        let new_session = NewOauthSession {
            did: session.account_did.as_str().to_string(),
            session_id: session.session_id.as_ref().to_string(),
            session_data: session_json,
        };

        // Try insert, on conflict update
        diesel::insert_into(oauth_sessions)
            .values(&new_session)
            .on_conflict((did, session_id))
            .do_update()
            .set((
                session_data.eq(&new_session.session_data),
                updated_at.eq(diesel::dsl::now),
            ))
            .execute(&mut conn)
            .await
            .map_err(|e| SessionStoreError::Other(Box::new(e)))?;

        Ok(())
    }

    async fn delete_session(
        &self,
        did_param: &Did<'_>,
        session_id_param: &str,
    ) -> Result<(), SessionStoreError> {
        use crate::schema::oauth_sessions::dsl::*;

        let mut conn = self
            .db
            .pool
            .get()
            .await
            .map_err(|e| SessionStoreError::Other(Box::new(e)))?;

        diesel::delete(
            oauth_sessions
                .filter(did.eq(did_param.as_str()))
                .filter(session_id.eq(session_id_param)),
        )
        .execute(&mut conn)
        .await
        .map_err(|e| SessionStoreError::Other(Box::new(e)))?;

        Ok(())
    }

    async fn get_auth_req_info(
        &self,
        state_param: &str,
    ) -> Result<Option<AuthRequestData<'_>>, SessionStoreError> {
        use crate::schema::oauth_auth_requests::dsl::*;

        let mut conn = self
            .db
            .pool
            .get()
            .await
            .map_err(|e| SessionStoreError::Other(Box::new(e)))?;

        let result: Vec<OauthAuthRequest> = oauth_auth_requests
            .filter(state.eq(state_param))
            .limit(1)
            .select(OauthAuthRequest::as_select())
            .load(&mut conn)
            .await
            .map_err(|e| SessionStoreError::Other(Box::new(e)))?;

        if let Some(req) = result.get(0) {
            let auth_data = jacquard::from_json_value::<AuthRequestData>(req.auth_req_data.clone())
                .map_err(|e| SessionStoreError::Other(Box::new(e)))?;
            Ok(Some(auth_data))
        } else {
            Ok(None)
        }
    }

    async fn save_auth_req_info(
        &self,
        auth_req_info: &AuthRequestData<'_>,
    ) -> Result<(), SessionStoreError> {
        use crate::schema::oauth_auth_requests::dsl::*;

        let mut conn = self
            .db
            .pool
            .get()
            .await
            .map_err(|e| SessionStoreError::Other(Box::new(e)))?;

        let auth_json = serde_json::to_value(auth_req_info)
            .map_err(|e| SessionStoreError::Other(Box::new(e)))?;

        let new_auth_req = NewOauthAuthRequest {
            state: auth_req_info.state.as_ref().to_string(),
            account_did: auth_req_info
                .account_did
                .as_ref()
                .map(|d| d.as_str().to_string()),
            auth_req_data: auth_json,
        };

        diesel::insert_into(oauth_auth_requests)
            .values(&new_auth_req)
            .execute(&mut conn)
            .await
            .map_err(|e| SessionStoreError::Other(Box::new(e)))?;

        Ok(())
    }

    async fn delete_auth_req_info(&self, state_param: &str) -> Result<(), SessionStoreError> {
        use crate::schema::oauth_auth_requests::dsl::*;

        let mut conn = self
            .db
            .pool
            .get()
            .await
            .map_err(|e| SessionStoreError::Other(Box::new(e)))?;

        diesel::delete(oauth_auth_requests.filter(state.eq(state_param)))
            .execute(&mut conn)
            .await
            .map_err(|e| SessionStoreError::Other(Box::new(e)))?;

        Ok(())
    }
}
