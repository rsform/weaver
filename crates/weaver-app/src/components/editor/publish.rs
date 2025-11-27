//! Entry publishing functionality for the markdown editor.
//!
//! Handles creating/updating AT Protocol notebook entries from editor state.

use dioxus::prelude::*;
use jacquard::types::ident::AtIdentifier;
use jacquard::types::string::{AtUri, Datetime, Nsid};
use jacquard::{IntoStatic, prelude::*, to_data};
use weaver_api::com_atproto::repo::{create_record::CreateRecord, put_record::PutRecord};
use weaver_api::sh_weaver::embed::images::Images;
use weaver_api::sh_weaver::notebook::entry::{Entry, EntryEmbeds};
use weaver_common::{WeaverError, WeaverExt};

const ENTRY_NSID: &str = "sh.weaver.notebook.entry";

use crate::auth::AuthState;
use crate::fetch::Fetcher;

use super::document::EditorDocument;
use super::storage::delete_draft;

/// Result of a publish operation.
#[derive(Clone, Debug)]
pub enum PublishResult {
    /// Entry was created (new)
    Created(AtUri<'static>),
    /// Entry was updated (existing)
    Updated(AtUri<'static>),
}

impl PublishResult {
    pub fn uri(&self) -> &AtUri<'static> {
        match self {
            PublishResult::Created(uri) | PublishResult::Updated(uri) => uri,
        }
    }
}

/// Publish an entry to the AT Protocol.
///
/// Supports three modes:
/// - With notebook_title: uses `upsert_entry` to publish to a notebook
/// - Without notebook but with entry_uri in doc: uses `put_record` to update existing
/// - Without notebook and no entry_uri: uses `create_record` for free-floating entry
///
/// # Arguments
/// * `fetcher` - The authenticated fetcher/client
/// * `doc` - The editor document containing entry data
/// * `notebook_title` - Optional title of the notebook to publish to
/// * `draft_key` - Storage key for the draft (for cleanup)
///
/// # Returns
/// The AT-URI of the created/updated entry, or an error.
pub async fn publish_entry(
    fetcher: &Fetcher,
    doc: &EditorDocument,
    notebook_title: Option<&str>,
    draft_key: &str,
) -> Result<PublishResult, WeaverError> {
    // Get images from the document
    let editor_images = doc.images();

    // Build embeds if we have images
    let entry_embeds = if editor_images.is_empty() {
        None
    } else {
        // Extract Image types from EditorImage wrappers
        let images: Vec<_> = editor_images.iter().map(|ei| ei.image.clone()).collect();

        Some(EntryEmbeds {
            images: Some(Images {
                images,
                extra_data: None,
            }),
            ..Default::default()
        })
    };

    // Build tags (convert Vec<String> to the expected type)
    let tags = {
        let tag_strings = doc.tags();
        if tag_strings.is_empty() {
            None
        } else {
            Some(tag_strings.into_iter().map(Into::into).collect())
        }
    };

    // Determine path - use doc path if set, otherwise slugify title
    let path = {
        let doc_path = doc.path();
        if doc_path.is_empty() {
            slugify(&doc.title())
        } else {
            doc_path
        }
    };

    // Build the entry
    let entry = Entry::new()
        .content(doc.content())
        .title(doc.title())
        .path(path)
        .created_at(Datetime::now())
        .maybe_tags(tags)
        .maybe_embeds(entry_embeds)
        .build();
    let entry_data = to_data(&entry).unwrap();

    let client = fetcher.get_client();
    let result = if let Some(notebook) = notebook_title {
        // Publish to a notebook via upsert_entry
        let (uri, was_created) = client.upsert_entry(notebook, &doc.title(), entry).await?;

        if was_created {
            PublishResult::Created(uri)
        } else {
            PublishResult::Updated(uri)
        }
    } else if let Some(existing_uri) = doc.entry_uri() {
        // Update existing free-floating entry
        let did = fetcher
            .current_did()
            .await
            .ok_or_else(|| WeaverError::InvalidNotebook("Not authenticated".into()))?;

        let rkey = existing_uri
            .rkey()
            .ok_or_else(|| WeaverError::InvalidNotebook("Entry URI missing rkey".into()))?;

        let collection = Nsid::new(ENTRY_NSID).map_err(|e| WeaverError::AtprotoString(e))?;

        let request = PutRecord::new()
            .repo(AtIdentifier::Did(did))
            .collection(collection)
            .rkey(rkey.clone())
            .record(entry_data)
            .build();

        let response = fetcher
            .send(request)
            .await
            .map_err(jacquard::client::AgentError::from)?;
        let output = response
            .into_output()
            .map_err(|e| WeaverError::InvalidNotebook(e.to_string()))?;

        PublishResult::Updated(output.uri.into_static())
    } else {
        // Create new free-floating entry
        let did = fetcher
            .current_did()
            .await
            .ok_or_else(|| WeaverError::InvalidNotebook("Not authenticated".into()))?;

        let collection = Nsid::new(ENTRY_NSID).map_err(|e| WeaverError::AtprotoString(e))?;

        let request = CreateRecord::new()
            .repo(AtIdentifier::Did(did))
            .collection(collection)
            .record(entry_data)
            .build();

        let response = fetcher
            .send(request)
            .await
            .map_err(jacquard::client::AgentError::from)?;
        let output = response
            .into_output()
            .map_err(|e| WeaverError::InvalidNotebook(e.to_string()))?;

        PublishResult::Created(output.uri.into_static())
    };

    // Cleanup: delete PublishedBlob records (entry's embed refs now keep blobs alive)
    // TODO: Implement when image upload is added
    // for img in &editor_images {
    //     if let Some(ref published_uri) = img.published_blob_uri {
    //         let _ = delete_published_blob(fetcher, published_uri).await;
    //     }
    // }

    // Clear local draft
    delete_draft(draft_key);

    Ok(result)
}

/// Simple slug generation from title.
fn slugify(title: &str) -> String {
    title
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c
            } else if c.is_whitespace() || c == '-' || c == '_' {
                '-'
            } else {
                // Skip other characters
                '\0'
            }
        })
        .filter(|&c| c != '\0')
        .collect::<String>()
        // Collapse multiple dashes
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

