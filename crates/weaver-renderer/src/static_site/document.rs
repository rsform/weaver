use crate::css::{generate_base_css, generate_syntax_css};
use crate::static_site::context::{KaTeXSource, StaticSiteContext};
use crate::theme::Theme;
use miette::IntoDiagnostic;
use weaver_common::jacquard::client::AgentSession;

#[derive(Debug, Clone, Copy)]
pub enum CssMode {
    Linked,
    Inline,
}

pub async fn write_document_head<A: AgentSession>(
    context: &StaticSiteContext<A>,
    writer: &mut (impl tokio::io::AsyncWrite + Unpin),
    css_mode: CssMode,
) -> miette::Result<()> {
    use tokio::io::AsyncWriteExt;

    // Get title from frontmatter or current path
    let title = if let Some(path) = context.dir_contents.as_ref()
        .and_then(|contents| contents.get(context.position))
    {
        context.titles.get(path)
            .map(|t| t.value().to_string())
            .unwrap_or_else(|| {
                path.file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("Untitled")
                    .to_string()
            })
    } else {
        "Untitled".to_string()
    };

    writer.write_all(b"<!DOCTYPE html>\n").await.into_diagnostic()?;
    writer.write_all(b"<html lang=\"en\">\n").await.into_diagnostic()?;
    writer.write_all(b"<head>\n").await.into_diagnostic()?;
    writer.write_all(b"  <meta charset=\"utf-8\">\n").await.into_diagnostic()?;
    writer.write_all(b"  <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n").await.into_diagnostic()?;

    // Title
    writer.write_all(b"  <title>").await.into_diagnostic()?;
    writer.write_all(title.as_bytes()).await.into_diagnostic()?;
    writer.write_all(b"</title>\n").await.into_diagnostic()?;

    // CSS
    match css_mode {
        CssMode::Linked => {
            writer.write_all(b"  <link rel=\"stylesheet\" href=\"/css/base.css\">\n").await.into_diagnostic()?;
            writer.write_all(b"  <link rel=\"stylesheet\" href=\"/css/syntax.css\">\n").await.into_diagnostic()?;
        }
        CssMode::Inline => {
            let default_theme = Theme::default();
            let theme = context.theme.as_deref().unwrap_or(&default_theme);

            writer.write_all(b"  <style>\n").await.into_diagnostic()?;
            writer.write_all(generate_base_css(theme).as_bytes()).await.into_diagnostic()?;
            writer.write_all(b"  </style>\n").await.into_diagnostic()?;

            writer.write_all(b"  <style>\n").await.into_diagnostic()?;
            let syntax_css = generate_syntax_css(&theme.syntect_theme_name, &context.syntax_set)?;
            writer.write_all(syntax_css.as_bytes()).await.into_diagnostic()?;
            writer.write_all(b"  </style>\n").await.into_diagnostic()?;
        }
    }

    // KaTeX if enabled
    if let Some(ref katex) = context.katex_source {
        match katex {
            KaTeXSource::Cdn => {
                writer.write_all(b"  <link rel=\"stylesheet\" href=\"https://cdn.jsdelivr.net/npm/katex@0.16.9/dist/katex.min.css\">\n").await.into_diagnostic()?;
                writer.write_all(b"  <script defer src=\"https://cdn.jsdelivr.net/npm/katex@0.16.9/dist/katex.min.js\"></script>\n").await.into_diagnostic()?;
                writer.write_all(b"  <script defer src=\"https://cdn.jsdelivr.net/npm/katex@0.16.9/dist/contrib/auto-render.min.js\" onload=\"renderMathInElement(document.body);\"></script>\n").await.into_diagnostic()?;
            }
            KaTeXSource::Local(path) => {
                let path_str = path.to_string_lossy();
                writer.write_all(format!("  <link rel=\"stylesheet\" href=\"{}/katex.min.css\">\n", path_str).as_bytes()).await.into_diagnostic()?;
                writer.write_all(format!("  <script defer src=\"{}/katex.min.js\"></script>\n", path_str).as_bytes()).await.into_diagnostic()?;
                writer.write_all(format!("  <script defer src=\"{}/contrib/auto-render.min.js\" onload=\"renderMathInElement(document.body);\"></script>\n", path_str).as_bytes()).await.into_diagnostic()?;
            }
        }
    }

    writer.write_all(b"</head>\n").await.into_diagnostic()?;
    writer.write_all(b"<body>\n").await.into_diagnostic()?;

    Ok(())
}

pub async fn write_document_footer(
    writer: &mut (impl tokio::io::AsyncWrite + Unpin),
) -> miette::Result<()> {
    use tokio::io::AsyncWriteExt;

    writer.write_all(b"</body>\n").await.into_diagnostic()?;
    writer.write_all(b"</html>\n").await.into_diagnostic()?;

    Ok(())
}
