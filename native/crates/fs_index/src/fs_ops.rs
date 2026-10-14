//! Raw file-system operations (port of `src/main/services/fs.ts`).
//!
//! These functions do **no** workspace validation; use
//! [`Workspace`](crate::Workspace) for the guarded versions. Unlike the
//! TypeScript originals, which swallowed errors into `[]` / `''` /
//! `{ success: false }`, every function returns a [`FsIndexResult`] so callers
//! decide how to degrade.

use std::fs;
use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};

use vsm_core::FileEntry;

use crate::error::{FsIndexError, FsIndexResult};
use crate::paths::{locale_compare, should_skip_entry_in_tree};
use vsm_core::paths::names_eq;

/// List the entries of `dir_path` for the file tree: hidden (dot) entries,
/// `node_modules`, build output folders and `_temp_run*` are omitted;
/// directories come first, then names in locale order.
///
/// Symlinks are reported as directories when they point at one (the
/// TypeScript version reported them as files).
pub fn read_dir(dir_path: &Path) -> FsIndexResult<Vec<FileEntry>> {
    let entries = fs::read_dir(dir_path).map_err(|e| FsIndexError::io(dir_path, e))?;
    let mut result = Vec::new();

    for entry in entries {
        let entry = entry.map_err(|e| FsIndexError::io(dir_path, e))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if should_skip_entry_in_tree(&name) {
            continue;
        }
        let path = entry.path();
        let is_directory = match entry.file_type() {
            Ok(t) if t.is_symlink() => fs::metadata(&path).map(|m| m.is_dir()).unwrap_or(false),
            Ok(t) => t.is_dir(),
            Err(_) => false,
        };
        result.push(FileEntry {
            name,
            path,
            is_directory,
            children: None,
        });
    }

    result.sort_by(|a, b| {
        b.is_directory
            .cmp(&a.is_directory)
            .then_with(|| locale_compare(&a.name, &b.name))
    });
    Ok(result)
}

/// Read a file as text. Invalid UTF-8 is replaced with U+FFFD (like Node's
/// `utf8` decoding).
pub fn read_file(file_path: &Path) -> FsIndexResult<String> {
    let bytes = fs::read(file_path).map_err(|e| FsIndexError::io(file_path, e))?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// Write `content` to `file_path`, creating or truncating it.
pub fn write_file(file_path: &Path, content: &str) -> FsIndexResult<()> {
    fs::write(file_path, content).map_err(|e| FsIndexError::io(file_path, e))
}

/// Create an empty file `file_name` inside `dir_path` and return its path.
///
/// # Errors
/// `AlreadyExists("File already exists")` if something is already there.
pub fn create_file(dir_path: &Path, file_name: &str) -> FsIndexResult<PathBuf> {
    let full_path = dir_path.join(file_name);
    match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&full_path)
    {
        Ok(_) => Ok(full_path),
        Err(e) if e.kind() == ErrorKind::AlreadyExists => {
            Err(FsIndexError::AlreadyExists("File already exists"))
        }
        Err(e) => Err(FsIndexError::io(full_path, e)),
    }
}

/// Create the folder `folder_name` inside `dir_path` and return its path.
///
/// # Errors
/// `AlreadyExists("Folder already exists")` if something is already there.
pub fn create_directory(dir_path: &Path, folder_name: &str) -> FsIndexResult<PathBuf> {
    let full_path = dir_path.join(folder_name);
    match fs::create_dir(&full_path) {
        Ok(()) => Ok(full_path),
        Err(e) if e.kind() == ErrorKind::AlreadyExists => {
            Err(FsIndexError::AlreadyExists("Folder already exists"))
        }
        Err(e) => Err(FsIndexError::io(full_path, e)),
    }
}

/// Delete a file, or a directory recursively.
///
/// # Errors
/// `NotFound("Path does not exist")` if the path is missing.
pub fn delete_path(target_path: &Path) -> FsIndexResult<()> {
    let meta = match fs::symlink_metadata(target_path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == ErrorKind::NotFound => {
            return Err(FsIndexError::NotFound("Path does not exist"))
        }
        Err(e) => return Err(FsIndexError::io(target_path, e)),
    };
    let result: io::Result<()> = if meta.is_dir() {
        fs::remove_dir_all(target_path)
    } else if is_directory_link(&meta) {
        // Windows directory symlinks/junctions are removed with `remove_dir`
        // and, like any link, never followed.
        fs::remove_dir(target_path)
    } else {
        fs::remove_file(target_path)
    };
    result.map_err(|e| FsIndexError::io(target_path, e))
}

#[cfg(windows)]
fn is_directory_link(meta: &fs::Metadata) -> bool {
    use std::os::windows::fs::FileTypeExt;
    meta.file_type().is_symlink_dir()
}

#[cfg(not(windows))]
fn is_directory_link(_meta: &fs::Metadata) -> bool {
    false
}

/// Whether `new` merely differs from `old` by letter case and, on this file
/// system, names the same file (a case-only rename on a case-insensitive
/// volume, where `new.exists()` is true because it *is* `old`).
fn is_case_only_rename(old: &Path, new: &Path) -> bool {
    if old == new || !names_eq(old.as_os_str(), new.as_os_str(), true) {
        return false;
    }
    same_file(old, new)
}

#[cfg(unix)]
fn same_file(a: &Path, b: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (fs::symlink_metadata(a), fs::symlink_metadata(b)) {
        (Ok(x), Ok(y)) => x.dev() == y.dev() && x.ino() == y.ino(),
        _ => false,
    }
}

#[cfg(not(unix))]
fn same_file(a: &Path, b: &Path) -> bool {
    // Windows paths that differ only by case name the same entry (file IDs
    // are not available on stable Rust).
    a.exists() && b.exists()
}

