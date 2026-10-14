//! vsm-git — port of `src/main/services/git.ts`.
//!
//! The Electron app used `simple-git`; here we shell out to the `git` CLI and
//! parse machine-readable output (`status --porcelain=v2 -z`, `for-each-ref`,
//! `log -z --format=...`).  User supplied strings are only ever passed as
//! `Command` arguments (never through a shell), and pathspecs always follow a
//! `--` separator so they cannot be mistaken for options.

mod cmd;
mod model;
mod ops;
mod status;

pub use model::{
    BranchInfo, Branch, CommitInfo, ChangeKind, DiffRequest, FileChange, GitFileStatus, GitStatus,
    OperationResult, RepoStatus,
};
pub use ops::{
    add, branches, checkout, commit, create_branch, current_branch, diff, discard, is_repo, log,
    pull, push, repo_root, status, status_map, unstage,
};

/// Errors produced by this crate.
#[derive(Debug, thiserror::Error)]
pub enum GitError {
    /// Spawning `git` (or reading a file) failed, e.g. git is not installed.
    #[error("failed to run git: {0}")]
    Io(#[from] std::io::Error),
    /// The directory is not inside a git work tree.
    #[error("not a git repository: {0}")]
    NotARepo(String),
    /// `git` exited with a non-zero status.
    #[error("git {command} failed ({code}): {stderr}")]
    Command {
        command: String,
        code: String,
        stderr: String,
    },
    /// Output of git could not be parsed.
    #[error("unexpected git output: {0}")]
    Parse(String),
    /// A caller supplied argument is not acceptable (e.g. a ref starting with `-`).
    #[error("invalid argument: {0}")]
    InvalidArgument(String),
    /// Discard is not supported for this kind of change.
    #[error("{0}")]
    Unsupported(String),
}

pub type Result<T> = std::result::Result<T, GitError>;
