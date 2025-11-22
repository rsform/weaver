//! Error types for weaver - thin wrapper over jacquard errors

use jacquard::{types::string::AtStrError, xrpc::GenericXrpcError};
use miette::{Diagnostic, NamedSource, SourceOffset, SourceSpan};
use std::borrow::Cow;

/// Main error type for weaver operations
#[derive(thiserror::Error, Debug, Diagnostic)]
pub enum WeaverError {
    /// Jacquard Agent error
    #[error(transparent)]
    #[diagnostic_source]
    Agent(#[from] jacquard::client::error::AgentError),

    /// Jacquard Identity resolution error
    #[error(transparent)]
    #[diagnostic_source]
    Identity(#[from] jacquard::identity::resolver::IdentityError),

    /// Invalid notebook structure
    #[error("invalid notebook structure: {0}")]
    InvalidNotebook(String),

    /// Markdown parsing/rendering error
    #[error("markdown error: {0}")]
    Markdown(String),

    /// IO error
    #[error(transparent)]
    Io(#[from] n0_future::io::Error),

    /// Parse error with source location
    #[error(transparent)]
    #[diagnostic_source]
    Parse(#[from] ParseError),

    /// Serialization/deserialization error
    #[error(transparent)]
    #[diagnostic_source]
    Serde(#[from] SerDeError),

    /// Task join error
    #[error(transparent)]
    Task(#[from] n0_future::task::JoinError),

    /// atproto string parsing error
    #[error(transparent)]
    AtprotoString(#[from] AtStrError),

    /// XRPC error
    #[error(transparent)]
    Xrpc(#[from] jacquard::xrpc::XrpcError<GenericXrpcError>),
}

/// Parse error with source code location information
#[derive(thiserror::Error, Debug, Diagnostic)]
#[error("parse error: {}",self.kind)]
#[diagnostic(code(weaver::parse))]
pub struct ParseError {
    #[diagnostic_source]
    kind: ParseErrorKind,
    #[source_code]
    src: NamedSource<Cow<'static, str>>,
    #[label("error")]
    err_location: SourceSpan,
    err_line_col: Option<(usize, usize)>,
    #[help]
    advice: Option<String>,
}

impl ParseError {
    pub fn with_source(self, src: NamedSource<Cow<'static, str>>) -> Self {
        if let Some((line, column)) = self.err_line_col {
            let location = SourceSpan::new(
                SourceOffset::from_location(src.inner(), line, column),
                self.err_location.len(),
            );
            Self {
                kind: self.kind,
                src,
                err_location: location,
                err_line_col: Some((line, column)),
                advice: self.advice,
            }
        } else {
            let (line, col) = offset_to_line_col(self.err_location.offset(), &self.src);
            let len = self.err_location.len();
            let location =
                SourceSpan::new(SourceOffset::from_location(src.inner(), line, col), len);
            Self {
                kind: self.kind,
                src,
                err_location: location,
                err_line_col: self.err_line_col,
                advice: self.advice,
            }
        }
    }
}

#[derive(thiserror::Error, Debug, Diagnostic)]
#[non_exhaustive]
pub enum ParseErrorKind {
    #[error(transparent)]
    SerdeError(#[from] SerDeError),
    #[error("error in markdown parsing or rendering: {0}")]
    MarkdownError(markdown_weaver::CowStr<'static>),
}

/// Serialization/deserialization errors
#[derive(thiserror::Error, Debug, Diagnostic)]
#[non_exhaustive]
pub enum SerDeError {
    #[error(transparent)]
    #[diagnostic_source]
    Json(#[from] serde_json::Error),
}

impl From<serde_json::Error> for ParseError {
    fn from(err: serde_json::Error) -> Self {
        let line = err.line();
        let column = err.column();
        let location = SourceSpan::new(SourceOffset::from_location("", line, column), 0);
        Self {
            kind: ParseErrorKind::SerdeError(SerDeError::Json(err)),
            src: NamedSource::new(Cow::Borrowed("json"), Cow::Borrowed("")),
            err_location: location,
            advice: None,
            err_line_col: Some((line, column)),
        }
    }
}

fn offset_to_line_col(offset: usize, src: &NamedSource<Cow<'static, str>>) -> (usize, usize) {
    let mut acc_chars = 0usize;

    for (i, line) in src.inner().split_inclusive('\n').enumerate() {
        acc_chars += line.len();
        if offset < acc_chars {
            let mut col = 0usize;
            let line_offset = offset - acc_chars;
            for (byte_idx, _) in line.char_indices() {
                if byte_idx >= line_offset {
                    return (i + 1, col);
                }
                col += 1;
            }
            return (i + 1, col);
        }
    }
    (src.inner().lines().count(), 0)
}
