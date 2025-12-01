mod storage;
pub use storage::AuthStore;

mod state;
pub use state::AuthState;

use crate::fetch::Fetcher;
use dioxus::prelude::*;
#[cfg(all(feature = "fullstack-server", feature = "server"))]
use jacquard::oauth::types::OAuthClientMetadata;

/// Result of attempting to restore a session
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestoreResult {
    /// Session was successfully restored
    Restored,
    /// No saved session was found
    NoSession,
    /// Session was found but expired/invalid and has been cleared
    SessionExpired,
}

#[cfg(all(feature = "fullstack-server", feature = "server"))]
#[get("/oauth-client-metadata.json")]
pub async fn client_metadata() -> Result<axum::Json<serde_json::Value>> {
    use jacquard::oauth::atproto::atproto_client_metadata;

    use crate::CONFIG;

    let atproto_metadata = atproto_client_metadata(CONFIG.oauth.clone(), &None)?;

    Ok(axum::response::Json(serde_json::to_value(
        atproto_metadata,
    )?))
}

#[cfg(not(target_arch = "wasm32"))]
pub async fn restore_session(_fetcher: Fetcher, _auth_state: Signal<AuthState>) -> RestoreResult {
    RestoreResult::NoSession
}

#[cfg(target_arch = "wasm32")]
pub async fn restore_session(fetcher: Fetcher, mut auth_state: Signal<AuthState>) -> RestoreResult {
    use gloo_storage::{LocalStorage, Storage};
    use jacquard::oauth::authstore::ClientAuthStore;
    use jacquard::types::string::Did;

    // Look for session keys in localStorage (format: oauth_session_{did}_{session_id})
    let Ok(keys) = LocalStorage::get_all::<serde_json::Value>() else {
        return RestoreResult::NoSession;
    };
    let Some(keys) = keys.as_object() else {
        return RestoreResult::NoSession;
    };

    let mut found_session: Option<(String, String)> = None;
    for key in keys.keys() {
        if key.starts_with("oauth_session_") {
            let parts: Vec<&str> = key
                .strip_prefix("oauth_session_")
                .unwrap()
                .split('_')
                .collect();
            if parts.len() >= 2 {
                found_session = Some((parts[0].to_string(), parts[1..].join("_")));
                break;
            }
        }
    }

    let Some((did_str, session_id)) = found_session else {
        return RestoreResult::NoSession;
    };
    let Ok(did) = Did::new_owned(did_str) else {
        return RestoreResult::NoSession;
    };

    match fetcher.client.oauth_client.restore(&did, &session_id).await {
        Ok(session) => {
            let (restored_did, session_id) = session.session_info().await;
            auth_state
                .write()
                .set_authenticated(restored_did, session_id);
            fetcher.upgrade_to_authenticated(session).await;
            tracing::debug!("session restored");
            RestoreResult::Restored
        }
        Err(e) => {
            tracing::warn!("Session restore failed, clearing dead session: {e}");
            let _ = AuthStore::new().delete_session(&did, &session_id).await;
            RestoreResult::SessionExpired
        }
    }
}
