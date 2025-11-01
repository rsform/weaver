#[allow(unused_imports)]
use crate::fetch;
use dioxus::prelude::*;
#[allow(unused_imports)]
use dioxus::{fullstack::extract::Extension, fullstack::get_server_url, CapturedError};
use jacquard::smol_str::SmolStr;
#[allow(unused_imports)]
use std::sync::Arc;
#[allow(unused_imports)]
use weaver_renderer::theme::Theme;

#[component]
pub fn NotebookCss(ident: SmolStr, notebook: SmolStr) -> Element {
    rsx! {
        document::Stylesheet {
            href: "{get_server_url()}/css/{ident}/{notebook}"
        }
    }
}

#[get("/css/{ident}/{notebook}", fetcher: Extension<Arc<fetch::CachedFetcher>>)]
pub async fn css(ident: SmolStr, notebook: SmolStr) -> Result<String> {
    use jacquard::client::AgentSessionExt;
    use jacquard::types::ident::AtIdentifier;
    use jacquard::{from_data, CowStr};

    use weaver_api::sh_weaver::notebook::book::Book;
    use weaver_renderer::css::{generate_base_css, generate_syntax_css};
    use weaver_renderer::theme::defaultTheme;

    let ident = AtIdentifier::new_owned(ident)?;
    let theme = if let Some(notebook) = fetcher.get_notebook(ident, notebook).await? {
        let book: Book = from_data(&notebook.0.record).unwrap();
        if let Some(theme) = book.theme {
            if let Ok(theme) = fetcher.client.get_record::<Theme>(&theme.uri).await {
                theme
                    .into_output()
                    .map(|t| t.value)
                    .unwrap_or(defaultTheme())
            } else {
                defaultTheme()
            }
        } else {
            defaultTheme()
        }
    } else {
        defaultTheme()
    };
    let mut css = generate_base_css(&theme);
    css.push_str(
        &generate_syntax_css(&theme)
            .await
            .map_err(|e| CapturedError::from_display(e))
            .unwrap_or_default(),
    );
    Ok(css)
}
