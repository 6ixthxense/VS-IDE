//! Workspace-scoped file operations that keep the index in sync
//! (port of `src/main/ipc/workspace.ts` and `src/main/ipc/file.ts`).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use vsm_core::FileEntry;

use crate::error::FsIndexResult;
use crate::fs_ops;
use crate::index::{FileIndex, IndexConfig};
use crate::workspace::WorkspaceService;

/// An open workspace: root guard + file index + guarded file operations.
///
/// Every path argument is validated with
/// [`WorkspaceService::assert_within_workspace`], every new file/folder name
/// with [`WorkspaceService::assert_workspace_entry_name`], and the
/// [`FileIndex`] is updated after successful mutations. Errors are returned
/// instead of being flattened to `[]` / `''` / `false` like the IPC handlers
/// did; map them at the UI boundary if that behaviour is wanted.
#[derive(Clone, Default)]
pub struct Workspace {
    service: WorkspaceService,
    index: FileIndex,
}

impl Workspace {
    /// A workspace with no folder open, using the default index configuration.
    pub fn new() -> Self {
        Self::default()
    }

    /// A workspace with an explicit index configuration.
    pub fn with_index_config(config: IndexConfig) -> Self {
        Self {
            service: WorkspaceService::new(),
            index: FileIndex::with_config(config),
        }
    }

    /// The root guard.
    pub fn service(&self) -> &WorkspaceService {
        &self.service
    }

    /// The file index (use it to subscribe to change events).
    pub fn index(&self) -> &FileIndex {
        &self.index
    }

    /// Open `root_path` (`None` closes the workspace), pointing the index at
    /// it. Returns the resolved root. May take a while on large trees (starts
    /// the watcher); call from a background thread.
    pub fn set_root_path(&self, root_path: Option<&Path>) -> FsIndexResult<Option<PathBuf>> {
        let resolved = self.service.set_root_path(root_path)?;
        self.index.set_root_path(resolved.as_deref());
        Ok(resolved)
    }

    /// The open root, if any.
    pub fn root_path(&self) -> Option<PathBuf> {
        self.service.root_path()
    }

    /// All files in the workspace (see [`FileIndex::get_files`]).
    pub fn all_files(&self) -> FsIndexResult<Arc<Vec<PathBuf>>> {
        let root = self.service.require_root_path()?;
        self.index.get_files(&root)
    }

    /// Whether `path` exists.
    pub fn path_exists(&self, path: &Path) -> FsIndexResult<bool> {
        let safe = self.service.assert_within_workspace(path, "Path")?;
        Ok(fs_ops::path_exists(&safe))
    }

    /// List a directory for the file tree.
    pub fn read_dir(&self, dir_path: &Path) -> FsIndexResult<Vec<FileEntry>> {
        let safe = self.service.assert_within_workspace(dir_path, "Directory path")?;
        fs_ops::read_dir(&safe)
    }

    /// Read a text file.
    pub fn read_file(&self, file_path: &Path) -> FsIndexResult<String> {
        let safe = self.service.assert_within_workspace(file_path, "File path")?;
        fs_ops::read_file(&safe)
    }

    /// Write a text file.
    pub fn write_file(&self, file_path: &Path, content: &str) -> FsIndexResult<()> {
        let safe = self.service.assert_within_workspace(file_path, "File path")?;
        fs_ops::write_file(&safe, content)
    }

    /// Create an empty file `file_name` in `dir_path`; returns its path.
    pub fn create_file(&self, dir_path: &Path, file_name: &str) -> FsIndexResult<PathBuf> {
        let dir = self.service.assert_within_workspace(dir_path, "Directory path")?;
        let name = self.service.assert_workspace_entry_name(file_name, "File name")?;
        let created = fs_ops::create_file(&dir, &name)?;
        self.index.add_file(&created);
        Ok(created)
    }

    /// Create a folder `folder_name` in `dir_path`; returns its path.
    pub fn create_directory(&self, dir_path: &Path, folder_name: &str) -> FsIndexResult<PathBuf> {
        let dir = self.service.assert_within_workspace(dir_path, "Directory path")?;
        let name = self.service.assert_workspace_entry_name(folder_name, "Folder name")?;
        fs_ops::create_directory(&dir, &name)
    }

    /// Delete a file or folder (recursively).
    pub fn delete_path(&self, target_path: &Path) -> FsIndexResult<()> {
        let safe = self
            .service
            .assert_within_workspace_not_root(target_path, "Target path")?;
        fs_ops::delete_path(&safe)?;
        self.index.remove_path(&safe);
        Ok(())
    }

