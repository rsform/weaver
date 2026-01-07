//! Image upload component for the markdown editor.
//!
//! Provides file picker and upload functionality for adding images to entries.
//! Shows a preview dialog with alt text input before confirming the upload.

use base64::{Engine, engine::general_purpose::STANDARD};
use dioxus::prelude::*;
use jacquard::IntoStatic;
use jacquard::cowstr::ToCowStr;
use jacquard::types::ident::AtIdentifier;
use jacquard::{bytes::Bytes, types::tid::Tid};
use mime_sniffer::MimeTypeSniffer;

use super::document::SignalEditorDocument;
use crate::auth::AuthState;
use crate::fetch::Fetcher;
use weaver_api::sh_weaver::embed::images::Image;
use weaver_editor_core::{EditorDocument, EditorImageResolver};

use crate::components::{
    button::{Button, ButtonVariant},
    dialog::{DialogContent, DialogRoot, DialogTitle},
};

/// Result of an image upload operation.
#[derive(Clone, Debug)]
pub struct UploadedImage {
    /// The filename (used as the markdown reference name)
    pub name: String,
    /// Alt text for accessibility
    pub alt: String,
    /// MIME type of the image (sniffed from bytes)
    pub mime_type: String,
    /// Raw image bytes
    pub data: Bytes,
}

/// Pending image data before user confirms with alt text.
#[derive(Clone, Default)]
struct PendingImage {
    name: String,
    mime_type: String,
    data: Bytes,
    data_url: String,
}

/// Props for the ImageUploadButton component.
#[derive(Props, Clone, PartialEq)]
pub struct ImageUploadButtonProps {
    /// Callback when an image is selected and confirmed with alt text
    pub on_image_selected: EventHandler<UploadedImage>,
    /// Whether the button is disabled
    #[props(default = false)]
    pub disabled: bool,
}

/// A button that opens a file picker for image selection.
///
/// When a file is selected, shows a preview dialog with alt text input.
/// Only triggers the callback after user confirms.
#[component]
pub fn ImageUploadButton(props: ImageUploadButtonProps) -> Element {
    let mut show_dialog = use_signal(|| false);
    let mut pending_image = use_signal(PendingImage::default);
    let mut alt_text = use_signal(String::new);

    let on_file_change = move |evt: Event<FormData>| {
        spawn(async move {
            let files = evt.files();
            if let Some(file) = files.first() {
                let name = file.name();

                if let Ok(data) = file.read_bytes().await {
                    let bytes = Bytes::from(data);
                    let mime_type = bytes
                        .sniff_mime_type()
                        .unwrap_or("application/octet-stream")
                        .to_string();

                    let data_url = format!("data:{};base64,{}", mime_type, STANDARD.encode(&bytes));

                    pending_image.set(PendingImage {
                        name: name.clone(),
                        mime_type,
                        data: bytes,
                        data_url,
                    });
                    alt_text.set(String::new());
                    show_dialog.set(true);
                }
            }
        });
    };

    let on_image_selected = props.on_image_selected.clone();
    let confirm_upload = move |_| {
        let pending = pending_image();
        let uploaded = UploadedImage {
            name: pending.name,
            alt: alt_text(),
            mime_type: pending.mime_type,
            data: pending.data,
        };
        on_image_selected.call(uploaded);
        show_dialog.set(false);
        pending_image.set(PendingImage::default());
        alt_text.set(String::new());
    };

    let cancel_upload = move |_| {
        show_dialog.set(false);
        pending_image.set(PendingImage::default());
        alt_text.set(String::new());
    };

    rsx! {
        label {
            class: "toolbar-button",
            title: "Image",
            aria_label: "Add image",
            input {
                r#type: "file",
                accept: "image/*",
                style: "display: none;",
                disabled: props.disabled,
                onchange: on_file_change,
            }
            "🖼"
        }

        DialogRoot {
            open: show_dialog(),
            on_open_change: move |v| show_dialog.set(v),

            DialogContent {
                button {
                    class: "dialog-close",
                    r#type: "button",
                    aria_label: "Close",
                    tabindex: if show_dialog() { "0" } else { "-1" },
                    onclick: cancel_upload,
                    "×"
                }

                DialogTitle { "Add Image" }

                div { class: "image-preview-container",
                    img {
                        class: "image-preview",
                        src: "{pending_image().data_url}",
                        alt: "Preview",
                    }
                }

                div { class: "image-alt-input-container",
                    label {
                        r#for: "image-alt-text",
                        "Alt text"
                    }
                    textarea {
                        id: "image-alt-text",
                        class: "image-alt-input",
                        placeholder: "Describe this image for people who can't see it",
                        value: "{alt_text}",
                        oninput: move |e| alt_text.set(e.value()),
                        rows: "3",
                    }
                }

                div { class: "dialog-actions",
                    Button {
                        r#type: "button",
                        onclick: cancel_upload,
                        variant: ButtonVariant::Secondary,
                        "Cancel"
                    }
                    Button {
                        r#type: "button",
                        onclick: confirm_upload,
                        "Add Image"
                    }
                }
            }
        }
    }
}

