//! Shared data types (port of `src/shared/types/file.ts`).
//!
//! Serialized with `camelCase` field names so snapshots stay readable by the
//! TypeScript implementation.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Recovery status attached to a [`Tab`] after a session restore or an
/// external disk change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TabRecoveryState {
    /// Unsaved edits from a previous session were restored.
    Recovered,
    /// The buffer and the file on disk diverged.
    Conflict,
}

/// One node of the file tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileEntry {
    /// File or directory name (last path component).
    pub name: String,
    /// Full path of the entry.
    pub path: PathBuf,
    /// Whether the entry is a directory.
    pub is_directory: bool,
    /// Loaded children, if the directory has been expanded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub children: Option<Vec<FileEntry>>,
}

/// An open editor tab.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tab {
    /// Stable tab identifier.
    pub id: String,
    /// Path of the file shown in the tab.
    pub path: PathBuf,
    /// Display name.
    pub name: String,
    /// Current (possibly unsaved) buffer content.
    pub content: String,
    /// Content as last known to be on disk.
    pub saved_content: String,
    /// Language identifier (e.g. `typescript`).
    pub language: String,
    /// Recovery state, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recovery_state: Option<TabRecoveryState>,
    /// Human readable explanation of [`Tab::recovery_state`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recovery_message: Option<String>,
}

impl Tab {
    /// Whether the buffer differs from the saved content.
    pub fn is_dirty(&self) -> bool {
        self.content != self.saved_content
    }
}