    /// Rename `old_path` to `new_path` (both inside the workspace).
    pub fn rename_path(&self, old_path: &Path, new_path: &Path) -> FsIndexResult<PathBuf> {
        let old = self
            .service
            .assert_within_workspace_not_root(old_path, "Source path")?;
        let new = self.service.assert_within_workspace(new_path, "Destination path")?;
        let result = fs_ops::rename_path(&old, &new)?;
        self.index.rename_path(&old, &result);
        Ok(result)
    }

    /// Move `old_path` to `new_path`, into it if it is a directory.
    pub fn move_path(&self, old_path: &Path, new_path: &Path) -> FsIndexResult<PathBuf> {
        let old = self
            .service
            .assert_within_workspace_not_root(old_path, "Source path")?;
        let new = self.service.assert_within_workspace(new_path, "Destination path")?;
        let result = fs_ops::move_path(&old, &new)?;
        self.index.rename_path(&old, &result);
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::FsIndexError;
    use std::fs;

    fn open() -> (tempfile::TempDir, Workspace, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let ws = Workspace::with_index_config(IndexConfig {
            watch: false,
            ..IndexConfig::default()
        });
        let root = ws.set_root_path(Some(dir.path())).unwrap().unwrap();
        (dir, ws, root)
    }

    #[test]
    fn operations_require_an_open_workspace_and_stay_inside_it() {
        let ws = Workspace::new();
        assert!(matches!(
            ws.read_file(Path::new("/x")),
            Err(FsIndexError::NoWorkspace)
        ));

        let (_dir, ws, root) = open();
        let outside = root.join("..").join("evil.txt");
        assert!(matches!(
            ws.write_file(&outside, "x"),
            Err(FsIndexError::OutsideWorkspace { .. })
        ));
        assert!(ws.create_file(&root, "../evil.txt").is_err());
        assert!(ws.create_file(&root, "a/b.txt").is_err());
        assert!(ws.delete_path(&root.join("src").join("..").join("..")).is_err());
        assert!(ws.rename_path(&root, &outside).is_err());
    }

    #[test]
    fn the_workspace_root_cannot_be_deleted_renamed_or_moved() {
        let (_dir, ws, root) = open();
        fs::create_dir(root.join("sub")).unwrap();
        for result in [
            ws.delete_path(&root),
            ws.delete_path(&root.join("sub").join("..")),
            ws.rename_path(&root, &root.join("sub").join("x")).map(|_| ()),
            ws.move_path(&root, &root.join("sub")).map(|_| ()),
        ] {
            assert!(matches!(result, Err(FsIndexError::IsWorkspaceRoot { .. })), "{result:?}");
        }
        assert!(root.exists() && root.join("sub").exists());
    }

    #[test]
    fn mutations_keep_the_index_in_sync() {
        let (_dir, ws, root) = open();
        fs::create_dir(root.join("src")).unwrap();
        fs::write(root.join("src").join("a.ts"), "a").unwrap();
        assert_eq!(ws.all_files().unwrap().len(), 1);

        let created = ws.create_file(&root.join("src"), " b.ts ").unwrap();
        assert_eq!(created, root.join("src").join("b.ts"));
        assert!(ws.all_files().unwrap().contains(&created));
        assert!(!ws.index().is_dirty());

        ws.write_file(&created, "hello").unwrap();
        assert_eq!(ws.read_file(&created).unwrap(), "hello");
        assert!(ws.path_exists(&created).unwrap());

        let folder = ws.create_directory(&root, "lib").unwrap();
        let moved = ws.move_path(&created, &folder).unwrap();
        assert_eq!(moved, folder.join("b.ts"));
        let files = ws.all_files().unwrap();
        assert!(files.contains(&moved) && !files.contains(&created));

        let renamed = ws.rename_path(&folder, &root.join("lib2")).unwrap();
        assert!(ws.all_files().unwrap().contains(&renamed.join("b.ts")));

        ws.delete_path(&renamed).unwrap();
        assert_eq!(ws.all_files().unwrap().len(), 1);
        assert!(!ws.path_exists(&renamed).unwrap());

        let listing = ws.read_dir(&root).unwrap();
        assert_eq!(listing.len(), 1);
        assert!(listing[0].is_directory);
        assert!(matches!(
            ws.create_file(&root.join("src"), "a.ts"),
            Err(FsIndexError::AlreadyExists(_))
        ));
    }
}
