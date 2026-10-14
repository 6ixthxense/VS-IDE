//! Atomic file writes.

use std::fs;
use std::io::Write;
use std::path::Path;

use serde::Serialize;

use crate::error::{CoreError, CoreResult};

/// Atomically replace `path` with `bytes`.
///
/// The data is written to a temporary file in the same directory, flushed to
/// disk and then renamed over the destination, so readers observe either the
/// old or the new content, never a partial write. Missing parent directories
/// are created.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> CoreResult<()> {
    let parent = match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    fs::create_dir_all(parent).map_err(|e| CoreError::io(parent, e))?;

    let mut tmp = tempfile::Builder::new()
        .prefix(".vsm-tmp-")
        .tempfile_in(parent)
        .map_err(|e| CoreError::io(parent, e))?;
    tmp.write_all(bytes).map_err(|e| CoreError::io(path, e))?;
    tmp.as_file()
        .sync_all()
        .map_err(|e| CoreError::io(path, e))?;
    persist_with_retry(tmp, path)
}

/// Rename the temp file over `path`. On Windows the rename fails with
/// `PermissionDenied` while another process (antivirus, an indexer, a reader
/// without `FILE_SHARE_DELETE`) briefly holds the destination open, so retry
/// a few times with backoff there.
fn persist_with_retry(mut tmp: tempfile::NamedTempFile, path: &Path) -> CoreResult<()> {
    const ATTEMPTS: u32 = 6;
    let mut attempt = 1;
    loop {
        match tmp.persist(path) {
            Ok(_) => return Ok(()),
            Err(err) => {
                let retryable = cfg!(windows)
                    && err.error.kind() == std::io::ErrorKind::PermissionDenied
                    && attempt < ATTEMPTS;
                if !retryable {
                    return Err(CoreError::io(path, err.error));
                }
                tmp = err.file;
                let backoff_ms = 10 * u64::from(attempt) * u64::from(attempt);
                std::thread::sleep(std::time::Duration::from_millis(backoff_ms));
                attempt += 1;
            }
        }
    }
}

/// Serialize `value` as pretty JSON and write it atomically to `path`.
pub fn atomic_write_json<T: Serialize>(path: &Path, value: &T) -> CoreResult<()> {
    let mut bytes = serde_json::to_vec_pretty(value).map_err(CoreError::Serialize)?;
    bytes.push(b'\n');
    atomic_write(path, &bytes)
}

/// Read `path`; `Ok(None)` if it does not exist.
pub fn read_optional(path: &Path) -> CoreResult<Option<Vec<u8>>> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(CoreError::io(path, e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_and_replaces_atomically_without_leaving_temp_files() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("nested").join("a.json");

        atomic_write(&target, b"one").unwrap();
        atomic_write(&target, b"two").unwrap();

        assert_eq!(fs::read(&target).unwrap(), b"two");
        let entries: Vec<_> = fs::read_dir(target.parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(entries.len(), 1);
    }

    #[test]
    fn read_optional_returns_none_for_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read_optional(&dir.path().join("missing")).unwrap().is_none());
    }
}
