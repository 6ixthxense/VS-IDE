use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use regex::Regex;

use crate::error::{Result, SearchError};
use crate::options::build_search_regex;
use crate::scope::{Scope, SearchRequest};
use crate::utils::apply_js_replacement;

/// A file that could not be read or rewritten.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplaceFailure {
    pub path: PathBuf,
    pub error: String,
}

/// Outcome of [`replace_in_workspace`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReplaceReport {
    /// Number of files that were rewritten.
    pub changed_files: usize,
    /// Files that could not be processed (I/O errors). Binary and non-UTF-8 files are
    /// skipped silently, like the search does, and are not failures.
    pub failures: Vec<ReplaceFailure>,
    /// `true` when the cancellation flag stopped the run early.
    pub cancelled: bool,
}

/// Rewrite `path` through a temporary file in the same directory (flush + fsync,
/// permissions preserved, then an atomic rename).
fn write_atomically(path: &Path, content: &str) -> io::Result<()> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let permissions = fs::metadata(path)?.permissions();
    let mut temp = tempfile::Builder::new()
        .prefix(".vsm-replace-")
        .suffix(".tmp")
        .tempfile_in(dir)?;
    temp.write_all(content.as_bytes())?;
    temp.flush()?;
    temp.as_file().sync_all()?;
    fs::set_permissions(temp.path(), permissions)?;
    temp.persist(path).map_err(|error| error.error)?;
    Ok(())
}

/// `Ok(true)` if the file was changed. Files that are binary (contain NUL) or not valid
/// UTF-8 are left alone.
fn replace_in_file(path: &Path, regex: &Regex, replacement: &str) -> io::Result<bool> {
    let bytes = fs::read(path)?;
    if bytes.contains(&0) {
        return Ok(false);
    }
    let Ok(content) = String::from_utf8(bytes) else {
        return Ok(false);
    };
    if !regex.is_match(&content) {
        return Ok(false);
    }
    let next = apply_js_replacement(regex, &content, replacement);
    if next == content {
        return Ok(false);
    }
    write_atomically(path, &next)?;
    Ok(true)
}

/// Port of `replaceInWorkspace`. Replaces every match of the query in all candidate
/// files. The replacement uses JavaScript template syntax (`$&`, `$1`, `$<name>`, `$$`),
/// exactly like [`crate::build_replace_preview`], so the preview equals the result.
///
/// Each file is rewritten atomically (temp file + rename, permissions preserved).
/// Per-file errors are collected in the report instead of being swallowed.
pub fn replace_in_workspace(
    request: &SearchRequest,
    replacement: &str,
    cancel: &AtomicBool,
) -> Result<ReplaceReport> {
    if request.query.trim().is_empty() {
        return Err(SearchError::InvalidPattern("Invalid parameters".into()));
    }
    let options = request.options.normalized();
    let regex = build_search_regex(&request.query, &options)?;
    let scope = Scope::new(request)?;
    let files = scope.collect_sorted(cancel)?;

    let mut report = ReplaceReport::default();
    for path in files {
        if cancel.load(Ordering::Relaxed) {
            report.cancelled = true;
            break;
        }
        match replace_in_file(&path, &regex, replacement) {
            Ok(true) => report.changed_files += 1,
            Ok(false) => {}
            Err(error) => report.failures.push(ReplaceFailure {
                path,
                error: error.to_string(),
            }),
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn regex(pattern: &str) -> Regex {
        Regex::new(pattern).expect("regex")
    }

    #[test]
    fn unreadable_files_are_errors_not_silent() {
        let dir = tempfile::tempdir().expect("tempdir");
        let missing = dir.path().join("missing.txt");
        assert!(replace_in_file(&missing, &regex("a"), "b").is_err());
    }

    #[test]
    fn skips_binary_and_non_utf8_files() {
        let dir = tempfile::tempdir().expect("tempdir");
        let binary = dir.path().join("bin.dat");
        fs::write(&binary, b"needle\0needle").expect("write");
        assert!(!replace_in_file(&binary, &regex("needle"), "x").expect("replace"));
        assert_eq!(fs::read(&binary).expect("read"), b"needle\0needle");

        let latin1 = dir.path().join("latin1.txt");
        fs::write(&latin1, b"caf\xe9 needle").expect("write");
        assert!(!replace_in_file(&latin1, &regex("needle"), "x").expect("replace"));
        assert_eq!(fs::read(&latin1).expect("read"), b"caf\xe9 needle");
    }

    #[test]
    fn rewrite_is_atomic_and_leaves_no_temp_files() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("a.txt");
        fs::write(&file, "one needle two").expect("write");
        assert!(replace_in_file(&file, &regex("needle"), "[$&]").expect("replace"));
        assert_eq!(fs::read_to_string(&file).expect("read"), "one [needle] two");
        let names: Vec<_> = fs::read_dir(dir.path())
            .expect("read_dir")
            .map(|e| e.expect("entry").file_name())
            .collect();
        assert_eq!(names.len(), 1, "{names:?}");
    }

    #[cfg(unix)]
    #[test]
    fn rewrite_preserves_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("run.sh");
        fs::write(&file, "echo needle").expect("write");
        fs::set_permissions(&file, fs::Permissions::from_mode(0o751)).expect("chmod");
        assert!(replace_in_file(&file, &regex("needle"), "x").expect("replace"));
        let mode = fs::metadata(&file).expect("meta").permissions().mode() & 0o777;
        assert_eq!(mode, 0o751);
    }
}
