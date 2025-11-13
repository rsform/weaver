use crate::{Route, components::identity::NotebookCard, fetch};
use dioxus::prelude::*;
use jacquard::{IntoStatic, smol_str::ToSmolStr, types::aturi::AtUri};

const NOTEBOOK_CARD_CSS: Asset = asset!("/assets/styling/notebook-card.css");

/// The Home page component that will be rendered when the current route is `[Route::Home]`
#[component]
pub fn Home() -> Element {
    let fetcher = use_context::<fetch::CachedFetcher>();

    // Fetch notebooks from UFOS
    let notebooks = use_resource(move || {
        let fetcher = fetcher.clone();
        async move { fetcher.fetch_notebooks_from_ufos().await }
    });
    let navigator = use_navigator();
    let mut uri_input = use_signal(|| String::new());

    let handle_uri_submit = move || {
        let input_uri = uri_input.read().clone();
        if !input_uri.is_empty() {
            if let Ok(parsed) = AtUri::new(&input_uri) {
                navigator.push(Route::RecordView {
                    uri: vec![parsed.to_string()],
                });
            }
        }
    };

    rsx! {
        document::Link { rel: "stylesheet", href: NOTEBOOK_CARD_CSS }
        div {
            class: "record-view-container",
            div { class: "record-header",
                div { class: "uri-input-section",
                    input {
                        r#type: "text",
                        class: "uri-input",
                        placeholder: "at://did:plc:.../collection/rkey",
                        value: "{uri_input}",
                        oninput: move |evt| uri_input.set(evt.value()),
                        onkeydown: move |evt| {
                            if evt.key() == Key::Enter {
                                handle_uri_submit();
                            }
                        },
                    }
                }
            }
            div { class: "notebooks-list",
                match notebooks() {
                    Some(Ok(notebook_list)) => rsx! {
                        for notebook in notebook_list.iter() {
                            {
                                let view = &notebook.0;
                                let entries = &notebook.1;
                                rsx! {
                                    div {
                                        key: "{view.cid}",
                                        NotebookCard {
                                            notebook: view.clone(),
                                            entry_refs: entries.clone()
                                        }
                                    }
                                }
                            }
                        }
                    },
                    Some(Err(_)) => rsx! {
                        div { "Error loading notebooks" }
                    },
                    None => rsx! {
                        div { "Loading notebooks..." }
                    }
                }
            }
        }

    }
}
