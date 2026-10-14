//! `vsm-fs_index` - workspace file index, file watching and workspace-scoped
//! file operations for the native VS-Monitor IDE.
//!
//! Port of the Electron main-process services `fileIndex.ts`, `workspace.ts`,
//! `fs.ts` and the `ipc/workspace.ts` / `ipc/file.ts` glue:
//!
//! * [`WorkspaceService`] - the open workspace root plus path-traversal guards
//!   ([`WorkspaceService::assert_within_workspace`],
//!   [`WorkspaceService::assert_workspace_entry_name`]).
//! * [`FileIndex`] - a sorted, cached list of all files in the workspace,
//!   built with the `ignore` crate (honours `.gitignore` and the built-in skip
//!   list), kept up to date incrementally by a debounced `notify` watcher, and
//!   observable through callbacks or a channel ([`IndexEvent`]).
//! * [`fs_ops`] - raw file/folder operations (tree listing with directories
//!   first, read/write/create/rename/move/delete).
//! * [`Workspace`] - the facade that combines the three the way the IPC
//!   handlers did: every operation is validated against the workspace root and
//!   the index is updated after mutations.
//!
//! Everything is synchronous; call potentially slow operations
//! ([`FileIndex::get_files`], [`Workspace::set_root_path`]) from a background
//! thread. Watcher callbacks run on the watcher's own thread.

pub mod error;
pub mod fs_ops;
mod host;
mod ignore_rules;
mod index;
pub mod paths;
mod watcher;
pub mod workspace;

pub use error::{FsIndexError, FsIndexResult};
pub use host::Workspace;
pub use index::{FileIndex, IndexConfig, IndexEvent, Subscription};
pub use paths::{should_skip_directory, should_skip_entry_in_tree};
pub use workspace::WorkspaceService;
