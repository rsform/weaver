#[allow(unused_imports)]
use crate::fetch;
#[allow(unused_imports)]
use dioxus::{prelude::*, CapturedError};

#[cfg(feature = "fullstack-server")]
use dioxus::fullstack::{
    get_server_url,
    headers::ContentType,
    http::header::CONTENT_TYPE,
    response::{self, Response},
};
use jacquard::smol_str::SmolStr;
#[allow(unused_imports)]
use std::sync::Arc;
#[allow(unused_imports)]
use weaver_renderer::theme::{ResolvedTheme, Theme};

#[cfg(feature = "server")]
use axum::{extract::Extension, response::IntoResponse};

#[cfg(feature = "fullstack-server")]
#[component]
pub fn NotebookCss(ident: SmolStr, notebook: SmolStr) -> Element {
    rsx! {
        document::Stylesheet {
            href: "{get_server_url()}/{ident}/{notebook}/css"
        }
    }
}

#[cfg(not(feature = "fullstack-server"))]
#[component]
pub fn NotebookCss(ident: SmolStr, notebook: SmolStr) -> Element {
    use jacquard::client::AgentSessionExt;
    use jacquard::types::ident::AtIdentifier;
    use jacquard::{from_data, CowStr};
    use weaver_api::sh_weaver::notebook::book::Book;
    use weaver_renderer::css::{generate_base_css, generate_syntax_css};
    use weaver_renderer::theme::{default_resolved_theme, resolve_theme};

    let fetcher = use_context::<fetch::CachedFetcher>();

    let css_content = use_resource(move || {
        let ident = ident.clone();
        let notebook = notebook.clone();
        let fetcher = fetcher.clone();

        async move {
            let ident = AtIdentifier::new_owned(ident).ok()?;
            let resolved_theme =
                if let Some(notebook) = fetcher.get_notebook(ident, notebook).await.ok()? {
                    let book: Book = from_data(&notebook.0.record).ok()?;
                    if let Some(theme_ref) = book.theme {
                        if let Ok(theme_response) =
                            fetcher.client.get_record::<Theme>(&theme_ref.uri).await
                        {
                            if let Ok(theme_output) = theme_response.into_output() {
                                let theme: Theme = theme_output.into();
                                resolve_theme(fetcher.client.as_ref(), &theme)
                                    .await
                                    .unwrap_or_else(|_| default_resolved_theme())
                            } else {
                                default_resolved_theme()
                            }
                        } else {
                            default_resolved_theme()
                        }
                    } else {
                        default_resolved_theme()
                    }
                } else {
                    default_resolved_theme()
                };

            let mut css = generate_base_css(&resolved_theme);
            css.push_str(
                &generate_syntax_css(&resolved_theme)
                    .await
                    .unwrap_or_default(),
            );

            Some(css)
        }
    });

    match css_content() {
        Some(Some(css)) => rsx! { document::Style { {css} } },
        _ => rsx! {},
    }
}

#[cfg(feature = "fullstack-server")]
#[get("/{ident}/{notebook}/css", fetcher: Extension<Arc<fetch::CachedFetcher>>)]
pub async fn css(ident: SmolStr, notebook: SmolStr) -> Result<Response> {
    use jacquard::client::AgentSessionExt;
    use jacquard::types::ident::AtIdentifier;
    use jacquard::{from_data, CowStr};

    use weaver_api::sh_weaver::notebook::book::Book;
    use weaver_renderer::css::{generate_base_css, generate_syntax_css};
    use weaver_renderer::theme::{default_resolved_theme, resolve_theme};

    let ident = AtIdentifier::new_owned(ident)?;
    let resolved_theme = if let Some(notebook) = fetcher.get_notebook(ident, notebook).await? {
        let book: Book = from_data(&notebook.0.record).unwrap();
        if let Some(theme_ref) = book.theme {
            if let Ok(theme_response) = fetcher.client.get_record::<Theme>(&theme_ref.uri).await {
                if let Ok(theme_output) = theme_response.into_output() {
                    let theme: Theme = theme_output.into();
                    // Try to resolve the theme (fetch colour schemes from PDS)
                    resolve_theme(fetcher.client.as_ref(), &theme)
                        .await
                        .unwrap_or_else(|_| default_resolved_theme())
                } else {
                    default_resolved_theme()
                }
            } else {
                default_resolved_theme()
            }
        } else {
            default_resolved_theme()
        }
    } else {
        default_resolved_theme()
    };

    let mut css = generate_base_css(&resolved_theme);
    css.push_str(
        &generate_syntax_css(&resolved_theme)
            .await
            .map_err(|e| CapturedError::from_display(e))
            .unwrap_or_default(),
    );

    Ok(([(CONTENT_TYPE, "text/css")], css).into_response())
}
