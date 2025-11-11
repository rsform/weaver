use dioxus::logger::tracing::{error, info};
use dioxus::prelude::*;
use jacquard::oauth::client::OAuthClient;
use jacquard::oauth::session::ClientData;
use jacquard::{oauth::types::AuthorizeOptions, smol_str::SmolStr};

use crate::CONFIG;
use crate::{
    components::{
        button::{Button, ButtonVariant},
        dialog::{DialogContent, DialogRoot, DialogTitle},
        input::Input,
    },
    fetch::CachedFetcher,
};

#[component]
pub fn LoginModal(open: Signal<bool>) -> Element {
    let mut handle_input = use_signal(|| String::new());
    let mut error = use_signal(|| Option::<String>::None);
    let mut is_loading = use_signal(|| false);

    let mut handle_submit = move || {
        let handle = handle_input.read().clone();
        if handle.is_empty() {
            error.set(Some("Please enter a handle".to_string()));
            return;
        }

        is_loading.set(true);
        error.set(None);

        let fetcher = use_context::<CachedFetcher>();

        #[cfg(target_arch = "wasm32")]
        {
            use crate::Route;
            use gloo_storage::Storage;
            let full_route = use_route::<Route>();
            gloo_storage::LocalStorage::set("cached_route", format!("{}", full_route));
        }

        use_effect(move || {
            let handle = handle.clone();
            let fetcher = fetcher.clone();
            spawn(async move {
                if let Err(e) = start_oauth_flow(handle, fetcher).await {
                    error!("Authentication failed: {}", e);
                    error.set(Some(format!("Authentication failed: {}", e)));
                    is_loading.set(false);
                }
                open.set(false);
            });
        });
    };

    rsx! {
        DialogRoot { open: open(), on_open_change: move |v| open.set(v),
            DialogContent {
                button {
                    class: "dialog-close",
                    r#type: "button",
                    aria_label: "Close",
                    tabindex: if open() { "0" } else { "-1" },
                    onclick: move |_| {
                        open.set(false)
                    },
                    "×"
                }
                DialogTitle { "Sign In with AT Protocol" }
                    Input {
                        oninput: move |e: FormEvent| handle_input.set(e.value()),
                        onkeypress: move |k: KeyboardEvent| {
                            if k.key() == Key::Enter {
                                handle_submit();
                            }
                        },
                        placeholder: "Enter your handle",
                        value: "{handle_input}",
                    }
                    if let Some(err) = error() {
                        div { class: "error", "{err}" }
                    }
                    Button {
                        r#type: "button",
                        onclick: move |_| {
                            open.set(false)
                        },
                        disabled: is_loading(),
                        variant: ButtonVariant::Secondary,
                        "Cancel"
                    }
                    Button {
                        r#type: "submit",
                        disabled: is_loading(),
                        onclick: move |_| {
                            handle_submit();
                        },
                        if is_loading() { "Authenticating..." } else { "Sign In" }
                    }

            }
        }
    }
}

async fn start_oauth_flow(handle: String, fetcher: CachedFetcher) -> Result<(), SmolStr> {
    info!("Starting OAuth flow for handle: {}", handle);

    let client_data = ClientData {
        keyset: fetcher
            .client
            .oauth_client
            .registry
            .client_data
            .keyset
            .clone(),
        config: CONFIG.oauth.clone(),
    };

    // Build client using store and resolver
    let flow_client = OAuthClient::new_with_shared(
        fetcher.client.oauth_client.registry.store.clone(),
        fetcher.client.oauth_client.client.clone(),
        client_data.clone(),
    );

    let auth_url = flow_client
        .start_auth(handle, AuthorizeOptions::default())
        .await
        .map_err(|e| format!("{:?}", e))?;
    #[cfg(target_arch = "wasm32")]
    {
        let window = web_sys::window().ok_or("no window")?;
        let location = window.location();
        location
            .set_href(&auth_url)
            .map_err(|e| format!("{:?}", e))?;
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        webbrowser::open(&auth_url).map_err(|e| format!("{:?}", e))?;
    }
    Ok(())
}
