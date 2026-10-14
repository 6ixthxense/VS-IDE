//! Lexical path helpers equivalent to Node's `path.resolve` / `path.relative` checks.

use std::path::{Path, PathBuf};

pub use vsm_core::paths::{is_within, normalize_lexical};

use crate::error::TaskError;

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

#[cfg(test)]
mod tests {
    use super::*;

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
