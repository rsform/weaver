//! Entry publishing and loading functionality for the markdown editor.
//!
//! Handles creating/updating/loading AT Protocol notebook entries.

use dioxus::prelude::*;
use jacquard::cowstr::ToCowStr;
use jacquard::smol_str::ToSmolStr;
use jacquard::types::collection::Collection;
use jacquard::types::ident::AtIdentifier;
use jacquard::types::recordkey::RecordKey;
#[allow(unused_imports)]
use jacquard::types::string::{AtUri, Datetime, Nsid, Rkey};
use jacquard::types::tid::Ticker;
use jacquard::{IntoStatic, from_data, prelude::*, to_data};
use regex_lite::Regex;
use std::sync::LazyLock;
use weaver_api::com_atproto::repo::get_record::GetRecord;
use weaver_api::com_atproto::repo::strong_ref::StrongRef;
use weaver_api::com_atproto::repo::{create_record::CreateRecord, put_record::PutRecord};
use weaver_api::sh_weaver::embed::images::Images;
use weaver_api::sh_weaver::embed::records::{RecordEmbed, Records};
use weaver_api::sh_weaver::notebook::entry::{Entry, EntryEmbeds};
use weaver_common::{WeaverError, WeaverExt};

const ENTRY_NSID: &str = "sh.weaver.notebook.entry";

/// Regex to match draft image paths: /image/{did}/draft/{blob_rkey}/{name}
/// Captures: 1=did, 2=blob_rkey, 3=name
static DRAFT_IMAGE_PATH_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"/image/([^/]+)/draft/([^/]+)/([^)\s]+)").unwrap());

/// Rewrite draft image paths to published paths.
///
/// Converts `/image/{did}/draft/{blob_rkey}/{name}` to `/image/{did}/{entry_rkey}/{name}`
fn rewrite_draft_paths(content: &str, entry_rkey: &str) -> String {
    DRAFT_IMAGE_PATH_REGEX
        .replace_all(content, |caps: &regex_lite::Captures| {
            let did = &caps[1];
            let name = &caps[3];
            format!("/image/{}/{}/{}", did, entry_rkey, name)
        })
        .into_owned()
}

/// Rewrite draft paths for notebook entries.
///
/// Converts `/image/{did}/draft/{blob_rkey}/{name}` to `/image/{notebook}/{name}`
fn rewrite_draft_paths_for_notebook(content: &str, notebook_key: &str) -> String {
    DRAFT_IMAGE_PATH_REGEX
        .replace_all(content, |caps: &regex_lite::Captures| {
            let name = &caps[3];
            format!("/image/{}/{}", notebook_key, name)
        })
        .into_owned()
}

use crate::auth::AuthState;
use crate::fetch::Fetcher;

use super::document::EditorDocument;
use super::storage::{delete_draft, save_to_storage};

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

/// Result of fetching an entry for editing.
#[derive(Clone, PartialEq)]
pub struct LoadedEntry {
    pub entry: Entry<'static>,
    pub entry_ref: StrongRef<'static>,
}

/// Fetch an existing entry from the PDS for editing.
pub async fn load_entry_for_editing(
    fetcher: &Fetcher,
    uri: &AtUri<'_>,
) -> Result<LoadedEntry, WeaverError> {
    // Parse the AT-URI components
    let ident = uri.authority();
    let rkey = uri
        .rkey()
        .ok_or_else(|| WeaverError::InvalidNotebook("Entry URI missing rkey".into()))?;

    // Resolve DID and PDS
    let (did, pds_url) = match ident {
        AtIdentifier::Did(d) => {
            let pds = fetcher.client.pds_for_did(d).await.map_err(|e| {
                WeaverError::InvalidNotebook(format!("Failed to resolve DID: {}", e))
            })?;
            (d.clone(), pds)
        }
        AtIdentifier::Handle(h) => {
            let (did, pds) = fetcher.client.pds_for_handle(h).await.map_err(|e| {
                WeaverError::InvalidNotebook(format!("Failed to resolve handle: {}", e))
            })?;
            (did, pds)
        }
    };

    // Fetch the entry record
    let request = GetRecord::new()
        .repo(AtIdentifier::Did(did))
        .collection(Nsid::raw(<Entry as Collection>::NSID))
        .rkey(rkey.clone())
        .build();

    let response = fetcher
        .client
        .xrpc(pds_url)
        .send(&request)
        .await
        .map_err(|e| WeaverError::InvalidNotebook(format!("Failed to fetch entry: {}", e)))?;

    let record = response
        .into_output()
        .map_err(|e| WeaverError::InvalidNotebook(format!("Failed to parse response: {}", e)))?;

    // Deserialize the entry
    let entry: Entry = from_data(&record.value)
        .map_err(|e| WeaverError::InvalidNotebook(format!("Failed to deserialize entry: {}", e)))?;

    // Build StrongRef from URI and CID
    let entry_ref = StrongRef::new()
        .uri(uri.clone().into_static())
        .cid(
            record
                .cid
                .ok_or_else(|| WeaverError::InvalidNotebook("Entry response missing CID".into()))?
                .into_static(),
        )
        .build();

    Ok(LoadedEntry {
        entry: entry.into_static(),
        entry_ref,
    })
}

