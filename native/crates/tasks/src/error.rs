use std::io;
use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum TaskError {
    /// `stop_task` was called for a run that already finished (or never existed).
    #[error("Task is no longer running")]
    NotRunning,
    /// The OS refused to terminate the process tree.
    #[error("Task could not be stopped: {0}")]
    StopFailed(String),
    /// The task process could not be started.
    #[error("Unable to start this task: {0}")]
    Spawn(#[source] io::Error),
    /// A workspace root or task cwd that is not an absolute path.
    #[error("path must be absolute: {}", .0.display())]
    NotAbsolute(PathBuf),
    /// A task `cwd` pointing outside of the open workspace.
    #[error("Task cwd must stay inside the open workspace ({})", .0.display())]
    OutsideWorkspace(PathBuf),
}

pub type Result<T> = std::result::Result<T, TaskError>;
