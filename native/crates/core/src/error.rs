//! Error type for `vsm-core`.

use std::io;
use std::path::PathBuf;

/// Result alias used throughout `vsm-core`.
pub type CoreResult<T> = Result<T, CoreError>;

/// Errors produced by persistence helpers.
#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    /// An I/O operation on `path` failed.
    #[error("I/O error on {}: {source}", path.display())]
    Io {
        /// The path the operation was acting on.
        path: PathBuf,
        /// Underlying error.
        #[source]
        source: io::Error,
    },
    /// A value could not be serialized to JSON.
    #[error("failed to serialize JSON: {0}")]
    Serialize(#[source] serde_json::Error),
    /// The platform has no usable config directory (no home directory).
    #[error("could not determine the OS config directory")]
    NoConfigDir,
}

impl CoreError {
    pub(crate) fn io(path: impl Into<PathBuf>, source: io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}
