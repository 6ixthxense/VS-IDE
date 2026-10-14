use std::time::Duration;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum LspError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("protocol error: {0}")]
    Protocol(String),
    #[error("language server executable `{command}` for `{server}` was not found on PATH")]
    ServerNotFound { server: String, command: String },
    #[error("failed to start language server `{server}`: {message}")]
    Spawn { server: String, message: String },
    #[error("request `{method}` timed out after {after:?}")]
    Timeout { method: String, after: Duration },
    #[error("request was cancelled")]
    Cancelled,
    #[error("language server exited")]
    ServerExited,
    #[error("server returned error {code}: {message}")]
    ServerError {
        code: i64,
        message: String,
        data: Option<serde_json::Value>,
    },
    #[error("no language server configured for language `{0}`")]
    NoServerForLanguage(String),
    #[error("document `{0}` is not open")]
    DocumentNotOpen(String),
    #[error("invalid URI or path: {0}")]
    InvalidUri(String),
    #[error("server only supports full-document sync; full text must be provided")]
    NeedsFullText,
}

impl LspError {
    /// True for errors that mean the connection is gone.
    pub fn is_disconnected(&self) -> bool {
        matches!(self, LspError::ServerExited | LspError::Io(_))
    }
}
