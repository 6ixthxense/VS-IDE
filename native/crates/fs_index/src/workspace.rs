//! The open workspace root and path-traversal guards
//! (port of `src/main/services/workspace.ts`).

use std::path::{Path, PathBuf};
use std::sync::{Arc, PoisonError, RwLock};

use crate::error::{FsIndexError, FsIndexResult};
use crate::paths::{is_within_resolved, relative_to, resolve_path};

/// Holds the currently open workspace folder and validates paths against it.
///
/// Cheap to clone; clones share the same root.
#[derive(Debug, Clone, Default)]
pub struct WorkspaceService {
    root: Arc<RwLock<Option<PathBuf>>>,
}

impl WorkspaceService {
    /// A service with no workspace open.
    pub fn new() -> Self {
        Self::default()
    }

    /// Open `root_path` as the workspace (`None` closes it). Returns the
    /// resolved absolute root.
    ///
    /// # Errors
    /// [`FsIndexError::WorkspaceNotFound`] if the path does not exist and
    /// [`FsIndexError::WorkspaceNotDirectory`] if it is not a directory; the
    /// previous root is kept in both cases.
    pub fn set_root_path(&self, root_path: Option<&Path>) -> FsIndexResult<Option<PathBuf>> {
        let resolved = match root_path {
            None => None,
            Some(path) => {
                let resolved = resolve_path(path);
                match std::fs::metadata(&resolved) {
                    Ok(meta) if meta.is_dir() => Some(resolved),
                    Ok(_) => return Err(FsIndexError::WorkspaceNotDirectory),
                    Err(_) => return Err(FsIndexError::WorkspaceNotFound),
                }
            }
        };
        *self.root.write().unwrap_or_else(PoisonError::into_inner) = resolved.clone();
        Ok(resolved)
    }

