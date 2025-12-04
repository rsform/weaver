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

/* Scoped to notebook-content container */
.notebook-content {{
    font-family: var(--font-body);
    color: var(--color-text);
    background-color: var(--color-base);
    margin: 0 auto;
    padding: 1rem 0rem;
    word-wrap: break-word;
    overflow-wrap: break-word;
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
    word-wrap: break-word;
    overflow-wrap: break-word;
}}

a {{
    color: var(--color-link);
    text-decoration: none;
}}

.notebook-content a:hover {{
    color: var(--color-emphasis);
    text-decoration: underline;
}}

/* Wikilink validation (editor) */
.link-valid {{
    color: var(--color-link);
}}

.link-broken {{
    color: var(--color-error);
    text-decoration: underline wavy;
    text-decoration-color: var(--color-error);
    opacity: 0.8;
}}

/* Selection */
::selection {{
    background: var(--color-highlight);
    color: var(--color-text);
}}

/* Lists */
ul, ol {{
    margin-left: 1rem;
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
    display: block;
    overflow-x: auto;
    max-width: 100%;
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

/* AT Protocol Embeds - Container */
/* Light mode: paper with shadow, dark mode: blueprint with borders */
.atproto-embed {{
    display: block;
    position: relative;
    max-width: 550px;
    margin: 1rem 0;
    padding: 1rem;
    background: var(--color-surface);
    border-left: 2px solid var(--color-secondary);
    box-shadow: 0 1px 2px color-mix(in srgb, var(--color-text) 8%, transparent);
}}

.atproto-embed:hover {{
    border-left-color: var(--color-primary);
}}

@media (prefers-color-scheme: dark) {{
    .atproto-embed {{
        box-shadow: none;
        border: 1px solid var(--color-border);
        border-left: 2px solid var(--color-secondary);
    }}
}}

.atproto-embed-placeholder {{
    color: var(--color-muted);
    font-style: italic;
}}

.embed-loading {{
    display: block;
    padding: 0.5rem 0;
    color: var(--color-subtle);
    font-family: var(--font-mono);
    font-size: 0.85rem;
}}

/* Embed Author Block */
.embed-author {{
    display: flex;
    align-items: center;
    gap: 0.75rem;
    margin-bottom: 0.75rem;
}}

.embed-avatar {{
    width: 42px;
    height: 42px;
    min-width: 42px;
    min-height: 42px;
    margin: 0;
    flex-shrink: 0;
    object-fit: cover;
}}

.embed-author-info {{
    display: flex;
    flex-direction: column;
    gap: 0.1rem;
    min-width: 0;
}}

.embed-avatar-link {{
    display: block;
    flex-shrink: 0;
}}

.embed-author-name {{
    font-weight: 600;
    color: var(--color-text);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    text-decoration: none;
}}

a.embed-author-name:hover {{
    color: var(--color-link);
}}

.embed-author-handle {{
    font-size: 0.9em;
    font-family: var(--font-mono);
    color: var(--color-subtle);
    text-decoration: none;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
}}

.embed-author-handle:hover {{
    color: var(--color-link);
}}

/* Card-wide clickable link (sits behind content) */
.embed-card-link {{
    position: absolute;
    inset: 0;
    z-index: 0;
}}

.embed-card-link:focus {{
    outline: 2px solid var(--color-primary);
    outline-offset: 2px;
}}

/* Interactive elements sit above the card link */
.embed-author,
.embed-external,
.embed-quote,
.embed-images,
.embed-meta {{
    position: relative;
    z-index: 1;
}}

/* Embed Content Block */
.embed-content {{
    display: block;
    color: var(--color-text);
    line-height: 1.5;
    margin-bottom: 0.75rem;
    white-space: pre-wrap;
}}

.embed-description {{
    display: block;
    color: var(--color-text);
    font-size: 0.95em;
    line-height: 1.4;
}}

/* Embed Metadata Block */
.embed-meta {{
    display: flex;
    justify-content: space-between;
    align-items: center;
    font-size: 0.85em;
    color: var(--color-muted);
    margin-top: 0.75rem;
}}

.embed-stats {{
    display: flex;
    gap: 1rem;
    font-family: var(--font-mono);
}}

.embed-stat {{
    color: var(--color-subtle);
    font-size: 0.9em;
}}

.embed-time {{
    color: var(--color-subtle);
    text-decoration: none;
    font-family: var(--font-mono);
    font-size: 0.9em;
}}

.embed-time:hover {{
    color: var(--color-link);
}}

.embed-type {{
    font-size: 0.8em;
    color: var(--color-subtle);
    font-family: var(--font-mono);
    text-transform: uppercase;
    letter-spacing: 0.05em;
}}

/* Embed URL link (shown with syntax in editor) */
.embed-url {{
    color: var(--color-link);
    font-family: var(--font-mono);
    font-size: 0.9em;
    word-break: break-all;
}}

/* External link cards */
.embed-external {{
    display: flex;
    gap: 0.75rem;
    padding: 0.75rem;
    background: var(--color-surface);
    border: 1px dashed var(--color-border);
    text-decoration: none;
    color: inherit;
    margin-top: 0.5rem;
}}

.embed-external:hover {{
    border-left: 2px solid var(--color-primary);
    margin-left: -1px;
}}

@media (prefers-color-scheme: dark) {{
    .embed-external {{
        border: 1px solid var(--color-border);
    }}

    .embed-external:hover {{
        border-left: 2px solid var(--color-primary);
        margin-left: -1px;
    }}
}}

.embed-external-thumb {{
    width: 120px;
    height: 80px;
    object-fit: cover;
    flex-shrink: 0;
}}

.embed-external-info {{
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
    min-width: 0;
}}

.embed-external-title {{
    font-weight: 600;
    color: var(--color-text);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
}}

.embed-external-description {{
    font-size: 0.9em;
    color: var(--color-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    -webkit-box-orient: vertical;
}}

.embed-external-url {{
    font-size: 0.8em;
    font-family: var(--font-mono);
    color: var(--color-subtle);
}}

/* Image embeds */
.embed-images {{
    display: grid;
    gap: 4px;
    margin-top: 0.5rem;
    overflow: hidden;
}}

.embed-images-1 {{
    grid-template-columns: 1fr;
}}

.embed-images-2 {{
    grid-template-columns: 1fr 1fr;
}}

.embed-images-3 {{
    grid-template-columns: 1fr 1fr;
}}

.embed-images-4 {{
    grid-template-columns: 1fr 1fr;
}}

.embed-image-link {{
    display: block;
    line-height: 0;
}}

.embed-image {{
    width: 100%;
    height: auto;
    max-height: 500px;
    object-fit: cover;
    object-position: center;
    margin: 0;
}}

/* Quoted records */
.embed-quote {{
    display: block;
    margin-top: 0.5rem;
    padding: 0.75rem;
    background: var(--color-overlay);
    border-left: 2px solid var(--color-tertiary);
}}

@media (prefers-color-scheme: dark) {{
    .embed-quote {{
        border: 1px solid var(--color-border);
        border-left: 2px solid var(--color-tertiary);
    }}
}}

.embed-quote .embed-author {{
    margin-bottom: 0.5rem;
}}

.embed-quote .embed-avatar {{
    width: 24px;
    height: 24px;
    min-width: 24px;
    min-height: 24px;
}}

.embed-quote .embed-content {{
    font-size: 0.95em;
    margin-bottom: 0;
}}

/* Placeholder states */
.embed-video-placeholder,
.embed-not-found,
.embed-blocked,
.embed-detached,
.embed-unknown {{
    display: block;
    padding: 1rem;
    background: var(--color-overlay);
    border-left: 2px solid var(--color-border);
    color: var(--color-muted);
    font-style: italic;
    margin-top: 0.5rem;
    font-family: var(--font-mono);
    font-size: 0.9em;
}}

@media (prefers-color-scheme: dark) {{
    .embed-video-placeholder,
    .embed-not-found,
    .embed-blocked,
    .embed-detached,
    .embed-unknown {{
        border: 1px dashed var(--color-border);
    }}
}}

/* Record card embeds (feeds, lists, labelers, starter packs) */
.embed-record-card {{
    display: block;
    margin-top: 0.5rem;
    padding: 0.75rem;
    background: var(--color-overlay);
    border-left: 2px solid var(--color-tertiary);
}}

.embed-record-card > .embed-author-name {{
    display: block;
    font-size: 1.1em;
}}

.embed-subtitle {{
    display: block;
    font-size: 0.85em;
    color: var(--color-muted);
    margin-bottom: 0.5rem;
}}

.embed-record-card .embed-description {{
    display: block;
    margin: 0.5rem 0;
}}

.embed-record-card .embed-stats {{
    display: block;
    margin-top: 0.25rem;
}}

/* Generic record fields */
.embed-fields {{
    display: block;
    margin-top: 0.5rem;
    font-family: var(--font-ui);
    font-size: 0.85rem;
    color: var(--color-muted);
}}

.embed-field {{
    display: block;
    margin-top: 0.25rem;
}}

/* Nested fields get indentation */
.embed-fields .embed-fields {{
    display: block;
    margin-top: 0.5rem;
    margin-left: 1rem;
    padding-left: 0.5rem;
    border-left: 1px solid var(--color-border);
}}

/* Type label inside fields should be block with spacing */
.embed-fields > .embed-author-handle {{
    display: block;
    margin-bottom: 0.25rem;
}}

.embed-field-name {{
    color: var(--color-subtle);
}}

.embed-field-number {{
    color: var(--color-tertiary);
}}

.embed-field-date {{
    color: var(--color-muted);
}}

.embed-field-count {{
    color: var(--color-muted);
    font-style: italic;
}}

.embed-field-bool-true {{
    color: var(--color-success);
}}

.embed-field-bool-false {{
    color: var(--color-muted);
}}

.embed-field-link,
.embed-field-aturi {{
    color: var(--color-link);
    text-decoration: none;
}}

.embed-field-link:hover,
.embed-field-aturi:hover {{
    text-decoration: underline;
}}

.embed-field-did {{
    font-family: var(--font-mono);
    font-size: 0.9em;
}}

.embed-field-did .did-scheme,
.embed-field-did .did-separator {{
    color: var(--color-muted);
}}

.embed-field-did .did-method {{
    color: var(--color-tertiary);
}}

.embed-field-did .did-identifier {{
    color: var(--color-text);
}}

.embed-field-nsid {{
    color: var(--color-secondary);
}}

.embed-field-handle {{
    color: var(--color-link);
}}

/* AT URI highlighting */
.aturi-scheme {{
    color: var(--color-muted);
}}

.aturi-slash {{
    color: var(--color-muted);
}}

.aturi-authority {{
    color: var(--color-link);
}}

.aturi-collection {{
    color: var(--color-secondary);
}}

.aturi-rkey {{
    color: var(--color-tertiary);
}}

/* Generic AT Protocol record embed */
.atproto-record > .embed-author-handle {{
    display: block;
    margin-bottom: 0.25rem;
}}

.atproto-record > .embed-author-name {{
    display: block;
    margin-bottom: 0.5rem;
}}

.atproto-record > .embed-content {{
    margin-bottom: 0.5rem;
}}

/* Notebook entry embed - full width, expandable */
.atproto-entry {{
    max-width: none;
    width: 100%;
    margin: 1.5rem 0;
    padding: 0;
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-left: 1px solid var(--color-border);
    box-shadow: none;
    overflow: hidden;
}}

.atproto-entry:hover {{
    border-left-color: var(--color-border);
}}

@media (prefers-color-scheme: dark) {{
    .atproto-entry {{
        border: 1px solid var(--color-border);
        border-left: 1px solid var(--color-border);
    }}
}}

.embed-entry-header {{
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: 0.5rem 1rem;
    padding: 0.75rem 1rem;
    background: var(--color-overlay);
    border-bottom: 1px solid var(--color-border);
}}

.embed-entry-title {{
    font-size: 1.1em;
    font-weight: 600;
    color: var(--color-text);
}}

.embed-entry-author {{
    font-size: 0.85em;
    color: var(--color-muted);
}}

/* Hidden checkbox for expand/collapse */
.embed-entry-toggle {{
    display: none;
}}

/* Content wrapper - scrollable when collapsed */
.embed-entry-content {{
    max-height: 30rem;
    overflow-y: auto;
    padding: 1rem;
    transition: max-height 0.3s ease;
}}

/* When checkbox is checked, expand fully */
.embed-entry-toggle:checked ~ .embed-entry-content {{
    max-height: none;
}}

/* Expand/collapse button */
.embed-entry-expand {{
    display: block;
    width: 100%;
    padding: 0.5rem;
    text-align: center;
    font-size: 0.85em;
    font-family: var(--font-ui);
    color: var(--color-muted);
    background: var(--color-overlay);
    border-top: 1px solid var(--color-border);
    cursor: pointer;
    user-select: none;
}}

.embed-entry-expand:hover {{
    color: var(--color-text);
    background: var(--color-surface);
}}

/* Toggle button text */
.embed-entry-expand::before {{
    content: "Expand ↓";
}}

.embed-entry-toggle:checked ~ .embed-entry-expand::before {{
    content: "Collapse ↑";
}}

/* Hide expand button if content doesn't overflow (via JS class) */
.atproto-entry.no-overflow .embed-entry-expand {{
    display: none;
}}

/* Horizontal Rule */
hr {{
    border: none;
    border-top: 2px solid var(--color-border);
    margin: 2rem 0;
}}

/* Tablet and mobile responsiveness */
@media (max-width: 900px) {{
    .notebook-content {{
        padding: 1.5rem 1rem;
        max-width: 100%;
    }}

    h1 {{ font-size: 1.85rem; }}
    h2 {{ font-size: 1.4rem; }}
    h3 {{ font-size: 1.2rem; }}

    blockquote {{
        margin-left: 0;
        margin-right: 0;
    }}
}}

/* Small mobile phones */
@media (max-width: 480px) {{
    .notebook-content {{
        padding: 1rem 0.75rem;
    }}

    h1 {{ font-size: 1.65rem; }}
    h2 {{ font-size: 1.3rem; }}
    h3 {{ font-size: 1.1rem; }}

    blockquote {{
        padding-left: 0.75rem;
        padding-right: 0.75rem;
    }}
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

pub fn generate_default_css() -> miette::Result<String> {
    let mut theme_set = ThemeSet::load_defaults();
    let rose_pine = {
        let mut cursor = Cursor::new(ROSE_PINE_THEME.as_bytes());
        ThemeSet::load_from_reader(&mut cursor)
            .into_diagnostic()
            .map_err(|e| miette::miette!("Failed to load embedded rose-pine theme: {}", e))?
    };
    let rose_pine_dawn = {
        let mut cursor = Cursor::new(ROSE_PINE_DAWN_THEME.as_bytes());
        ThemeSet::load_from_reader(&mut cursor)
            .into_diagnostic()
            .map_err(|e| miette::miette!("Failed to load embedded rose-pine-dawn theme: {}", e))?
    };
    theme_set.themes.insert("rose-pine".to_string(), rose_pine);
    theme_set
        .themes
        .insert("rose-pine-dawn".to_string(), rose_pine_dawn);
    // Generate dark mode CSS (default)
    let dark_css = css_for_theme_with_class_style(
        theme_set.themes.get("rose-pine").unwrap(),
        ClassStyle::SpacedPrefixed {
            prefix: crate::code_pretty::CSS_PREFIX,
        },
    )
    .into_diagnostic()?;

    // Generate light mode CSS
    let light_css = css_for_theme_with_class_style(
        theme_set.themes.get("rose-pine-dawn").unwrap(),
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
