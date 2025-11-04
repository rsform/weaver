use crate::theme::{ResolvedTheme, ThemeDarkCodeTheme, ThemeLightCodeTheme};
use miette::IntoDiagnostic;
use std::io::Cursor;
use syntect::highlighting::ThemeSet;
use syntect::html::{ClassStyle, css_for_theme_with_class_style};
use weaver_api::com_atproto::sync::get_blob::GetBlob;
use weaver_common::jacquard::client::BasicClient;
use weaver_common::jacquard::prelude::*;
use weaver_common::jacquard::xrpc::XrpcExt;

// Embed rose-pine themes at compile time
const ROSE_PINE_THEME: &str = include_str!("../themes/rose-pine.tmTheme");
const ROSE_PINE_DAWN_THEME: &str = include_str!("../themes/rose-pine-dawn.tmTheme");

pub fn generate_base_css(theme: &ResolvedTheme) -> String {
    let dark = &theme.dark_scheme;
    let light = &theme.light_scheme;
    let fonts = &theme.fonts;
    let spacing = &theme.spacing;

    format!(
        r#"/* CSS Reset */
*, *::before, *::after {{
    box-sizing: border-box;
    margin: 0;
    padding: 0;
}}

/* CSS Variables - Light Mode (default) */
:root {{
    --color-base: {};
    --color-surface: {};
    --color-overlay: {};
    --color-text: {};
    --color-muted: {};
    --color-subtle: {};
    --color-emphasis: {};
    --color-primary: {};
    --color-secondary: {};
    --color-tertiary: {};
    --color-error: {};
    --color-warning: {};
    --color-success: {};
    --color-border: {};
    --color-link: {};
    --color-highlight: {};

    --font-body: {};
    --font-heading: {};
    --font-mono: {};

    --spacing-base: {};
    --spacing-line-height: {};
    --spacing-scale: {};
}}

/* CSS Variables - Dark Mode */
@media (prefers-color-scheme: dark) {{
    :root {{
        --color-base: {};
        --color-surface: {};
        --color-overlay: {};
        --color-text: {};
        --color-muted: {};
        --color-subtle: {};
        --color-emphasis: {};
        --color-primary: {};
        --color-secondary: {};
        --color-tertiary: {};
        --color-error: {};
        --color-warning: {};
        --color-success: {};
        --color-border: {};
        --color-link: {};
        --color-highlight: {};
    }}
}}

/* Base Styles */
html {{
    font-size: var(--spacing-base);
    line-height: var(--spacing-line-height);
}}

body {{
    font-family: var(--font-body);
    color: var(--color-text);
    background-color: var(--color-base);
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
  color: var(--color-secondary);
}}
h2 {{
  font-size: 1.5rem;
  color: var(--color-primary);
}}
h3 {{
  font-size: 1.25rem;
  color: var(--color-secondary);
}}
h4 {{
  font-size: 1.2rem;
  color: var(--color-tertiary);
}}
h5 {{
  font-size: 1.125rem;
  color: var(--color-secondary);
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
    color: var(--color-emphasis);
    text-decoration: underline;
}}

/* Selection */
::selection {{
    background: var(--color-highlight);
    color: var(--color-text);
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
    background: var(--color-surface);
    padding: 0.125rem 0.25rem;
    border-radius: 4px;
    font-size: 0.9em;
}}

pre {{
    overflow-x: auto;
    margin-bottom: 1rem;
    border-radius: 5px;
    border: 1px solid var(--color-border);
    box-sizing: border-box;
}}

/* Code blocks inside pre are handled by syntax theme */
pre code {{
    display: block;
    width: fit-content;
    min-width: 100%;
    padding: 1rem;
    background: var(--color-surface);
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
    border-left: 2px solid var(--color-secondary);
    background: var(--color-surface);
    padding-left: 1rem;
    padding-right: 1rem;
    padding-top: 0.5rem;
    padding-bottom: 0.04rem;
    margin: 1rem 0;
    font-size: 0.95em;
    border-bottom-right-radius: 5px;
    border-top-right-radius: 5px;
}}
}}

/* Tables */
table {{
    border-collapse: collapse;
    width: 100%;
    margin-bottom: 1rem;
}}

th, td {{
    border: 1px solid var(--color-border);
    padding: 0.5rem;
    text-align: left;
}}

th {{
    background: var(--color-surface);
    font-weight: 600;
}}

tr:hover {{
    background: var(--color-surface);
}}

/* Footnotes */
.footnote-reference {{
    font-size: 0.8em;
    color: var(--color-subtle);
}}

.footnote-definition {{
    margin-top: 2rem;
    padding-top: 0.5rem;
    border-top: 1px solid var(--color-border);
    font-size: 0.9em;
}}

.footnote-definition-label {{
    font-weight: 600;
    margin-right: 0.5rem;
    color: var(--color-primary);
}}

/* Images */
img {{
    max-width: 100%;
    height: auto;
    display: block;
    margin: 1rem 0;
    border-radius: 4px;
}}

