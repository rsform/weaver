//! Editor view - wraps the MarkdownEditor component for the /editor route.

use dioxus::prelude::*;
use crate::components::editor::MarkdownEditor;

/// Editor page view.
///
/// Displays the markdown editor at the /editor route for testing during development.
/// Eventually this will be integrated into the notebook editing workflow.
#[component]
pub fn Editor() -> Element {
    rsx! {
        div { class: "editor-page",
            MarkdownEditor { initial_content: None }
        }
    }
}
