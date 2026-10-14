use std::path::PathBuf;

/// Errors of the database layer.  Adapter methods that mirror the TS API convert these into
/// `success: false` / `error: Some(..)` result values, like the original.
#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("{0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Csv(#[from] csv::Error),
    #[error("{0}")]
    Xlsx(#[from] rust_xlsxwriter::XlsxError),
    #[error("{0}")]
    XlsxRead(#[from] calamine::XlsxError),
    #[error("{0}")]
    Calamine(#[from] calamine::Error),
    #[error("No workspace folder is open")]
    NoWorkspace,
    #[error("{label} must stay inside the open workspace")]
    OutsideWorkspace { label: String },
    #[error("{0}")]
    Invalid(String),
    #[error("Query timed out after {0} ms")]
    Timeout(u64),
    #[error("Query was cancelled")]
    Cancelled,
    #[error("unsupported file: {}", .0.display())]
    UnsupportedFile(PathBuf),
}

impl DbError {
    pub(crate) fn invalid(message: impl Into<String>) -> Self {
        Self::Invalid(message.into())
    }
}

pub type Result<T> = std::result::Result<T, DbError>;
