//! Lexical path helpers shared by the workspace guard and the file index.
//!
//! The implementations live in [`vsm_core::paths`]; this module re-exports the names that
//! `vsm-fs_index` has always offered so existing callers keep compiling.

use std::path::Path;

pub use vsm_core::paths::{
    compare_paths, locale_compare, merge_sorted, relative_to, resolve_path, should_skip_directory,
    should_skip_entry_in_tree, sort_paths,
};

/// `true` if `target` equals `parent` or lies below it. Both paths must already be resolved
/// (see [`resolve_path`]).
pub use vsm_core::paths::is_within as is_within_resolved;

/// Like [`is_within_resolved`] but resolves both paths first.
pub fn is_within(parent: &Path, target: &Path) -> bool {
    is_within_resolved(&resolve_path(parent), &resolve_path(target))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolving_is_within_normalises_first() {
        let root = Path::new("/ws");
        assert!(is_within(root, Path::new("/ws/a/../b")));
        assert!(!is_within(root, Path::new("/ws/../other")));
        assert!(!is_within_resolved(root, Path::new("/ws2/a")));
    }

    #[test]
    fn reexports_are_the_core_helpers() {
        assert!(should_skip_directory(".git"));
        assert!(should_skip_entry_in_tree("_temp_run_1"));
        assert_eq!(locale_compare("a", "B"), std::cmp::Ordering::Less);
        assert_eq!(relative_to(Path::new("/ws"), Path::new("/ws")), Some(Default::default()));
    }
}
