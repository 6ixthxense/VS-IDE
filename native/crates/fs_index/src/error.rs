//! Error type for `vsm-fs_index`.
//!
//! The `Display` text of the workspace-related variants matches the messages
//! of the TypeScript implementation.

use std::io;
use std::path::PathBuf;

/// Result alias used throughout `vsm-fs_index`.
pub type FsIndexResult<T> = Result<T, FsIndexError>;

/// Errors produced by workspace, index and file operations.
#[derive(Debug, thiserror::Error)]
pub enum FsIndexError {
    /// An operation needs an open workspace but none is set.
    #[error("No workspace folder is open")]
    NoWorkspace,
    /// The requested workspace root does not exist.
    #[error("Workspace path does not exist")]
    WorkspaceNotFound,
    /// The requested workspace root is not a directory.
    #[error("Workspace path must be a directory")]
    WorkspaceNotDirectory,
    /// A path escapes the open workspace.
    #[error("{label} must stay inside the open workspace")]
    OutsideWorkspace {
        /// Human readable name of the offending argument (e.g. `Target path`).
        label: String,
    },
    /// The operation must not target the workspace root itself.
    #[error("{label} must not be the workspace root")]
    IsWorkspaceRoot {
        /// Human readable name of the offending argument.
        label: String,
    },
    /// A file/folder name is empty, `.` or `..`.
    #[error("{label} is invalid")]
    InvalidName {
        /// Human readable name of the offending argument.
        label: String,
    },
    /// A file/folder name contains path separators.
    #[error("{label} must not contain path separators")]
    NameHasSeparators {
        /// Human readable name of the offending argument.
        label: String,
    },
    /// The target already exists (message is user presentable).
    #[error("{0}")]
    AlreadyExists(&'static str),
    /// The source does not exist (message is user presentable).
    #[error("{0}")]
    NotFound(&'static str),
    /// An I/O operation on `path` failed.
    #[error("I/O error on {}: {source}", path.display())]
    Io {
        /// Path the operation acted on.
        path: PathBuf,
        /// Underlying error.
        #[source]
        source: io::Error,
    },
    /// The file watcher could not be started.
    #[error("failed to watch {}: {source}", path.display())]
    Watch {
        /// Path being watched.
        path: PathBuf,
        /// Underlying error.
        #[source]
        source: notify::Error,
    },
}

impl FsIndexError {
    pub(crate) fn io(path: impl Into<PathBuf>, source: io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}
