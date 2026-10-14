use thiserror::Error;

/// Errors returned by the terminal backend.
#[derive(Debug, Error)]
pub enum TerminalError {
    /// No session with the given id exists.
    #[error("terminal session `{0}` not found")]
    NotFound(String),
    /// The PTY could not be opened or the shell could not be spawned.
    #[error("failed to spawn terminal process `{program}`: {message}")]
    Spawn { program: String, message: String },
    /// PTY level failure (open, resize, clone reader...).
    #[error("pty error: {0}")]
    Pty(String),
    /// I/O error while talking to the PTY.
    #[error("terminal I/O error: {0}")]
    Io(#[from] std::io::Error),
    /// The session's child process already exited.
    #[error("terminal session `{0}` has exited")]
    Exited(String),
    /// Invalid argument (zero-sized terminal, bad coordinates...).
    #[error("invalid argument: {0}")]
    InvalidArgument(String),
}