    /// The open workspace root, if any.
    pub fn root_path(&self) -> Option<PathBuf> {
        self.root
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// The open workspace root.
    ///
    /// # Errors
    /// [`FsIndexError::NoWorkspace`] if no folder is open.
    pub fn require_root_path(&self) -> FsIndexResult<PathBuf> {
        self.root_path().ok_or(FsIndexError::NoWorkspace)
    }

    /// Resolve `target_path` and ensure it is the workspace root or below it.
    ///
    /// # Errors
    /// [`FsIndexError::OutsideWorkspace`] (with `label`) if it escapes the
    /// workspace, [`FsIndexError::NoWorkspace`] if none is open.
    pub fn assert_within_workspace(&self, target_path: &Path, label: &str) -> FsIndexResult<PathBuf> {
        let root = self.require_root_path()?;
        let resolved = resolve_path(target_path);
        if is_within_resolved(&root, &resolved) {
            Ok(resolved)
        } else {
            Err(FsIndexError::OutsideWorkspace {
                label: label.to_owned(),
            })
        }
    }

    /// Like [`WorkspaceService::assert_within_workspace`] but additionally
    /// rejects the workspace root itself. Use for destructive operations
    /// (delete, rename, move).
    ///
    /// # Errors
    /// [`FsIndexError::IsWorkspaceRoot`] if `target_path` resolves to the root.
    pub fn assert_within_workspace_not_root(
        &self,
        target_path: &Path,
        label: &str,
    ) -> FsIndexResult<PathBuf> {
        let resolved = self.assert_within_workspace(target_path, label)?;
        let root = self.require_root_path()?;
        if relative_to(&root, &resolved).is_some_and(|rel| rel.as_os_str().is_empty()) {
            return Err(FsIndexError::IsWorkspaceRoot {
                label: label.to_owned(),
            });
        }
        Ok(resolved)
    }

    /// Validate a single file/folder name: trimmed, non-empty, not `.`/`..`
    /// and free of path separators. Returns the trimmed name.
    pub fn assert_workspace_entry_name(&self, name: &str, label: &str) -> FsIndexResult<String> {
        let trimmed = name.trim();
        if trimmed.is_empty() || trimmed == "." || trimmed == ".." {
            return Err(FsIndexError::InvalidName {
                label: label.to_owned(),
            });
        }
        // Same test as `path.basename(name) !== name`.
        let is_single_component = Path::new(trimmed)
            .file_name()
            .is_some_and(|file_name| file_name == std::ffi::OsStr::new(trimmed));
        if !is_single_component {
            return Err(FsIndexError::NameHasSeparators {
                label: label.to_owned(),
            });
        }
        Ok(trimmed.to_owned())
    }

    /// Workspace-relative form of `target_path` (`.` for the root itself),
    /// suitable for passing to git.
    pub fn to_relative_workspace_path(&self, target_path: &Path) -> FsIndexResult<PathBuf> {
        let root = self.require_root_path()?;
        let resolved = self.assert_within_workspace(target_path, "Path")?;
        let relative = relative_to(&root, &resolved).unwrap_or_default();
        if relative.as_os_str().is_empty() {
            Ok(PathBuf::from("."))
        } else {
            Ok(relative)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> (tempfile::TempDir, WorkspaceService, PathBuf) {
        let dir = tempfile::Builder::new()
            .prefix("vs-monitor-ide-workspace-")
            .tempdir()
            .unwrap();
        let service = WorkspaceService::new();
        let root = service.set_root_path(Some(dir.path())).unwrap().unwrap();
        (dir, service, root)
    }

    #[test]
    fn allows_nested_paths_inside_the_active_workspace_and_returns_a_relative_git_safe_path() {
        let (_dir, service, root) = setup();
        let nested = root.join("src").join("main.ts");

        assert_eq!(
            service.assert_within_workspace(&nested, "Path").unwrap(),
            resolve_path(&nested)
        );
        assert_eq!(
            service.to_relative_workspace_path(&nested).unwrap(),
            Path::new("src").join("main.ts")
        );
        assert_eq!(
            service.to_relative_workspace_path(&root).unwrap(),
            PathBuf::from(".")
        );
    }

    #[test]
    fn rejects_paths_that_escape_the_active_workspace() {
        let (_dir, service, root) = setup();
        let outside = resolve_path(&root.join("..").join("outside.ts"));

        let err = service.assert_within_workspace(&outside, "Target path").unwrap_err();
        assert_eq!(err.to_string(), "Target path must stay inside the open workspace");

        // Dot-dot smuggled inside an otherwise nested path.
        let sneaky = root.join("src").join("..").join("..").join("x");
        assert!(service.assert_within_workspace(&sneaky, "Path").is_err());
        // A sibling that merely shares the root as string prefix.
        let sibling = PathBuf::from(format!("{}-evil/a.ts", root.display()));
        assert!(service.assert_within_workspace(&sibling, "Path").is_err());
    }

    #[test]
    fn rejects_invalid_entry_names_that_try_to_smuggle_path_traversal() {
        let service = WorkspaceService::new();
        assert_eq!(
            service
                .assert_workspace_entry_name("../escape.ts", "File name")
                .unwrap_err()
                .to_string(),
            "File name must not contain path separators"
        );
        assert_eq!(
            service
                .assert_workspace_entry_name("   ", "File name")
                .unwrap_err()
                .to_string(),
            "File name is invalid"
        );
        for bad in [".", "..", "a/b", "dir/"] {
            assert!(service.assert_workspace_entry_name(bad, "Name").is_err(), "{bad}");
        }
        assert_eq!(
            service.assert_workspace_entry_name("  ok.ts ", "Name").unwrap(),
            "ok.ts"
        );
    }

    #[test]
    fn destructive_operations_reject_the_workspace_root() {
        let (_dir, service, root) = setup();
        let err = service
            .assert_within_workspace_not_root(&root.join("src").join(".."), "Target path")
            .unwrap_err();
        assert_eq!(err.to_string(), "Target path must not be the workspace root");
        assert!(service
            .assert_within_workspace_not_root(&root.join("src"), "Target path")
            .is_ok());
        assert!(service
            .assert_within_workspace_not_root(&root.join(".."), "Target path")
            .is_err());
    }

    #[test]
    fn set_root_path_validates_the_folder() {
        let dir = tempfile::tempdir().unwrap();
        let service = WorkspaceService::new();
        assert_eq!(
            service.require_root_path().unwrap_err().to_string(),
            "No workspace folder is open"
        );
        assert_eq!(
            service
                .set_root_path(Some(&dir.path().join("missing")))
                .unwrap_err()
                .to_string(),
            "Workspace path does not exist"
        );
        let file = dir.path().join("f.txt");
        std::fs::write(&file, "x").unwrap();
        assert_eq!(
            service.set_root_path(Some(&file)).unwrap_err().to_string(),
            "Workspace path must be a directory"
        );
        assert_eq!(service.set_root_path(None).unwrap(), None);
    }
}
