//! LaTeX math rendering via pulldown-latex → MathML

use markdown_weaver_escape::escape_html;
use pulldown_latex::{
    config::DisplayMode, config::RenderConfig, mathml::push_mathml, Parser, Storage,
};

/// Result of attempting to render LaTeX math
pub enum MathResult {
    /// Successfully rendered MathML
    Success(String),
    /// Rendering failed - contains fallback HTML with source and error message
    Error { html: String, message: String },
}

/// Render LaTeX math to MathML
///
/// # Arguments
/// * `latex` - The LaTeX source string (without delimiters like $ or $$)
/// * `display_mode` - If true, render as display math (block); if false, inline
pub fn render_math(latex: &str, display_mode: bool) -> MathResult {
    let storage = Storage::new();
    let parser = Parser::new(latex, &storage);
    let config = RenderConfig {
        display_mode: if display_mode {
            DisplayMode::Block
        } else {
            DisplayMode::Inline
        },
        ..Default::default()
    };

    let mut mathml = String::new();

    // Collect events, tracking any errors
    let events: Vec<_> = parser.collect();
    let errors: Vec<String> = events
        .iter()
        .filter_map(|e| e.as_ref().err().map(|err| err.to_string()))
        .collect();

    if errors.is_empty() {
        // All events parsed successfully - push_mathml wants the Results directly
        if let Err(e) = push_mathml(&mut mathml, events.into_iter(), config) {
            return MathResult::Error {
                html: format_error_html(latex, &e.to_string(), display_mode),
                message: e.to_string(),
            };
        }
        MathResult::Success(mathml)
    } else {
        // Had parse errors - return error HTML
        let error_msg = errors.join("; ");
        MathResult::Error {
            html: format_error_html(latex, &error_msg, display_mode),
            message: error_msg,
        }
    }
}

fn format_error_html(latex: &str, error: &str, display_mode: bool) -> String {
    let mode_class = if display_mode {
        "math-display"
    } else {
        "math-inline"
    };
    let mut escaped_latex = String::new();
    let mut escaped_error = String::new();
    // These won't fail writing to String
    let _ = escape_html(&mut escaped_latex, latex);
    let _ = escape_html(&mut escaped_error, error);
    format!(
        r#"<span class="math math-error {mode_class}" title="{escaped_error}"><code>{escaped_latex}</code></span>"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_inline_math() {
        let result = render_math("x^2", false);
        assert!(matches!(result, MathResult::Success(_)));
        if let MathResult::Success(mathml) = result {
            assert!(mathml.contains("<math"));
            assert!(mathml.contains("</math>"));
        }
    }

    #[test]
    fn renders_display_math() {
        let result = render_math(r"\frac{a}{b}", true);
        assert!(matches!(result, MathResult::Success(_)));
        if let MathResult::Success(mathml) = result {
            assert!(mathml.contains("<math"));
            assert!(mathml.contains("<mfrac"));
        }
    }

    #[test]
    fn renders_complex_math() {
        let result = render_math(r"\sum_{i=0}^{n} x_i", true);
        assert!(matches!(result, MathResult::Success(_)));
    }

    #[test]
    fn handles_invalid_latex() {
        // Unclosed brace
        let result = render_math(r"\frac{a", false);
        assert!(matches!(result, MathResult::Error { .. }));
        if let MathResult::Error { html, message } = result {
            assert!(html.contains("math-error"));
            assert!(!message.is_empty());
        }
    }
}
