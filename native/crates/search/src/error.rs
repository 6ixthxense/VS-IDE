use std::io;
use std::path::PathBuf;

/// Errors produced by the search crate.
#[derive(Debug, thiserror::Error)]
pub enum SearchError {
    /// The query could not be compiled into a regular expression.
    #[error("invalid search pattern: {0}")]
    InvalidPattern(String),
    /// The workspace root must be an absolute path.
    #[error("workspace root must be an absolute path: {}", .0.display())]
    RootNotAbsolute(PathBuf),
    /// The scope target (file or directory) lives outside the workspace root.
    #[error("{} is outside of the workspace root {}", .target.display(), .root.display())]
    OutsideRoot { root: PathBuf, target: PathBuf },
    /// An I/O error that prevents the whole operation (not a single unreadable file).
    #[error("I/O error on {}: {source}", .path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    /// The operation observed the cancellation flag.
    #[error("search was cancelled")]
    Cancelled,
    /// The background search thread panicked.
    #[error("search worker panicked")]
    WorkerPanicked,
}

pub type Result<T> = std::result::Result<T, SearchError>;