/// Props for the publish button component.
#[derive(Props, Clone, PartialEq)]
pub struct PublishButtonProps {
    /// The editor document
    pub document: EditorDocument,
    /// Storage key for the draft
    pub draft_key: String,
}

/// Publish button component with notebook selection.
#[component]
pub fn PublishButton(props: PublishButtonProps) -> Element {
    let fetcher = use_context::<Fetcher>();
    let auth_state = use_context::<Signal<AuthState>>();

    let mut show_dialog = use_signal(|| false);
    let mut notebook_title = use_signal(|| String::from("Default"));
    let mut use_notebook = use_signal(|| true);
    let mut is_publishing = use_signal(|| false);
    let mut error_message: Signal<Option<String>> = use_signal(|| None);
    let mut success_uri: Signal<Option<AtUri<'static>>> = use_signal(|| None);

    let is_authenticated = auth_state.read().is_authenticated();
    let doc = props.document.clone();
    let draft_key = props.draft_key.clone();

    // Check if we're editing an existing entry
    let is_editing_existing = doc.entry_uri().is_some();

    // Validate that we have required fields
    let can_publish = !doc.title().trim().is_empty() && !doc.content().trim().is_empty();

    let open_dialog = move |_| {
        error_message.set(None);
        success_uri.set(None);
        show_dialog.set(true);
    };

    let close_dialog = move |_| {
        show_dialog.set(false);
    };

    let draft_key_clone = draft_key.clone();
    let doc_for_publish = doc.clone();
    let do_publish = move |_| {
        let fetcher = fetcher.clone();
        let draft_key = draft_key_clone.clone();
        let doc_snapshot = doc_for_publish.clone();
        let notebook = if use_notebook() {
            Some(notebook_title())
        } else {
            None
        };

        spawn(async move {
            is_publishing.set(true);
            error_message.set(None);

            match publish_entry(&fetcher, &doc_snapshot, notebook.as_deref(), &draft_key).await {
                Ok(result) => {
                    success_uri.set(Some(result.uri().clone()));
                }
                Err(e) => {
                    error_message.set(Some(format!("{}", e)));
                }
            }

            is_publishing.set(false);
        });
    };

    rsx! {
        button {
            class: "publish-button",
            disabled: !is_authenticated || !can_publish,
            onclick: open_dialog,
            title: if !is_authenticated {
                "Log in to publish"
            } else if !can_publish {
                "Title and content required"
            } else {
                "Publish entry"
            },
            "Publish"
        }

        if show_dialog() {
            div {
                class: "publish-dialog-overlay",
                onclick: close_dialog,

                div {
                    class: "publish-dialog",
                    onclick: move |e| e.stop_propagation(),

                    h2 { "Publish Entry" }

                    if let Some(uri) = success_uri() {
                        div { class: "publish-success",
                            p { "Entry published successfully!" }
                            a {
                                href: "{uri}",
                                target: "_blank",
                                "View entry →"
                            }
                            button {
                                class: "publish-done",
                                onclick: close_dialog,
                                "Done"
                            }
                        }
                    } else {
                        div { class: "publish-form",
                            if is_editing_existing {
                                div { class: "publish-info",
                                    p { "Updating existing entry" }
                                }
                            }

                            div { class: "publish-field publish-checkbox",
                                label {
                                    input {
                                        r#type: "checkbox",
                                        checked: use_notebook(),
                                        onchange: move |e| use_notebook.set(e.checked()),
                                    }
                                    " Publish to notebook"
                                }
                            }

                            if use_notebook() {
                                div { class: "publish-field",
                                    label { "Notebook" }
                                    input {
                                        r#type: "text",
                                        class: "publish-input",
                                        placeholder: "Notebook title...",
                                        value: "{notebook_title}",
                                        oninput: move |e| notebook_title.set(e.value()),
                                    }
                                }
                            }

                            div { class: "publish-preview",
                                p { "Title: {doc.title()}" }
                                p { "Path: {doc.path()}" }
                                if !doc.tags().is_empty() {
                                    p { "Tags: {doc.tags().join(\", \")}" }
                                }
                            }

                            if let Some(err) = error_message() {
                                div { class: "publish-error",
                                    "{err}"
                                }
                            }

                            div { class: "publish-actions",
                                button {
                                    class: "publish-cancel",
                                    onclick: close_dialog,
                                    disabled: is_publishing(),
                                    "Cancel"
                                }
                                button {
                                    class: "publish-submit",
                                    onclick: do_publish,
                                    disabled: is_publishing() || (use_notebook() && notebook_title().trim().is_empty()),
                                    if is_publishing() {
                                        "Publishing..."
                                    } else {
                                        "Publish"
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