/// Rename `old_path` to exactly `new_path`; refuses to overwrite.
///
/// # Errors
/// `NotFound("Source path does not exist")` /
/// `AlreadyExists("Destination path already exists")`.
pub fn rename_path(old_path: &Path, new_path: &Path) -> FsIndexResult<PathBuf> {
    if !old_path.exists() {
        return Err(FsIndexError::NotFound("Source path does not exist"));
    }
    if new_path.exists() && !is_case_only_rename(old_path, new_path) {
        return Err(FsIndexError::AlreadyExists("Destination path already exists"));
    }
    fs::rename(old_path, new_path).map_err(|e| FsIndexError::io(old_path, e))?;
    Ok(new_path.to_path_buf())
}

/// Move `old_path` to `new_path`; if `new_path` is an existing directory the
/// item is moved *inside* it. Returns the final path.
///
/// # Errors
/// `NotFound("Source path does not exist")` /
/// `AlreadyExists("Destination already exists")`.
pub fn move_path(old_path: &Path, new_path: &Path) -> FsIndexResult<PathBuf> {
    if !old_path.exists() {
        return Err(FsIndexError::NotFound("Source path does not exist"));
    }
    let final_path = if new_path.is_dir() {
        match old_path.file_name() {
            Some(name) => new_path.join(name),
            None => new_path.to_path_buf(),
        }
    } else {
        new_path.to_path_buf()
    };
    if final_path.exists() && !is_case_only_rename(old_path, &final_path) {
        return Err(FsIndexError::AlreadyExists("Destination already exists"));
    }
    fs::rename(old_path, &final_path).map_err(|e| FsIndexError::io(old_path, e))?;
    Ok(final_path)
}

/// Whether `file_path` exists (broken symlinks and I/O errors count as missing).
pub fn path_exists(file_path: &Path) -> bool {
    file_path.exists()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_dir_lists_directories_first_and_hides_noise() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        for d in ["src", "Zeta", "node_modules", ".git", "dist", "_temp_run_1", "alpha"] {
            fs::create_dir(p.join(d)).unwrap();
        }
        for f in ["b.txt", "A.txt", ".env", "README.md"] {
            fs::write(p.join(f), "").unwrap();
        }

        let entries = read_dir(p).unwrap();
        let names: Vec<_> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["alpha", "src", "Zeta", "A.txt", "b.txt", "README.md"]);
        assert!(entries[0].is_directory && !entries[3].is_directory);
        assert_eq!(entries[1].path, p.join("src"));
        assert!(read_dir(&p.join("missing")).is_err());
    }

    #[test]
    fn create_rename_move_delete_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();

        let file = create_file(p, "a.txt").unwrap();
        assert_eq!(
            create_file(p, "a.txt").unwrap_err().to_string(),
            "File already exists"
        );
        write_file(&file, "héllo").unwrap();
        assert_eq!(read_file(&file).unwrap(), "héllo");

        let folder = create_directory(p, "sub").unwrap();
        assert_eq!(
            create_directory(p, "sub").unwrap_err().to_string(),
            "Folder already exists"
        );

        let renamed = rename_path(&file, &p.join("b.txt")).unwrap();
        assert!(!file.exists() && renamed.exists());
        assert_eq!(
            rename_path(&file, &p.join("c.txt")).unwrap_err().to_string(),
            "Source path does not exist"
        );
        create_file(p, "other.txt").unwrap();
        assert_eq!(
            rename_path(&renamed, &p.join("other.txt")).unwrap_err().to_string(),
            "Destination path already exists"
        );

        // Moving into a directory keeps the file name.
        let moved = move_path(&renamed, &folder).unwrap();
        assert_eq!(moved, folder.join("b.txt"));
        assert_eq!(
            move_path(&p.join("other.txt"), &folder.join("b.txt"))
                .unwrap_err()
                .to_string(),
            "Destination already exists"
        );

        delete_path(&folder).unwrap();
        assert!(!folder.exists());
        assert_eq!(delete_path(&folder).unwrap_err().to_string(), "Path does not exist");
        assert!(path_exists(&p.join("other.txt")));
        assert!(!path_exists(&p.join("nope")));
    }

    #[cfg(unix)]
    #[test]
    fn case_only_rename_of_the_same_file_is_allowed_but_distinct_files_are_not() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        // Stand-in for a case-insensitive volume: two names, one inode.
        fs::write(p.join("readme.md"), "x").unwrap();
        fs::hard_link(p.join("readme.md"), p.join("README.md")).unwrap();
        assert!(rename_path(&p.join("readme.md"), &p.join("README.md")).is_ok());

        // Different files whose names differ only by case must not be clobbered.
        fs::write(p.join("a"), "1").unwrap();
        fs::write(p.join("A"), "2").unwrap();
        assert_eq!(
            rename_path(&p.join("a"), &p.join("A")).unwrap_err().to_string(),
            "Destination path already exists"
        );
        assert_eq!(fs::read_to_string(p.join("A")).unwrap(), "2");
    }

    #[cfg(unix)]
    #[test]
    fn deleting_a_symlink_removes_the_link_not_the_target() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        fs::create_dir(p.join("target")).unwrap();
        fs::write(p.join("target").join("keep.txt"), "x").unwrap();
        std::os::unix::fs::symlink(p.join("target"), p.join("link")).unwrap();
        delete_path(&p.join("link")).unwrap();
        assert!(!p.join("link").exists());
        assert!(p.join("target").join("keep.txt").exists());
    }

    #[test]
    fn read_file_tolerates_invalid_utf8() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("bin");
        fs::write(&f, [b'a', 0xff, b'b']).unwrap();
        assert_eq!(read_file(&f).unwrap(), "a\u{fffd}b");
    }
}
