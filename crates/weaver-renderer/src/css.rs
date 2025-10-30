use crate::theme::Theme;
use miette::IntoDiagnostic;
use syntect::highlighting::ThemeSet;
use syntect::html::{ClassStyle, css_for_theme_with_class_style};
use syntect::parsing::SyntaxSet;

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
    max-width: 65ch;
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

h1 {{ font-size: 2.5rem; }}
h2 {{ font-size: 2rem; }}
h3 {{ font-size: 1.5rem; }}
h4 {{ font-size: 1.25rem; }}
h5 {{ font-size: 1.125rem; }}
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

/* Horizontal Rule */
hr {{
    border: none;
    border-top: 1px solid rgba(0, 0, 0, 0.1);
    margin: 2rem 0;
}}
"#,
        theme.colors.background,
        theme.colors.foreground,
        theme.colors.link,
        theme.colors.link_hover,
        theme.fonts.body,
        theme.fonts.heading,
        theme.fonts.monospace,
        theme.spacing.base_font_size,
        theme.spacing.line_height,
        theme.spacing.scale,
    )
}

pub fn generate_syntax_css(
    syntect_theme_name: &str,
    _syntax_set: &SyntaxSet,
) -> miette::Result<String> {
    let theme_set = ThemeSet::load_defaults();
    let theme = theme_set
        .themes
        .get(syntect_theme_name)
        .ok_or_else(|| miette::miette!("Theme '{}' not found", syntect_theme_name))?;

    let css = css_for_theme_with_class_style(
        theme,
        ClassStyle::SpacedPrefixed {
            prefix: crate::code_pretty::CSS_PREFIX,
        },
    )
    .into_diagnostic()?;

    Ok(css)
}