/* Horizontal Rule */
hr {{
    border: none;
    border-top: 2px solid var(--color-border);
    margin: 2rem 0;
}}
"#,
        // Light mode colours
        light.base,
        light.surface,
        light.overlay,
        light.text,
        light.muted,
        light.subtle,
        light.emphasis,
        light.primary,
        light.secondary,
        light.tertiary,
        light.error,
        light.warning,
        light.success,
        light.border,
        light.link,
        light.highlight,
        // Fonts and spacing
        fonts.body,
        fonts.heading,
        fonts.monospace,
        spacing.base_size,
        spacing.line_height,
        spacing.scale,
        // Dark mode colours
        dark.base,
        dark.surface,
        dark.overlay,
        dark.text,
        dark.muted,
        dark.subtle,
        dark.emphasis,
        dark.primary,
        dark.secondary,
        dark.tertiary,
        dark.error,
        dark.warning,
        dark.success,
        dark.border,
        dark.link,
        dark.highlight,
    )
}

async fn load_syntect_dark_theme(
    code_theme: &ThemeDarkCodeTheme<'_>,
) -> miette::Result<syntect::highlighting::Theme> {
    match code_theme {
        ThemeDarkCodeTheme::CodeThemeName(name) => {
            match name.as_str() {
                "rose-pine" => {
                    let mut cursor = Cursor::new(ROSE_PINE_THEME.as_bytes());
                    ThemeSet::load_from_reader(&mut cursor)
                        .into_diagnostic()
                        .map_err(|e| {
                            miette::miette!("Failed to load embedded rose-pine theme: {}", e)
                        })
                }
                "rose-pine-dawn" => {
                    let mut cursor = Cursor::new(ROSE_PINE_DAWN_THEME.as_bytes());
                    ThemeSet::load_from_reader(&mut cursor)
                        .into_diagnostic()
                        .map_err(|e| {
                            miette::miette!("Failed to load embedded rose-pine-dawn theme: {}", e)
                        })
                }
                _ => {
                    // Fall back to syntect's built-in themes
                    let theme_set = ThemeSet::load_defaults();
                    theme_set
                        .themes
                        .get(name.as_str())
                        .ok_or_else(|| miette::miette!("Theme '{}' not found in defaults", name))
                        .cloned()
                }
            }
        }
        ThemeDarkCodeTheme::CodeThemeFile(file) => {
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
                .map_err(|e| miette::miette!("Failed to download theme: {}", e))
        }
        _ => {
            let mut cursor = Cursor::new(ROSE_PINE_THEME.as_bytes());
            ThemeSet::load_from_reader(&mut cursor)
                .into_diagnostic()
                .map_err(|e| miette::miette!("Failed to load embedded rose-pine theme: {}", e))
        }
    }
}

async fn load_syntect_light_theme(
    code_theme: &ThemeLightCodeTheme<'_>,
) -> miette::Result<syntect::highlighting::Theme> {
    match code_theme {
        ThemeLightCodeTheme::CodeThemeName(name) => {
            match name.as_str() {
                "rose-pine" => {
                    let mut cursor = Cursor::new(ROSE_PINE_THEME.as_bytes());
                    ThemeSet::load_from_reader(&mut cursor)
                        .into_diagnostic()
                        .map_err(|e| {
                            miette::miette!("Failed to load embedded rose-pine theme: {}", e)
                        })
                }
                "rose-pine-dawn" => {
                    let mut cursor = Cursor::new(ROSE_PINE_DAWN_THEME.as_bytes());
                    ThemeSet::load_from_reader(&mut cursor)
                        .into_diagnostic()
                        .map_err(|e| {
                            miette::miette!("Failed to load embedded rose-pine-dawn theme: {}", e)
                        })
                }
                _ => {
                    // Fall back to syntect's built-in themes
                    let theme_set = ThemeSet::load_defaults();
                    theme_set
                        .themes
                        .get(name.as_str())
                        .ok_or_else(|| miette::miette!("Theme '{}' not found in defaults", name))
                        .cloned()
                }
            }
        }
        ThemeLightCodeTheme::CodeThemeFile(file) => {
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
                .map_err(|e| miette::miette!("Failed to download theme: {}", e))
        }
        _ => {
            let mut cursor = Cursor::new(ROSE_PINE_THEME.as_bytes());
            ThemeSet::load_from_reader(&mut cursor)
                .into_diagnostic()
                .map_err(|e| miette::miette!("Failed to load embedded rose-pine theme: {}", e))
        }
    }
}

pub async fn generate_syntax_css(theme: &ResolvedTheme<'_>) -> miette::Result<String> {
    // Load both themes
    let dark_syntect_theme = load_syntect_dark_theme(&theme.dark_code_theme).await?;
    let light_syntect_theme = load_syntect_light_theme(&theme.light_code_theme).await?;

    // Generate dark mode CSS (default)
    let dark_css = css_for_theme_with_class_style(
        &dark_syntect_theme,
        ClassStyle::SpacedPrefixed {
            prefix: crate::code_pretty::CSS_PREFIX,
        },
    )
    .into_diagnostic()?;

    // Generate light mode CSS
    let light_css = css_for_theme_with_class_style(
        &light_syntect_theme,
        ClassStyle::SpacedPrefixed {
            prefix: crate::code_pretty::CSS_PREFIX,
        },
    )
    .into_diagnostic()?;

    // Combine with media queries
    let mut result = String::new();
    result.push_str("/* Syntax highlighting - Light Mode (default) */\n");
    result.push_str(&light_css);
    result.push_str("\n\n/* Syntax highlighting - Dark Mode */\n");
    result.push_str("@media (prefers-color-scheme: dark) {\n");
    result.push_str(&dark_css);
    result.push_str("}\n");

    Ok(result)
}