/// Handle an uploaded image: add to resolver, insert markdown, and upload to PDS.
///
/// This is the main handler for when an image is confirmed via the upload dialog.
/// It:
/// 1. Creates a data URL for immediate preview
/// 2. Adds to the image resolver for display
/// 3. Inserts markdown image syntax at cursor
/// 4. If authenticated, uploads to PDS in background
#[allow(clippy::too_many_arguments)]
pub fn handle_image_upload(
    uploaded: UploadedImage,
    doc: &mut SignalEditorDocument,
    image_resolver: &mut Signal<EditorImageResolver>,
    auth_state: &Signal<AuthState>,
    fetcher: &Fetcher,
) {
    // Build data URL for immediate preview.
    let data_url = format!(
        "data:{};base64,{}",
        uploaded.mime_type,
        STANDARD.encode(&uploaded.data)
    );

    // Add to resolver for immediate display.
    let name = uploaded.name.clone();
    image_resolver.with_mut(|resolver| {
        resolver.add_pending(name.clone(), data_url);
    });

    // Insert markdown image syntax at cursor.
    let alt_text = if uploaded.alt.is_empty() {
        name.clone()
    } else {
        uploaded.alt.clone()
    };

    // Check if authenticated and get DID for draft path.
    let auth = auth_state.read();
    let did_for_path = auth.did.clone();
    let is_authenticated = auth.is_authenticated();
    drop(auth);

    // Pre-generate TID for the blob rkey (used in draft path and upload).
    let blob_tid = jacquard::types::tid::Ticker::new().next(None);

    // Build markdown with proper draft path if authenticated.
    let markdown = if let Some(ref did) = did_for_path {
        format!(
            "![{}](/image/{}/draft/{}/{})",
            alt_text,
            did,
            blob_tid.as_str(),
            name
        )
    } else {
        // Fallback for unauthenticated - simple path (won't be publishable anyway).
        format!("![{}](/image/{})", alt_text, name)
    };

    let pos = doc.cursor_offset();
    doc.insert(pos, &markdown);

    // Upload to PDS in background if authenticated.
    if is_authenticated {
        let fetcher = fetcher.clone();
        let name_for_upload = name.clone();
        let alt_for_upload = alt_text.clone();
        let data = uploaded.data.clone();
        let mut doc_for_spawn = doc.clone();
        let mut resolver_for_spawn = *image_resolver;

        spawn(async move {
            upload_image_to_pds(
                &fetcher,
                &mut doc_for_spawn,
                &mut resolver_for_spawn,
                data,
                name_for_upload,
                alt_for_upload,
                blob_tid,
            )
            .await;
        });
    } else {
        tracing::debug!(name = %name, "Image added with data URL (not authenticated)");
    }
}

/// Upload image to PDS and update resolver.
async fn upload_image_to_pds(
    fetcher: &Fetcher,
    doc: &mut SignalEditorDocument,
    image_resolver: &mut Signal<EditorImageResolver>,
    data: Bytes,
    name: String,
    alt: String,
    blob_tid: Tid,
) {
    let client = fetcher.get_client();
    use weaver_common::WeaverExt;

    // Clone data for cache pre-warming.
    #[cfg(feature = "fullstack-server")]
    let data_for_cache = data.clone();

    // Use pre-generated TID as rkey for the blob record.
    let rkey = jacquard::types::recordkey::RecordKey::any(blob_tid.as_str())
        .expect("TID is valid record key");

    // Upload blob and create temporary PublishedBlob record.
    match client.publish_blob(data, &name, Some(rkey)).await {
        Ok((strong_ref, published_blob)) => {
            // Get DID from fetcher.
            let did = match fetcher.current_did().await {
                Some(d) => d,
                None => {
                    tracing::warn!("No DID available");
                    return;
                }
            };

            // Extract rkey from the AT-URI.
            let blob_rkey = match strong_ref.uri.rkey() {
                Some(rkey) => rkey.0.clone().into_static(),
                None => {
                    tracing::warn!("No rkey in PublishedBlob URI");
                    return;
                }
            };

            let cid = published_blob.upload.blob().cid().clone().into_static();

            let name_for_resolver = name.clone();
            let image = Image::new()
                .alt(alt.to_cowstr())
                .image(published_blob.upload)
                .name(name.to_cowstr())
                .build();
            doc.add_image(&image, Some(&strong_ref.uri));

            // Promote from pending to uploaded in resolver.
            let ident = AtIdentifier::Did(did);
            image_resolver.with_mut(|resolver| {
                resolver.promote_to_uploaded(&name_for_resolver, blob_rkey, ident);
            });

            tracing::info!(name = %name_for_resolver, "Image uploaded to PDS");

            // Pre-warm server cache with blob bytes.
            #[cfg(feature = "fullstack-server")]
            {
                use jacquard::smol_str::ToSmolStr;
                if let Err(e) = crate::data::cache_blob_bytes(
                    cid.to_smolstr(),
                    Some(name_for_resolver.into()),
                    None,
                    data_for_cache.into(),
                )
                .await
                {
                    tracing::warn!(error = %e, "Failed to pre-warm blob cache");
                }
            }

            // Suppress unused variable warning when fullstack-server is disabled.
            #[cfg(not(feature = "fullstack-server"))]
            let _ = cid;
        }
        Err(e) => {
            tracing::error!(error = %e, "Failed to upload image");
            // Image stays as data URL - will work for preview but not publish.
        }
    }
}
