use crate::auth::AuthState;
use crate::fetch::CachedFetcher;
use dioxus::prelude::*;
use jacquard::{
    IntoStatic,
    cowstr::ToCowStr,
    oauth::{error::OAuthError, types::CallbackParams},
    smol_str::SmolStr,
};
use tracing::{error, info};

#[component]
pub fn Callback(
    state: ReadSignal<SmolStr>,
    iss: ReadSignal<SmolStr>,
    code: ReadSignal<SmolStr>,
) -> Element {
    let fetcher = use_context::<CachedFetcher>();
    let mut auth = use_context::<Signal<AuthState>>();
    #[cfg(feature = "web")]
    let result = {
        use_resource(move || {
            let fetcher = fetcher.clone();
            let callback_params = CallbackParams {
                code: code().to_cowstr(),
                state: Some(state().to_cowstr()),
                iss: Some(iss().to_cowstr()),
            }
            .into_static();
            info!("Auth Callback: {:?}", callback_params);
            async move {
                let session = fetcher
                    .client
                    .oauth_client
                    .callback(callback_params)
                    .await?;
                let (did, session_id) = session.session_info().await;
                auth.write().set_authenticated(did, session_id);
                fetcher.upgrade_to_authenticated(session).await;
                Ok::<(), OAuthError>(())
            }
        })
    };
    #[cfg(not(feature = "web"))]
    let result = { use_resource(move || async { Ok::<(), OAuthError>(()) }) };

    match &*result.read_unchecked() {
        Some(Ok(())) => {
            #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
            {
                use gloo_storage::Storage;
                let mut prev = gloo_storage::LocalStorage::get::<String>("cached_route").ok();
                if let Some(prev) = prev.take() {
                    tracing::info!("Navigating to previous page");
                    let nav = use_navigator();
                    gloo_storage::LocalStorage::delete("cached_route");
                    nav.replace(prev);
                }
            }
            rsx! {
                div {
                    h1 { "Success" }
                    p { "You have successfully authenticated. You can close this browser window." }
                }
            }
        }
        Some(Err(err)) => {
            error!("Auth Error: {}", err);
            rsx! {

                div {
                    h1 { "Error" }
                    p { "{err}" }
                }
            }
        }
        None => rsx! {
            div {
                h1 { "Loading..." }
            }
        },
    }
}
