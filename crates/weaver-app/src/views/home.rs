use crate::{Route, components::identity::NotebookCard, data};
use dioxus::prelude::*;
use jacquard::types::aturi::AtUri;

const NOTEBOOK_CARD_CSS: Asset = asset!("/assets/styling/notebook-card.css");

/// The Home page component that will be rendered when the current route is `[Route::Home]`
#[component]
pub fn Home() -> Element {
    // Fetch notebooks from UFOS with SSR support
    let notebooks = data::use_notebooks_from_ufos()?;
    let navigator = use_navigator();
    let mut uri_input = use_signal(|| String::new());

    let handle_uri_submit = move || {
        let input_uri = uri_input.read().clone();
        if !input_uri.is_empty() {
            if let Ok(parsed) = AtUri::new(&input_uri) {
                navigator.push(Route::RecordPage {
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
                match &*notebooks.read() {
                    Some(notebook_list) => rsx! {
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
                    _ => rsx! {
                        div { "Loading notebooks..." }
                    }
                }
            }
        }

    }
}
