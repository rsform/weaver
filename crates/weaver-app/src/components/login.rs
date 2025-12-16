use dioxus::logger::tracing::{error, info};
use dioxus::prelude::*;
use jacquard::oauth::client::OAuthClient;
use jacquard::oauth::session::ClientData;
use jacquard::{oauth::types::AuthorizeOptions, smol_str::SmolStr};

use crate::{CONFIG, Route};
use crate::{
    components::{
        button::{Button, ButtonVariant},
        dialog::{DialogContent, DialogRoot, DialogTitle},
        input::Input,
    },
    fetch::Fetcher,
};

fn handle_submit(
    full_route: Route,
    fetcher: Fetcher,
    mut error: Signal<Option<String>>,
    mut is_loading: Signal<bool>,
    handle_input: Signal<String>,
    mut open: Signal<bool>,
) {
    let handle = handle_input.read().clone();
    if handle.is_empty() {
        error.set(Some("Please enter a handle".to_string()));
        return;
    }

    is_loading.set(true);
    error.set(None);

    #[cfg(target_arch = "wasm32")]
    {
        use gloo_storage::Storage;
        gloo_storage::LocalStorage::set("cached_route", format!("{}", full_route)).ok();
        spawn(async move {
            match start_oauth_flow(handle, fetcher).await {
                Ok(_) => {
                    open.set(false);
                }
                Err(e) => {
                    error!("Authentication failed: {}", e);
                    error.set(Some(format!("Authentication failed: {}", e)));
                    is_loading.set(false);
                }
            }
        });
    }
}

#[component]
pub fn LoginModal(open: Signal<bool>) -> Element {
    let mut handle_input = use_signal(|| String::new());
    let error = use_signal(|| Option::<String>::None);
    let is_loading = use_signal(|| false);
    let full_route = use_route::<Route>();
    let fetcher = use_context::<Fetcher>();
    let submit_route = full_route.clone();
    let submit_fetcher = fetcher.clone();
    let submit_closure1 = move || {
        let submit_route = submit_route.clone();
        let submit_fetcher = submit_fetcher.clone();
        handle_submit(
            submit_route,
            submit_fetcher,
            error,
            is_loading,
            handle_input,
            open,
        );
    };

    let submit_closure2 = move || {
        let submit_route = full_route.clone();
        let submit_fetcher = fetcher.clone();
        handle_submit(
            submit_route,
            submit_fetcher,
            error,
            is_loading,
            handle_input,
            open,
        );
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
                                submit_closure1();
                            }
                        },
                        placeholder: "Enter your handle",
                        value: "{handle_input}",
                    }
                    if let Some(err) = error() {
                        div { class: "error", "{err}" }
                    }
                    Button {
                        r#type: "submit",
                        disabled: is_loading(),
                        onclick: move |_| {
                            submit_closure2();
                        },
                        if is_loading() { "Authenticating..." } else { "Sign In" }
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


            }
        }
    }
}

async fn start_oauth_flow(handle: String, fetcher: Fetcher) -> Result<(), SmolStr> {
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
