//! Editor toolbar component with formatting buttons.

use weaver_editor_core::FormatAction;
use super::image_upload::{ImageUploadButton, UploadedImage};
use dioxus::prelude::*;

/// Editor toolbar with formatting buttons.
///
/// Provides buttons for common markdown formatting operations.
#[component]
pub fn EditorToolbar(
    on_format: EventHandler<FormatAction>,
    on_image: EventHandler<UploadedImage>,
) -> Element {
    rsx! {
        div {
            class: "editor-toolbar",
            role: "toolbar",
            aria_label: "Text formatting",
            aria_orientation: "vertical",
            button {
                class: "toolbar-button",
                title: "Bold (Ctrl+B)",
                aria_label: "Bold (Ctrl+B)",
                onclick: move |_| on_format.call(FormatAction::Bold),
                "B"
            }
            button {
                class: "toolbar-button",
                title: "Italic (Ctrl+I)",
                aria_label: "Italic (Ctrl+I)",
                onclick: move |_| on_format.call(FormatAction::Italic),
                "I"
            }
            button {
                class: "toolbar-button",
                title: "Strikethrough",
                aria_label: "Strikethrough",
                onclick: move |_| on_format.call(FormatAction::Strikethrough),
                "S"
            }
            button {
                class: "toolbar-button",
                title: "Code",
                aria_label: "Code",
                onclick: move |_| on_format.call(FormatAction::Code),
                "<>"
            }

            span { class: "toolbar-separator" }

            button {
                class: "toolbar-button",
                title: "Heading 1",
                aria_label: "Heading 1",
                onclick: move |_| on_format.call(FormatAction::Heading(1)),
                "H1"
            }
            button {
                class: "toolbar-button",
                title: "Heading 2",
                aria_label: "Heading 2",
                onclick: move |_| on_format.call(FormatAction::Heading(2)),
                "H2"
            }
            button {
                class: "toolbar-button",
                title: "Heading 3",
                aria_label: "Heading 3",
                onclick: move |_| on_format.call(FormatAction::Heading(3)),
                "H3"
            }

            span { class: "toolbar-separator" }

            button {
                class: "toolbar-button",
                title: "Bullet List",
                aria_label: "Bullet List",
                onclick: move |_| on_format.call(FormatAction::BulletList),
                "•"
            }
            button {
                class: "toolbar-button",
                title: "Numbered List",
                aria_label: "Numbered List",
                onclick: move |_| on_format.call(FormatAction::NumberedList),
                "1."
            }
            button {
                class: "toolbar-button",
                title: "Quote",
                aria_label: "Quote",
                onclick: move |_| on_format.call(FormatAction::Quote),
                "❝"
            }

            span { class: "toolbar-separator" }

            button {
                class: "toolbar-button",
                title: "Link",
                aria_label: "Link",
                onclick: move |_| on_format.call(FormatAction::Link),
                "🔗"
            }
            ImageUploadButton {
                on_image_selected: move |img| on_image.call(img),
            }
        }
    }
}
