use crate::auth::AuthState;
use crate::fetch::Fetcher;
use dioxus::prelude::*;
use jacquard::{
    IntoStatic,
    cowstr::ToCowStr,
    oauth::{error::OAuthError, types::CallbackParams},
    smol_str::SmolStr,
};
use tracing::{error, info};
use weaver_api::sh_weaver::actor::profile::Profile as WeaverProfile;

#[component]
pub fn Callback(
    state: ReadSignal<SmolStr>,
    iss: ReadSignal<SmolStr>,
    code: ReadSignal<SmolStr>,
) -> Element {
    let fetcher = use_context::<Fetcher>();
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
                let did_owned = did.into_static();
                auth.write()
                    .set_authenticated(did_owned.clone(), session_id);
                fetcher.upgrade_to_authenticated(session).await;

                // Create weaver profile if it doesn't exist
                if let Err(e) = ensure_weaver_profile(&fetcher, &did_owned).await {
                    error!("Failed to ensure weaver profile: {:?}", e);
                }

                Ok::<(), OAuthError>(())
            }
        })
    };
    #[cfg(not(feature = "web"))]
    let result = { use_resource(move || async { Ok::<(), OAuthError>(()) }) };
    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    let nav = use_navigator();

    match &*result.read_unchecked() {
        Some(Ok(())) => {
            #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
            {
                use gloo_storage::Storage;
                let mut prev = gloo_storage::LocalStorage::get::<String>("cached_route").ok();
                if let Some(prev) = prev.take() {
                    tracing::info!("Navigating to previous page");

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

/// Ensures a weaver profile exists for the authenticated user.
/// If no weaver profile exists, creates one by mirroring the bsky profile.
#[cfg(feature = "web")]
async fn ensure_weaver_profile(
    fetcher: &Fetcher,
    did: &jacquard::types::string::Did<'_>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use jacquard::{
        client::AgentSessionExt,
        types::string::{Datetime, RecordKey},
    };
    use weaver_api::app_bsky::actor::profile::Profile as BskyProfile;

    let weaver_uri_str = format!("at://{}/sh.weaver.actor.profile/self", did);
    let weaver_uri = WeaverProfile::uri(&weaver_uri_str)?;

    // Check if weaver profile already exists
    if fetcher.fetch_record(&weaver_uri).await.is_ok() {
        info!("Weaver profile already exists for {}", did);
        return Ok(());
    }

    info!(
        "No weaver profile found for {}, creating from bsky profile",
        did
    );

    // Fetch bsky profile
    let bsky_uri_str = format!("at://{}/app.bsky.actor.profile/self", did);
    let bsky_uri = BskyProfile::uri(&bsky_uri_str)?;
    let bsky_record = fetcher.fetch_record(&bsky_uri).await?;

    // Create weaver profile mirroring bsky
    let weaver_profile = WeaverProfile::new()
        .maybe_display_name(bsky_record.value.display_name.clone())
        .maybe_description(bsky_record.value.description.clone())
        .maybe_avatar(bsky_record.value.avatar.clone())
        .maybe_banner(bsky_record.value.banner.clone())
        .bluesky(true)
        .created_at(Datetime::now())
        .build();

    let self_rkey = RecordKey::any("self").expect("self is valid record key");

    fetcher.put_record(self_rkey, weaver_profile).await?;
    info!("Created weaver profile for {}", did);

    Ok(())
}