/// Publish an entry to the AT Protocol.
///
/// Supports three modes:
/// - With notebook_title: uses `upsert_entry` to publish to a notebook
/// - Without notebook but with entry_uri in doc: uses `put_record` to update existing
/// - Without notebook and no entry_uri: uses `create_record` for free-floating entry
///
/// Draft image paths are rewritten to published paths before publishing.
/// On successful create, sets `doc.entry_uri` so subsequent publishes update the same record.
pub async fn publish_entry(
    fetcher: &Fetcher,
    doc: &mut EditorDocument,
    notebook_title: Option<&str>,
    draft_key: &str,
) -> Result<PublishResult, WeaverError> {
    // Get images from the document
    let editor_images = doc.images();

    // Resolve AT embed URIs to StrongRefs
    let at_embed_uris = doc.at_embed_uris();
    let mut record_embeds: Vec<RecordEmbed<'static>> = Vec::new();
    for uri in at_embed_uris {
        match fetcher.confirm_record_ref(&uri).await {
            Ok(strong_ref) => {
                // Store original URI in name field for lookup when authority differs (handle vs DID)
                record_embeds.push(
                    RecordEmbed::new()
                        .name(uri.to_cowstr().into_static())
                        .record(strong_ref)
                        .build(),
                );
            }
            Err(e) => {
                tracing::warn!("Failed to resolve embed {}: {}", uri, e);
            }
        }
    }

    // Build embeds if we have images or records
    tracing::debug!(
        "[publish_entry] Building embeds: {} images, {} record embeds",
        editor_images.len(),
        record_embeds.len()
    );
    let entry_embeds = if editor_images.is_empty() && record_embeds.is_empty() {
        None
    } else {
        let images = if editor_images.is_empty() {
            None
        } else {
            Some(Images {
                images: editor_images.iter().map(|ei| ei.image.clone()).collect(),
                extra_data: None,
            })
        };

        let records = if record_embeds.is_empty() {
            None
        } else {
            Some(Records::new().records(record_embeds).build())
        };

        Some(EntryEmbeds {
            images,
            records,
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

    let client = fetcher.get_client();
    let result = if let Some(notebook) = notebook_title {
        // Publish to a notebook via upsert_entry
        // Rewrite draft image paths to notebook paths: /image/{notebook}/{name}
        let content = rewrite_draft_paths_for_notebook(&doc.content(), notebook);

        let entry = Entry::new()
            .content(content)
            .title(doc.title())
            .path(path)
            .created_at(Datetime::now())
            .updated_at(Datetime::now())
            .maybe_tags(tags)
            .maybe_embeds(entry_embeds)
            .build();

        // Check if we have a stored notebook URI (for re-publishing to same notebook)
        // This avoids duplicate notebook creation when re-publishing
        let (notebook_uri, entry_refs) = if let Some(stored_uri) = doc.notebook_uri() {
            // Try to fetch notebook directly by URI to avoid duplicate creation
            match client.get_notebook_by_uri(&stored_uri).await {
                Ok(Some((uri, refs))) => {
                    tracing::debug!("Found notebook by stored URI: {}", uri);
                    (uri, refs)
                }
                Ok(None) | Err(_) => {
                    // Stored URI invalid or notebook deleted, fall back to title lookup
                    tracing::warn!("Stored notebook URI invalid, falling back to title lookup");
                    let (did, _) = client
                        .session_info()
                        .await
                        .ok_or_else(|| WeaverError::InvalidNotebook("Not authenticated".into()))?;
                    client.upsert_notebook(notebook, &did).await?
                }
            }
        } else {
            // No stored URI, use title-based lookup/creation
            let (did, _) = client
                .session_info()
                .await
                .ok_or_else(|| WeaverError::InvalidNotebook("Not authenticated".into()))?;
            client.upsert_notebook(notebook, &did).await?
        };

        // Pass existing rkey if re-publishing (to allow title changes without creating new entry)
        let doc_entry_ref = doc.entry_ref();
        let existing_rkey = doc_entry_ref.as_ref().and_then(|r| r.uri.rkey());

        // Use upsert_entry_with_notebook since we already have notebook data
        let (entry_ref, notebook_uri_final, was_created) = client
            .upsert_entry_with_notebook(
                notebook_uri,
                entry_refs,
                &doc.title(),
                entry,
                existing_rkey.map(|r| r.0.as_str()),
            )
            .await?;
        let uri = entry_ref.uri.clone();

        // Set entry_ref so subsequent publishes update this record
        doc.set_entry_ref(Some(entry_ref));

        // Store the notebook URI for future re-publishing
        doc.set_notebook_uri(Some(notebook_uri_final.to_smolstr()));

        if was_created {
            PublishResult::Created(uri)
        } else {
            PublishResult::Updated(uri)
        }
    } else if let Some(existing_ref) = doc.entry_ref() {
        // Update existing entry (either owner or collaborator)
        let current_did = fetcher
            .current_did()
            .await
            .ok_or_else(|| WeaverError::InvalidNotebook("Not authenticated".into()))?;

        let rkey = existing_ref
            .uri
            .rkey()
            .ok_or_else(|| WeaverError::InvalidNotebook("Entry URI missing rkey".into()))?;

        // Check if we're the owner or a collaborator
        let owner_did = match existing_ref.uri.authority() {
            AtIdentifier::Did(d) => d.clone(),
            AtIdentifier::Handle(h) => fetcher.client.resolve_handle(h).await.map_err(|e| {
                WeaverError::InvalidNotebook(format!("Failed to resolve handle: {}", e))
            })?,
        };
        let is_collaborator = owner_did != current_did;

        // Rewrite draft image paths to published paths
        let content = rewrite_draft_paths(&doc.content(), rkey.0.as_str());

        let entry = Entry::new()
            .content(content)
            .title(doc.title())
            .path(path)
            .created_at(Datetime::now())
            .updated_at(Datetime::now())
            .maybe_tags(tags)
            .maybe_embeds(entry_embeds)
            .build();
        let entry_data = to_data(&entry).unwrap();

        let collection = Nsid::new(ENTRY_NSID).map_err(|e| WeaverError::AtprotoString(e))?;

        // Collaborator: create/update in THEIR repo with SAME rkey
        // Owner: update in their own repo
        let request = PutRecord::new()
            .repo(AtIdentifier::Did(current_did.clone()))
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

        if is_collaborator {
            // Collaborator: don't update doc.entry_ref() - it still points to original
            // Their version is a parallel record at at://{collab_did}/sh.weaver.notebook.entry/{same_rkey}
            tracing::info!(
                "Collaborator published version: {} (original: {})",
                output.uri,
                existing_ref.uri
            );
            PublishResult::Created(output.uri.into_static())
        } else {
            // Owner: update entry_ref with new CID
            let updated_ref = StrongRef::new()
                .uri(output.uri.clone().into_static())
                .cid(output.cid.into_static())
                .build();
            doc.set_entry_ref(Some(updated_ref));
            PublishResult::Updated(output.uri.into_static())
        }
    } else {
        // Create new free-floating entry - pre-generate rkey for path rewriting
        let did = fetcher
            .current_did()
            .await
            .ok_or_else(|| WeaverError::InvalidNotebook("Not authenticated".into()))?;

        // Pre-generate TID for the entry rkey
        let entry_tid = Ticker::new().next(None);
        let entry_rkey_str = entry_tid.as_str();

        // Rewrite draft image paths to published paths
        let content = rewrite_draft_paths(&doc.content(), entry_rkey_str);

        let entry = Entry::new()
            .content(content)
            .title(doc.title())
            .path(path)
            .created_at(Datetime::now())
            .updated_at(Datetime::now())
            .maybe_tags(tags)
            .maybe_embeds(entry_embeds)
            .build();
        let entry_data = to_data(&entry).unwrap();

        let collection = Nsid::new(ENTRY_NSID).map_err(|e| WeaverError::AtprotoString(e))?;
        let rkey = RecordKey::any(entry_rkey_str)
            .map_err(|e| WeaverError::InvalidNotebook(e.to_string()))?;

        let request = CreateRecord::new()
            .repo(AtIdentifier::Did(did))
            .collection(collection)
            .rkey(rkey)
            .record(entry_data)
            .build();

        let response = fetcher
            .send(request)
            .await
            .map_err(jacquard::client::AgentError::from)?;
        let output = response
            .into_output()
            .map_err(|e| WeaverError::InvalidNotebook(e.to_string()))?;

        let uri = output.uri.into_static();
        // Set entry_ref so subsequent publishes update this record
        let entry_ref = StrongRef::new()
            .uri(uri.clone())
            .cid(output.cid.into_static())
            .build();
        doc.set_entry_ref(Some(entry_ref));
        PublishResult::Created(uri)
    };

    // Cleanup: delete PublishedBlob records (entry's embed refs now keep blobs alive)
    // TODO: Implement when image upload is added
    // for img in &editor_images {
    //     if let Some(ref published_uri) = img.published_blob_uri {
    //         let _ = delete_published_blob(fetcher, published_uri).await;
    //     }
    // }

    // Delete the old draft key
    delete_draft(draft_key);

    // Save with the new uri-based key so continued editing is tracked by entry URI
    let new_key = result.uri().to_string();
    if let Err(e) = save_to_storage(doc, &new_key) {
        tracing::warn!("Failed to save draft after publish: {e}");
    }

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
    /// Pre-selected notebook (from URL param)
    #[props(optional)]
    pub target_notebook: Option<String>,
}

/// Publish button component with notebook selection.
#[component]
pub fn PublishButton(props: PublishButtonProps) -> Element {
    let fetcher = use_context::<Fetcher>();
    let auth_state = use_context::<Signal<AuthState>>();

    let mut show_dialog = use_signal(|| false);
    let mut notebook_title = use_signal(|| {
        props
            .target_notebook
            .clone()
            .unwrap_or_else(|| String::from("Default"))
    });
    let mut use_notebook = use_signal(|| props.target_notebook.is_some());
    let mut is_publishing = use_signal(|| false);
    let mut error_message: Signal<Option<String>> = use_signal(|| None);
    let mut success_uri: Signal<Option<AtUri<'static>>> = use_signal(|| None);

    let is_authenticated = auth_state.read().is_authenticated();
    let doc = props.document.clone();
    let draft_key = props.draft_key.clone();

    // Check if we're editing an existing entry
    let is_editing_existing = doc.entry_ref().is_some();

    // Check if we're publishing as a collaborator (editing someone else's entry)
    let is_collaborator = {
        let entry_ref = doc.entry_ref();
        let current_did = auth_state.read().did.clone();
        match (entry_ref, current_did) {
            (Some(ref r), Some(ref current)) => {
                match r.uri.authority() {
                    AtIdentifier::Did(owner_did) => owner_did != current,
                    AtIdentifier::Handle(_) => false, // Can't determine without async resolve
                }
            }
            _ => false,
        }
    };

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

            let mut doc_snapshot = doc_snapshot;
            match publish_entry(&fetcher, &mut doc_snapshot, notebook.as_deref(), &draft_key).await
            {
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
                role: "dialog",
                aria_modal: "true",
                aria_labelledby: "publish-dialog-title",
                onclick: close_dialog,

                div {
                    class: "publish-dialog",
                    onclick: move |e| e.stop_propagation(),

                    h2 { id: "publish-dialog-title", "Publish Entry" }

                    if let Some(uri) = success_uri() {
                        {
                            // Construct web URL from AT-URI
                            let did = uri.authority();
                            let web_url = if use_notebook() {
                                // Notebook entry: /{did}/{notebook}/{entry_path}
                                format!("/{}/{}/{}", did, notebook_title(), doc.path())
                            } else {
                                // Standalone entry: /{did}/e/{rkey}
                                let rkey = uri.rkey().map(|r| r.0.as_str()).unwrap_or("");
                                format!("/{}/e/{}", did, rkey)
                            };

                            rsx! {
                                div { class: "publish-success",
                                    p { "Entry published successfully!" }
                                    a {
                                        href: "{web_url}",
                                        target: "_blank",
                                        "View entry → "
                                    }
                                    button {
                                        class: "publish-done",
                                        onclick: close_dialog,
                                        "Done"
                                    }
                                }
                            }
                        }
                    } else {
                        div { class: "publish-form",
                            if is_collaborator {
                                div { class: "publish-info publish-collab-info",
                                    p { "Publishing as collaborator" }
                                    p { class: "publish-collab-detail",
                                        "This creates a version in your repository."
                                    }
                                }
                            } else if is_editing_existing {
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
                                        aria_label: "Notebook title",
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
