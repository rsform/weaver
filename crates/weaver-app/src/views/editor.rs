//! Editor view - wraps the MarkdownEditor component for the /editor route.

use crate::components::editor::MarkdownEditor;
use dioxus::prelude::*;

/// Editor page view.
///
/// Displays the markdown editor at the /editor route.
/// Optionally loads an existing entry for editing via `?entry={at-uri}`.
#[component]
pub fn Editor(entry: Option<String>) -> Element {
    rsx! {
        EditorCss {}
        div { class: "editor-page",
            MarkdownEditor { entry_uri: entry }
        }
    }
}

#[component]
pub fn EditorCss() -> Element {
    use weaver_renderer::css::{generate_base_css, generate_syntax_css};
    use weaver_renderer::theme::default_resolved_theme;

    let css_content = use_resource(move || async move {
        let resolved_theme = default_resolved_theme();
        let mut css = generate_base_css(&resolved_theme);
        css.push_str(
            &generate_syntax_css(&resolved_theme)
                .await
                .unwrap_or_default(),
        );

        Some(css)
    });

    match css_content() {
        Some(Some(css)) => rsx! { document::Style { {css} } },
        _ => rsx! {},
    }
}
