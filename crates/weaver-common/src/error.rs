use miette::{Diagnostic, NamedSource, SourceOffset, SourceSpan};
use std::borrow::Cow;
use std::fmt;

#[derive(thiserror::Error, Debug, Diagnostic)]
#[error("error(s) in weaver")]
pub struct Error<E: fmt::Debug> {
    #[related]
    errors: Vec<WeaverErrorKind<E>>,

    #[help]
    advice: Option<String>,
}

#[derive(thiserror::Error, Debug, Diagnostic)]
pub enum WeaverErrorKind<E: fmt::Debug> {
    #[error(transparent)]
    #[diagnostic_source]
    ParseError(ParseError),
    #[error(transparent)]
    #[diagnostic_source]
    IoError(#[from] IoError),
    #[error(transparent)]
    #[diagnostic_source]
    TaskError(#[from] n0_future::task::JoinError),
    #[error(transparent)]
    #[diagnostic_source]
    AtprotoError(#[from] AtprotoError<E>),
    #[error(transparent)]
    #[diagnostic_source]
    NetworkError(#[from] NetworkError),
    #[error(transparent)]
    #[diagnostic_source]
    SerdeError(#[from] SerDeError),
}

#[derive(thiserror::Error, Debug, Diagnostic)]
#[error("io error")]
pub struct AtprotoError<E: fmt::Debug> {
    #[diagnostic_source]
    kind: AtprotoErrorKind<E>,
}

#[derive(thiserror::Error, Debug, Diagnostic)]
#[non_exhaustive]
pub enum NetworkError {
    #[error(transparent)]
    #[diagnostic_source]
    HttpRequest(#[from] http::Error),
    #[error("HTTP client error: {0}")]
    #[diagnostic_source]
    HttpClient(Box<dyn std::error::Error + Send + Sync + 'static>),
}

#[derive(thiserror::Error, Debug, Diagnostic)]
#[non_exhaustive]
pub enum AtprotoErrorKind<E: fmt::Debug> {
    #[error(transparent)]
    #[diagnostic_source]
    AtriumApi(#[from] atrium_api::error::Error),
    #[error("XRPC error: {:?}", .0)]
    #[diagnostic_source]
    AtriumXrpc(atrium_api::xrpc::error::XrpcError<E>),
    #[error("Authentication error: {:?}", .0)]
    #[diagnostic_source]
    Auth(http::HeaderValue),
    #[error("Unexpected respose type")]
    #[diagnostic_source]
    UnexpectedResponseType,
    #[error("Atrium error: {:?}", .0)]
    #[diagnostic_source]
    AtriumCatchall(atrium_api::xrpc::Error<E>),
}

#[derive(thiserror::Error, Debug, Diagnostic)]
#[error("io error")]
pub struct IoError {
    #[diagnostic_source]
    kind: IoErrorKind,
}

impl From<n0_future::io::Error> for IoError {
    fn from(err: n0_future::io::Error) -> Self {
        Self {
            kind: IoErrorKind::NoIoError(err),
        }
    }
}

#[derive(thiserror::Error, Debug, Diagnostic)]
#[non_exhaustive]
enum IoErrorKind {
    #[error(transparent)]
    NoIoError(#[from] n0_future::io::Error),
}

#[derive(thiserror::Error, Debug, Diagnostic)]
#[error("parse error")]
#[diagnostic()]
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
                SourceSpan::new(SourceOffset::from_location(&src.inner(), line, col), len);
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
    #[error(transparent)]
    MiniJinjaError(#[from] minijinja::Error),
    #[error("Error in Markdown parsing or rendering: {0}")]
    MarkdownError(markdown_weaver::CowStr<'static>),
}

/// Errors that can occur during serialization and deserialization.
/// Thin wrapper over various `merde` and `serde` implementation crate errors.
#[derive(thiserror::Error, Debug, Diagnostic)]
#[non_exhaustive]
pub enum SerDeError {
    #[error(transparent)]
    #[diagnostic_source]
    Merde(#[from] merde::MerdeError<'static>),
    #[error(transparent)]
    #[diagnostic_source]
    SDJson(#[from] serde_json::Error),
    #[error(transparent)]
    Cbor(#[from] serde_cbor::Error),
    #[error(transparent)]
    DagCbor(#[from] serde_ipld_dagcbor::error::CodecError),
    #[error(transparent)]
    SerHtmlForm(#[from] serde_html_form::ser::Error),
}

impl From<serde_json::Error> for ParseError {
    fn from(err: serde_json::Error) -> Self {
        let line = err.line();
        let column = err.column();
        let location = SourceSpan::new(SourceOffset::from_location(&"", line, column), 0);
        Self {
            kind: ParseErrorKind::SerdeError(SerDeError::SDJson(err)),
            src: NamedSource::new(Cow::Borrowed("json"), Cow::Borrowed("")),
            err_location: location,
            advice: None,
            err_line_col: Some((line, column)),
        }
    }
}

impl<E: fmt::Debug> TryFrom<atrium_api::xrpc::error::Error<E>> for SerDeError {
    type Error = atrium_api::xrpc::error::Error<E>;
    fn try_from(err: atrium_api::xrpc::error::Error<E>) -> Result<Self, Self::Error> {
        match err {
            atrium_api::xrpc::error::Error::SerdeJson(e) => Ok(Self::from(e)),
            atrium_api::xrpc::error::Error::SerdeHtmlForm(e) => Ok(Self::from(e)),
            _ => Err(err),
        }
    }
}

impl<E> From<atrium_api::xrpc::error::Error<E>> for WeaverErrorKind<E>
where
    E: fmt::Debug,
{
    fn from(err: atrium_api::xrpc::error::Error<E>) -> Self {
        use atrium_api::xrpc::error::Error::*;

        match err {
            Authentication(e) => {
                let atp_err = AtprotoError {
                    kind: AtprotoErrorKind::Auth(e),
                };
                Self::AtprotoError(atp_err)
            }
            UnexpectedResponseType => {
                let atp_err = AtprotoError {
                    kind: AtprotoErrorKind::UnexpectedResponseType,
                };
                Self::AtprotoError(atp_err)
            }
            XrpcResponse(e) => Self::AtprotoError(AtprotoError {
                kind: AtprotoErrorKind::AtriumXrpc(e),
            }),
            HttpRequest(e) => Self::NetworkError(NetworkError::HttpRequest(e)),
            HttpClient(e) => Self::NetworkError(NetworkError::HttpClient(e)),
            SerdeJson(e) => Self::SerdeError(SerDeError::SDJson(e)),
            SerdeHtmlForm(e) => Self::SerdeError(SerDeError::SerHtmlForm(e)),
        }
    }
}

fn offset_to_line_col(offset: usize, src: &NamedSource<Cow<'static, str>>) -> (usize, usize) {
    let mut acc_chars = 0usize;

    // Noting that I am using `split_inclusive('\n')` rather than `lines()`
    // because `lines()` doesn't include the line endings, so it screws up the
    // line/column calculations.
    for (i, line) in src.inner().split_inclusive('\n').enumerate() {
        acc_chars += line.len();
        // We go by line because it's efficient, so we go past the point
        // indicated by the offset, and then we figure out where it is in the
        // line.
        if offset < acc_chars {
            let mut col = 0usize;
            let line_offset = offset - acc_chars;
            for (byte_idx, _) in line.char_indices() {
                if byte_idx >= line_offset {
                    // i + 1 because lines are 1-indexed
                    return (i + 1, col);
                }
                col += 1;
            }
            return (i + 1, col);
        }
    }
    (src.inner().lines().count(), 0)
}
