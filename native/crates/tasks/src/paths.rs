//! Lexical path helpers equivalent to Node's `path.resolve` / `path.relative` checks.

use std::path::{Component, Path, PathBuf};

use crate::error::TaskError;

/// Resolve `.` and `..` without touching the filesystem.
pub fn normalize_lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// `path.resolve(base, raw)`: absolute `raw` wins, otherwise it is joined to `base`.
/// `base` is expected to be absolute (see [`try_resolve`] for the checked variant).
pub fn resolve(base: &Path, raw: &Path) -> PathBuf {
    normalize_lexical(&base.join(raw))
}

/// Like [`resolve`], but rejects a relative `base` instead of silently depending on the
/// process working directory.
pub fn try_resolve(base: &Path, raw: &Path) -> Result<PathBuf, TaskError> {
    if !base.is_absolute() {
        return Err(TaskError::NotAbsolute(base.to_path_buf()));
    }
    Ok(resolve(base, raw))
}

fn comparable(path: &Path, case_insensitive: bool) -> PathBuf {
    if case_insensitive {
        PathBuf::from(path.to_string_lossy().to_lowercase())
    } else {
        path.to_path_buf()
    }
}

/// `true` when `target` equals or lies below `parent` (case-insensitive on Windows).
pub fn is_within(parent: &Path, target: &Path) -> bool {
    is_within_with(parent, target, cfg!(windows))
}

pub(crate) fn is_within_with(parent: &Path, target: &Path, case_insensitive: bool) -> bool {
    comparable(target, case_insensitive).starts_with(comparable(parent, case_insensitive))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn containment_can_ignore_case() {
        let parent = Path::new("/Work/Space");
        assert!(!is_within_with(parent, Path::new("/work/space/a"), false));
        assert!(is_within_with(parent, Path::new("/work/space/a"), true));
        assert!(is_within_with(parent, Path::new("/work/space"), true));
        assert!(!is_within_with(parent, Path::new("/work/spaceship"), true));
    }

    #[test]
    fn relative_bases_are_rejected() {
        assert!(matches!(
            try_resolve(Path::new("rel"), Path::new("a")),
            Err(TaskError::NotAbsolute(_))
        ));
        let abs = std::env::temp_dir();
        assert_eq!(try_resolve(&abs, Path::new("a/../b")).expect("resolve"), abs.join("b"));
    }
}
