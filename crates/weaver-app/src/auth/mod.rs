mod storage;
use dioxus::CapturedError;
pub use storage::AuthStore;

mod state;
pub use state::AuthState;

use crate::fetch::Fetcher;
use dioxus::prelude::*;
#[cfg(all(feature = "fullstack-server", feature = "server"))]
use jacquard::oauth::types::OAuthClientMetadata;

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
pub async fn restore_session(
    _fetcher: Fetcher,
    _auth_state: Signal<AuthState>,
) -> Result<(), String> {
    Ok(())
}

#[cfg(target_arch = "wasm32")]
pub async fn restore_session(
    fetcher: Fetcher,
    mut auth_state: Signal<AuthState>,
) -> Result<(), CapturedError> {
    use dioxus::prelude::*;
    use gloo_storage::{LocalStorage, Storage};
    use jacquard::types::string::Did;
    // Look for session keys in localStorage (format: oauth_session_{did}_{session_id})
    let keys = LocalStorage::get_all::<serde_json::Value>()?;
    let mut found_session: Option<(String, String)> = None;

    let keys = keys
        .as_object()
        .ok_or(CapturedError::from_display(format!("{}", keys)))?;
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

    let (did_str, session_id) =
        found_session.ok_or(CapturedError::from_display("No saved session found"))?;
    let did = Did::new_owned(did_str)?;

    let session = fetcher
        .client
        .oauth_client
        .restore(&did, &session_id)
        .await?;

    // Get DID and handle from session
    let (restored_did, session_id) = session.session_info().await;

    // Update auth state
    auth_state
        .write()
        .set_authenticated(restored_did, session_id);
    fetcher.upgrade_to_authenticated(session).await;

    tracing::debug!("session restored");
    Ok(())
}
