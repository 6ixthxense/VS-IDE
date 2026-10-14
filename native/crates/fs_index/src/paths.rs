//! Lexical path helpers shared by the workspace guard and the file index.
//!
//! These mirror Node's `path.resolve` / `path.relative` semantics: paths are
//! made absolute and normalised **lexically** (no symlink resolution, no file
//! system access beyond the current directory). On Windows comparisons are
//! case-insensitive.

use std::cmp::Ordering;
use std::path::{Component, Path, PathBuf};

/// Make `path` absolute (relative to the current directory) and remove `.` and
/// `..` components lexically, like Node's `path.resolve`.
pub fn resolve_path(path: &Path) -> PathBuf {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    };

    let mut out = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                // `pop` refuses to remove a root/prefix, matching `/..` == `/`.
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Case-fold a path for comparison (Windows only).
fn fold(path: &Path) -> PathBuf {
    if cfg!(windows) {
        PathBuf::from(path.to_string_lossy().to_lowercase())
    } else {
        path.to_path_buf()
    }
}

/// `true` if `target` equals `parent` or lies below it. Both paths must
/// already be resolved (see [`resolve_path`]).
pub fn is_within_resolved(parent: &Path, target: &Path) -> bool {
    if cfg!(windows) {
        fold(target).starts_with(fold(parent))
    } else {
        target.starts_with(parent)
    }
}

/// Like [`is_within_resolved`] but resolves both paths first.
pub fn is_within(parent: &Path, target: &Path) -> bool {
    is_within_resolved(&resolve_path(parent), &resolve_path(target))
}

/// The part of `target` below `base` (empty if equal), or `None` if `target`
/// is not inside `base`. Both paths must already be resolved.
pub fn relative_to(base: &Path, target: &Path) -> Option<PathBuf> {
    if !is_within_resolved(base, target) {
        return None;
    }
    Some(
        target
            .components()
            .skip(base.components().count())
            .collect(),
    )
}

/// Directory names that are never indexed or listed: `node_modules`, build
/// output folders and every dot-directory (including `.git`).
pub fn should_skip_directory(name: &str) -> bool {
    matches!(
        name,
        "node_modules" | "dist" | "dist-electron" | "dist-renderer" | ".git"
    ) || name.starts_with('.')
}

/// Whether `name` is hidden from the file tree listing (superset of
/// [`should_skip_directory`] for tree view: also `_temp_run*`). Applies to both
/// files and directories, as in the TypeScript `readDir`.
pub fn should_skip_entry_in_tree(name: &str) -> bool {
    should_skip_directory(name) || name.starts_with("_temp_run")
}

fn char_class(c: char) -> u8 {
    if c.is_alphabetic() {
        2
    } else if c.is_numeric() {
        1
    } else {
        0
    }
}

/// Approximation of JavaScript's `String.prototype.localeCompare` for file
/// names: punctuation sorts before digits before letters, letters compare
/// case-insensitively first (lowercase before uppercase on ties), and the
/// result is a total order.
///
/// Allocation-free and lazy: the comparison stops at the first difference.
pub fn locale_compare(a: &str, b: &str) -> Ordering {
    let primary = a
        .chars()
        .flat_map(char::to_lowercase)
        .map(|c| (char_class(c), c))
        .cmp(b.chars().flat_map(char::to_lowercase).map(|c| (char_class(c), c)));
    primary
        .then_with(|| {
            // Lowercase before uppercase at the first differing character.
            for (x, y) in a.chars().zip(b.chars()) {
                if x != y {
                    return match (x.is_lowercase(), y.is_lowercase()) {
                        (true, false) => Ordering::Less,
                        (false, true) => Ordering::Greater,
                        _ => Ordering::Equal,
                    };
                }
            }
            Ordering::Equal
        })
        .then_with(|| a.cmp(b))
}

/// [`locale_compare`] on the string form of two paths.
pub fn compare_paths(a: &Path, b: &Path) -> Ordering {
    locale_compare(&a.to_string_lossy(), &b.to_string_lossy())
}

/// Sort key that orders like [`locale_compare`] on a precomputed string.
#[derive(Debug, PartialEq, Eq)]
struct LocaleKey(String);

