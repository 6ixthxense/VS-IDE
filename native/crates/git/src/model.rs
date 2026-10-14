//! Data types (serde-friendly, camelCase like the TypeScript shared types).

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Mirrors `GitFileStatus` from `shared/types/ipc.ts`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GitFileStatus {
    Modified,
    Added,
    Deleted,
    Renamed,
    Untracked,
    Staged,
}

impl GitFileStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Modified => "modified",
            Self::Added => "added",
            Self::Deleted => "deleted",
            Self::Renamed => "renamed",
            Self::Untracked => "untracked",
            Self::Staged => "staged",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "modified" => Self::Modified,
            "added" => Self::Added,
            "deleted" => Self::Deleted,
            "renamed" => Self::Renamed,
            "untracked" => Self::Untracked,
            "staged" => Self::Staged,
            _ => return None,
        })
    }
}

/// `GitStatus` in the TS code: absolute file path -> status.
pub type GitStatus = BTreeMap<PathBuf, GitFileStatus>;

/// Mirrors `GitDiffRequest`.  `status` is a free-form string in TS
/// (`GitFileStatus | string`), kept as such.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DiffRequest {
    pub file_path: PathBuf,
    pub staged: bool,
    pub status: Option<String>,
}

/// Mirrors `OperationResult`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationResult {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<String>,
}

impl OperationResult {
    pub fn ok() -> Self {
        Self {
            success: true,
            error: None,
            details: None,
        }
    }

    pub fn failure(fallback: &str, error: &dyn std::fmt::Display) -> Self {
        Self {
            success: false,
            error: Some(fallback.to_string()),
            details: Some(error.to_string()),
        }
    }

    pub(crate) fn from_result(result: crate::Result<()>, fallback: &str) -> Self {
        match result {
            Ok(()) => Self::ok(),
            Err(e) => Self::failure(fallback, &e),
        }
    }
}

/// Kind of change for one side (index or work tree) of a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ChangeKind {
    Modified,
    Added,
    Deleted,
    Renamed,
    Copied,
    TypeChanged,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileChange {
    /// Repo-root relative path with `/` separators.
    pub path: String,
    /// For renames/copies, the original path.
    pub orig_path: Option<String>,
    pub kind: ChangeKind,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BranchInfo {
    /// Current commit id (`None` on an unborn branch).
    pub oid: Option<String>,
    /// Branch name, `None` when HEAD is detached.
    pub head: Option<String>,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
}

/// Full parsed `git status`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoStatus {
    /// Absolute path of the work-tree top level.
    pub root: PathBuf,
    pub branch: BranchInfo,
    /// Changes recorded in the index (staged).
    pub staged: Vec<FileChange>,
    /// Changes in the work tree relative to the index (unstaged).
    pub unstaged: Vec<FileChange>,
    /// Untracked files (each file listed, `-uall`).
    pub untracked: Vec<String>,
    /// Unmerged (conflicted) paths.
    pub conflicted: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Branch {
    pub name: String,
    pub current: bool,
    pub remote: bool,
    pub commit: String,
    pub upstream: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitInfo {
    pub hash: String,
    pub short_hash: String,
    pub author_name: String,
    pub author_email: String,
    /// Strict ISO 8601 author date.
    pub date: String,
    pub subject: String,
}
