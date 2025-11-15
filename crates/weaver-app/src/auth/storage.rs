#[cfg(target_arch = "wasm32")]
use gloo_storage::{LocalStorage, SessionStorage, Storage};
use jacquard::client::SessionStoreError;
use jacquard::oauth::authstore::ClientAuthStore;
#[cfg(not(target_arch = "wasm32"))]
use jacquard::oauth::authstore::MemoryAuthStore;
use jacquard::oauth::session::{AuthRequestData, ClientSessionData};
use jacquard::types::string::Did;
use std::future::Future;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::LazyLock;

#[cfg(target_arch = "wasm32")]
#[derive(Clone)]
pub struct AuthStore;

#[cfg(target_arch = "wasm32")]
impl AuthStore {
    pub fn new() -> Self {
        Self
    }

    fn session_key(did: &Did<'_>, session_id: &str) -> String {
        format!("oauth_session_{}_{}", did.as_ref(), session_id)
    }

    fn auth_req_key(state: &str) -> String {
        format!("oauth_auth_req_{}", state)
    }
}

#[cfg(target_arch = "wasm32")]
impl ClientAuthStore for AuthStore {
    fn get_session(
        &self,
        did: &Did<'_>,
        session_id: &str,
    ) -> impl Future<Output = Result<Option<ClientSessionData<'_>>, SessionStoreError>> {
        let key = Self::session_key(did, session_id);
        async move {
            match LocalStorage::get::<serde_json::Value>(&key) {
                Ok(value) => {
                    let data: ClientSessionData<'static> =
                        jacquard::from_json_value::<ClientSessionData>(value).map_err(|e| {
                            SessionStoreError::Other(format!("Deserialize error: {}", e).into())
                        })?;
                    Ok(Some(data))
                }
                Err(gloo_storage::errors::StorageError::KeyNotFound(_)) => Ok(None),
                Err(e) => Err(SessionStoreError::Other(
                    format!("LocalStorage error: {}", e).into(),
                )),
            }
        }
    }

    fn upsert_session(
        &self,
        session: ClientSessionData<'_>,
    ) -> impl Future<Output = Result<(), SessionStoreError>> {
        async move {
            use jacquard::IntoStatic;

            let key = Self::session_key(&session.account_did, &session.session_id);
            let static_session = session.into_static();

            let value = serde_json::to_value(&static_session)
                .map_err(|e| SessionStoreError::Other(format!("Serialize error: {}", e).into()))?;

            LocalStorage::set(&key, &value).map_err(|e| {
                SessionStoreError::Other(format!("LocalStorage error: {}", e).into())
            })?;

            Ok(())
        }
    }

    fn delete_session(
        &self,
        did: &Did<'_>,
        session_id: &str,
    ) -> impl Future<Output = Result<(), SessionStoreError>> {
        let key = Self::session_key(did, session_id);
        async move {
            LocalStorage::delete(&key);
            Ok(())
        }
    }

    fn get_auth_req_info(
        &self,
        state: &str,
    ) -> impl Future<Output = Result<Option<AuthRequestData<'_>>, SessionStoreError>> {
        let key = Self::auth_req_key(state);
        async move {
            match LocalStorage::get::<serde_json::Value>(&key) {
                Ok(value) => {
                    let data: AuthRequestData<'static> =
                        jacquard::from_json_value::<AuthRequestData>(value).map_err(|e| {
                            SessionStoreError::Other(format!("Deserialize error: {}", e).into())
                        })?;
                    Ok(Some(data))
                }
                Err(gloo_storage::errors::StorageError::KeyNotFound(err)) => {
                    tracing::debug!("gloo error: {}", err);
                    Ok(None)
                }
                Err(e) => Err(SessionStoreError::Other(
                    format!("SessionStorage error: {}", e).into(),
                )),
            }
        }
    }

    fn save_auth_req_info(
        &self,
        auth_req_info: &AuthRequestData<'_>,
    ) -> impl Future<Output = Result<(), SessionStoreError>> {
        async move {
            use jacquard::IntoStatic;

            let key = Self::auth_req_key(&auth_req_info.state);
            let static_info = auth_req_info.clone().into_static();

            let value = serde_json::to_value(&static_info)
                .map_err(|e| SessionStoreError::Other(format!("Serialize error: {}", e).into()))?;

            LocalStorage::set(&key, &value).map_err(|e| {
                SessionStoreError::Other(format!("SessionStorage error: {}", e).into())
            })?;

            Ok(())
        }
    }

    fn delete_auth_req_info(
        &self,
        state: &str,
    ) -> impl Future<Output = Result<(), SessionStoreError>> {
        let key = Self::auth_req_key(state);
        async move {
            LocalStorage::delete(&key);
            Ok(())
        }
    }
}
#[cfg(not(target_arch = "wasm32"))]
use std::sync::Arc;

#[cfg(not(target_arch = "wasm32"))]
pub struct AuthStore(Arc<MemoryAuthStore>);

#[cfg(not(target_arch = "wasm32"))]
static MEM_STORE: LazyLock<Arc<MemoryAuthStore>> =
    LazyLock::new(|| Arc::new(MemoryAuthStore::new()));

#[cfg(not(target_arch = "wasm32"))]
impl AuthStore {
    pub fn new() -> Self {
        Self(MEM_STORE.clone())
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl ClientAuthStore for AuthStore {
    fn get_session(
        &self,
        did: &Did<'_>,
        session_id: &str,
    ) -> impl Future<Output = Result<Option<ClientSessionData<'_>>, SessionStoreError>> {
        self.0.get_session(did, session_id)
    }

    fn upsert_session(
        &self,
        session: ClientSessionData<'_>,
    ) -> impl Future<Output = Result<(), SessionStoreError>> {
        self.0.upsert_session(session)
    }

    fn delete_session(
        &self,
        did: &Did<'_>,
        session_id: &str,
    ) -> impl Future<Output = Result<(), SessionStoreError>> {
        self.0.delete_session(did, session_id)
    }

    fn get_auth_req_info(
        &self,
        state: &str,
    ) -> impl Future<Output = Result<Option<AuthRequestData<'_>>, SessionStoreError>> {
        self.0.get_auth_req_info(state)
    }

    fn save_auth_req_info(
        &self,
        auth_req_info: &AuthRequestData<'_>,
    ) -> impl Future<Output = Result<(), SessionStoreError>> {
        self.0.save_auth_req_info(auth_req_info)
    }

    fn delete_auth_req_info(
        &self,
        state: &str,
    ) -> impl Future<Output = Result<(), SessionStoreError>> {
        self.0.delete_auth_req_info(state)
    }
}