impl Ord for LocaleKey {
    fn cmp(&self, other: &Self) -> Ordering {
        locale_compare(&self.0, &other.0)
    }
}

impl PartialOrd for LocaleKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Sort `paths` in [`compare_paths`] order. Keys are computed once per path
/// and relative to `root`, so the shared prefix is never compared and nothing
/// is allocated per comparison (O(n log n) cheap comparisons).
pub fn sort_paths(paths: &mut [PathBuf], root: &Path) {
    paths.sort_by_cached_key(|p| {
        let shown = p.strip_prefix(root).unwrap_or(p);
        LocaleKey(shown.to_string_lossy().into_owned())
    });
}

/// Merge two slices that are each sorted in [`compare_paths`] order into one
/// sorted, de-duplicated vector in O(n + m).
pub fn merge_sorted(a: Vec<PathBuf>, b: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut out = Vec::with_capacity(a.len() + b.len());
    let mut left = a.into_iter().peekable();
    let mut right = b.into_iter().peekable();
    loop {
        let next = match (left.peek(), right.peek()) {
            (Some(l), Some(r)) => match compare_paths(l, r) {
                Ordering::Less => left.next(),
                Ordering::Greater => right.next(),
                Ordering::Equal => {
                    right.next();
                    left.next()
                }
            },
            (Some(_), None) => left.next(),
            (None, Some(_)) => right.next(),
            (None, None) => break,
        };
        out.extend(next);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_removes_dot_segments() {
        assert_eq!(resolve_path(Path::new("/a/b/../c/./d")), PathBuf::from("/a/c/d"));
        assert_eq!(resolve_path(Path::new("/../x")), PathBuf::from("/x"));
        assert!(resolve_path(Path::new("rel")).is_absolute());
    }

    #[test]
    fn within_and_relative() {
        let root = Path::new("/ws");
        assert!(is_within(root, Path::new("/ws")));
        assert!(is_within(root, Path::new("/ws/src/a.ts")));
        assert!(!is_within(root, Path::new("/ws2/a.ts")));
        assert!(!is_within(root, Path::new("/ws/../other")));
        assert_eq!(
            relative_to(root, Path::new("/ws/src/a.ts")),
            Some(PathBuf::from("src/a.ts"))
        );
        assert_eq!(relative_to(root, Path::new("/ws")), Some(PathBuf::new()));
        assert_eq!(relative_to(root, Path::new("/x")), None);
    }

    #[test]
    fn skip_rules() {
        for n in ["node_modules", "dist", "dist-electron", "dist-renderer", ".git", ".vscode"] {
            assert!(should_skip_directory(n), "{n}");
        }
        assert!(!should_skip_directory("src"));
        assert!(should_skip_entry_in_tree("_temp_run_1"));
        assert!(!should_skip_directory("_temp_run_1"));
    }

    #[test]
    fn locale_compare_is_case_insensitive_first() {
        assert_eq!(locale_compare("apple", "Banana"), Ordering::Less);
        assert_eq!(locale_compare("a", "A"), Ordering::Less);
        assert_eq!(locale_compare("A", "a"), Ordering::Greater);
        assert_eq!(locale_compare("README.md", "app.ts"), Ordering::Greater);
        assert_eq!(locale_compare("same", "same"), Ordering::Equal);
        assert_eq!(locale_compare("file10", "file2"), Ordering::Less);
    }

    #[test]
    fn sort_and_merge_agree_with_compare_paths() {
        let root = Path::new("/ws");
        let mut paths: Vec<PathBuf> = ["/ws/b/x", "/ws/A.txt", "/ws/a.txt", "/ws/B/y", "/ws/b.txt", "/ws/a/z"]
            .iter()
            .map(PathBuf::from)
            .collect();
        let mut expected = paths.clone();
        expected.sort_by(|a, b| compare_paths(a, b));
        sort_paths(&mut paths, root);
        assert_eq!(paths, expected);

        let (left, right) = paths.split_at(3);
        let merged = merge_sorted(right.to_vec(), left.to_vec());
        assert_eq!(merged, expected);
        let with_dup = merge_sorted(expected.clone(), vec![expected[2].clone()]);
        assert_eq!(with_dup, expected);
    }
}
