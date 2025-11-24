//! Editor toolbar component with formatting buttons.

use super::formatting::FormatAction;
use dioxus::prelude::*;

/// Editor toolbar with formatting buttons.
///
/// Provides buttons for common markdown formatting operations.
#[component]
pub fn EditorToolbar(on_format: EventHandler<FormatAction>) -> Element {
    rsx! {
        div { class: "editor-toolbar",
            button {
                class: "toolbar-button",
                title: "Bold (Ctrl+B)",
                onclick: move |_| on_format.call(FormatAction::Bold),
                "B"
            }
            button {
                class: "toolbar-button",
                title: "Italic (Ctrl+I)",
                onclick: move |_| on_format.call(FormatAction::Italic),
                "I"
            }
            button {
                class: "toolbar-button",
                title: "Strikethrough",
                onclick: move |_| on_format.call(FormatAction::Strikethrough),
                "S"
            }
            button {
                class: "toolbar-button",
                title: "Code",
                onclick: move |_| on_format.call(FormatAction::Code),
                "<>"
            }

            span { class: "toolbar-separator" }

            button {
                class: "toolbar-button",
                title: "Heading 1",
                onclick: move |_| on_format.call(FormatAction::Heading(1)),
                "H1"
            }
            button {
                class: "toolbar-button",
                title: "Heading 2",
                onclick: move |_| on_format.call(FormatAction::Heading(2)),
                "H2"
            }
            button {
                class: "toolbar-button",
                title: "Heading 3",
                onclick: move |_| on_format.call(FormatAction::Heading(3)),
                "H3"
            }

            span { class: "toolbar-separator" }

            button {
                class: "toolbar-button",
                title: "Bullet List",
                onclick: move |_| on_format.call(FormatAction::BulletList),
                "•"
            }
            button {
                class: "toolbar-button",
                title: "Numbered List",
                onclick: move |_| on_format.call(FormatAction::NumberedList),
                "1."
            }
            button {
                class: "toolbar-button",
                title: "Quote",
                onclick: move |_| on_format.call(FormatAction::Quote),
                "❝"
            }

            span { class: "toolbar-separator" }

            button {
                class: "toolbar-button",
                title: "Link",
                onclick: move |_| on_format.call(FormatAction::Link),
                "🔗"
            }
            button {
                class: "toolbar-button",
                title: "Image",
                onclick: move |_| on_format.call(FormatAction::Image),
                "🖼"
            }
        }
    }
}
