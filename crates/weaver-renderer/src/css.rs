use crate::theme::Theme;
use miette::IntoDiagnostic;
use std::io::Cursor;
use syntect::highlighting::ThemeSet;
use syntect::html::{ClassStyle, css_for_theme_with_class_style};
use weaver_api::com_atproto::sync::get_blob::GetBlob;
use weaver_api::sh_weaver::notebook::theme::ThemeCodeTheme;
use weaver_common::jacquard::client::BasicClient;
use weaver_common::jacquard::prelude::*;
use weaver_common::jacquard::xrpc::XrpcExt;

// Embed rose-pine themes at compile time
const ROSE_PINE_THEME: &str = include_str!("../themes/rose-pine.tmTheme");
const ROSE_PINE_DAWN_THEME: &str = include_str!("../themes/rose-pine-dawn.tmTheme");

pub fn generate_base_css(theme: &Theme) -> String {
    format!(
        r#"/* CSS Reset */
*, *::before, *::after {{
    box-sizing: border-box;
    margin: 0;
    padding: 0;
}}

/* CSS Variables */
:root {{
    --color-background: {};
    --color-foreground: {};
    --color-link: {};
    --color-link-hover: {};
    --color-primary: {};
    --color-secondary: {};

    --font-body: {};
    --font-heading: {};
    --font-mono: {};

    --spacing-base: {};
    --spacing-line-height: {};
    --spacing-scale: {};
}}

/* Base Styles */
html {{
    font-size: var(--spacing-base);
    line-height: var(--spacing-line-height);
}}

body {{
    font-family: var(--font-body);
    color: var(--color-foreground);
    background-color: var(--color-background);
    max-width: 90ch;
    margin: 0 auto;
    padding: 2rem 1rem;
}}

/* Typography */
h1, h2, h3, h4, h5, h6 {{
    font-family: var(--font-heading);
    margin-top: calc(1rem * var(--spacing-scale));
    margin-bottom: 0.5rem;
    line-height: 1.2;
}}

h1 {{
  font-size: 2rem;
  color: var(--color-primary);
}}
h2 {{
  font-size: 1.5rem;
  color: var(--color-secondary);
}}
h3 {{
  font-size: 1.25rem;
  color: var(--color-primary);
}}
h4 {{
  font-size: 1.2rem;
  color: var(--color-secondary);
}}
h5 {{
  font-size: 1.125rem;
  color: var(--color-primary);
}}
h6 {{ font-size: 1rem; }}

p {{
    margin-bottom: 1rem;
}}

a {{
    color: var(--color-link);
    text-decoration: none;
}}

a:hover {{
    color: var(--color-link-hover);
    text-decoration: underline;
}}

/* Lists */
ul, ol {{
    margin-left: 2rem;
    margin-bottom: 1rem;
}}

li {{
    margin-bottom: 0.25rem;
}}

/* Code */
code {{
    font-family: var(--font-mono);
    background-color: rgba(0, 0, 0, 0.05);
    padding: 0.125rem 0.25rem;
    border-radius: 3px;
    font-size: 0.9em;
}}

pre {{
    overflow-x: auto;
    margin-bottom: 1rem;
}}

pre code {{
    display: block;
    padding: 1rem;
    background-color: rgba(0, 0, 0, 0.03);
    border-radius: 5px;
}}

/* Math */
.math {{
    font-family: var(--font-mono);
}}

.math-display {{
    display: block;
    margin: 1rem 0;
    text-align: center;
}}

/* Blockquotes */
blockquote {{
    border-left: 4px solid var(--color-link);
    padding-left: 1rem;
    padding-right: 1rem;
    margin: 1rem 0;
    font-style: italic;
}}

/* Tables */
table {{
    border-collapse: collapse;
    width: 100%;
    margin-bottom: 1rem;
}}

th, td {{
    border: 1px solid rgba(0, 0, 0, 0.1);
    padding: 0.5rem;
    text-align: left;
}}

th {{
    background-color: rgba(0, 0, 0, 0.05);
    font-weight: 600;
}}

/* Footnotes */
.footnote-reference {{
    font-size: 0.8em;
}}

.footnote-definition {{
    margin-top: 2rem;
    padding-top: 0.5rem;
    border-top: 1px solid rgba(0, 0, 0, 0.1);
    font-size: 0.9em;
}}

.footnote-definition-label {{
    font-weight: 600;
    margin-right: 0.5rem;
}}

/* Images */
img {{
    max-width: 100%;
    height: auto;
    display: block;
    margin: 1rem 0;
}}

/* Horizontal Rule */
hr {{
    border: none;
    border-top: 1px solid rgba(0, 0, 0, 0.1);
    margin: 2rem 0;
}}
"#,
        theme.colours.background,
        theme.colours.foreground,
        theme.colours.link,
        theme.colours.link_hover,
        theme.colours.primary,
        theme.colours.secondary,
        theme.fonts.body,
        theme.fonts.heading,
        theme.fonts.monospace,
        theme.spacing.base_size,
        theme.spacing.line_height,
        theme.spacing.scale,
    )
}

pub async fn generate_syntax_css(theme: &Theme<'_>) -> miette::Result<String> {
    let syntect_theme = match &theme.code_theme {
        ThemeCodeTheme::CodeThemeName(name) => {
            match name.as_str() {
                "rose-pine" => {
                    let mut cursor = Cursor::new(ROSE_PINE_THEME.as_bytes());
                    ThemeSet::load_from_reader(&mut cursor)
                        .into_diagnostic()
                        .map_err(|e| {
                            miette::miette!("Failed to load embedded rose-pine theme: {}", e)
                        })?
                }
                "rose-pine-dawn" => {
                    let mut cursor = Cursor::new(ROSE_PINE_DAWN_THEME.as_bytes());
                    ThemeSet::load_from_reader(&mut cursor)
                        .into_diagnostic()
                        .map_err(|e| {
                            miette::miette!("Failed to load embedded rose-pine-dawn theme: {}", e)
                        })?
                }
                _ => {
                    // Fall back to syntect's built-in themes
                    let theme_set = ThemeSet::load_defaults();
                    theme_set
                        .themes
                        .get(name.as_str())
                        .ok_or_else(|| miette::miette!("Theme '{}' not found in defaults", name))?
                        .clone()
                }
            }
        }
        ThemeCodeTheme::CodeThemeFile(file) => {
            let client = BasicClient::unauthenticated();
            let pds = client.pds_for_did(&file.did).await?;
            let blob = client
                .xrpc(pds)
                .send(
                    &GetBlob::new()
                        .did(file.did.clone())
                        .cid(file.content.blob().cid().clone())
                        .build(),
                )
                .await?
                .buffer()
                .clone();
            let mut cursor = Cursor::new(blob);
            ThemeSet::load_from_reader(&mut cursor)
                .into_diagnostic()
                .map_err(|e| miette::miette!("Failed to download theme: {}", e))?
        }
        _ => {
            let mut cursor = Cursor::new(ROSE_PINE_THEME.as_bytes());
            ThemeSet::load_from_reader(&mut cursor)
                .into_diagnostic()
                .map_err(|e| miette::miette!("Failed to load embedded rose-pine theme: {}", e))?
        }
    };

    let css = css_for_theme_with_class_style(
        &syntect_theme,
        ClassStyle::SpacedPrefixed {
            prefix: crate::code_pretty::CSS_PREFIX,
        },
    )
    .into_diagnostic()?;

    Ok(css)
}
