//! Image upload component for the markdown editor.
//!
//! Provides file picker and upload functionality for adding images to entries.
//! Shows a preview dialog with alt text input before confirming the upload.

use base64::{Engine, engine::general_purpose::STANDARD};
use dioxus::prelude::*;
use jacquard::bytes::Bytes;
use mime_sniffer::MimeTypeSniffer;

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
