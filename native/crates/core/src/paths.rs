//! Canonical path, sort and skip helpers shared by every crate.
//!
//! These used to be copied into `vsm-fs_index`, `vsm-search`, `vsm-tasks`, `vsm-git` and
//! `vsm-db`, and the copies had drifted apart. Everything lives here now.
//!
//! # Conventions
//!
//! * All path math is **lexical**: no symlink resolution and no file system access (apart
//!   from [`resolve_path`], which reads the current directory for relative input). This
//!   mirrors Node's `path.resolve` / `path.relative`.
//! * Functions ending in `_with` take an explicit `case_insensitive` flag so callers and tests
//!   can pick the platform semantics. The unsuffixed wrappers use [`CASE_INSENSITIVE_FS`],
//!   which is `cfg!(windows)`: every pre-existing caller only folded case on Windows, and that
//!   is preserved (macOS volumes are *usually* case-insensitive but not always, so it is not
//!   assumed). Use the `_with` variants if a caller ever needs different semantics.
//! * Case folding is per path component (`str::to_lowercase`), so `/a/b` is never "within"
//!   `/a/bc`, with or without folding.
//!
//! # Skip list ([`should_skip_directory`])
//!
//! A directory is skipped by the file index, the search walker and the file watcher when its
//! name is one of `node_modules`, `dist`, `dist-electron`, `dist-renderer`, `.git`, or when it
//! starts with `.` (so every dot-directory). The file tree additionally hides `_temp_run*`
//! entries ([`should_skip_entry_in_tree`]). This matches `shouldSkipDirectory` in the
//! TypeScript `workspaceFileIndex`.
//!
//! # Sort order ([`locale_compare`])
//!
//! An approximation of `String.prototype.localeCompare` (ICU root collation) for file names:
//! whitespace < punctuation (ICU order, so `_` < `-` < `,` < `;` < `:` < `!` < `?` < `.` ...)
//! < digits < letters. Letters compare case-insensitively first, lowercase before uppercase on
//! ties, and the result is a total order. Digits are *not* numerically collated (`file10` <
//! `file2`), as with the default `localeCompare`.

use std::cmp::Ordering;
use std::path::{Component, Path, PathBuf};

/// Whether paths compare case-insensitively on this platform (`cfg!(windows)`).
pub const CASE_INSENSITIVE_FS: bool = cfg!(windows);

// ---------------------------------------------------------------------------------------
// Skip lists
// ---------------------------------------------------------------------------------------

/// Directory names that are never indexed, searched or watched: `node_modules`, build output
/// folders (`dist`, `dist-electron`, `dist-renderer`) and every dot-directory (including
/// `.git`).
pub fn should_skip_directory(name: &str) -> bool {
    matches!(
        name,
        "node_modules" | "dist" | "dist-electron" | "dist-renderer" | ".git"
    ) || name.starts_with('.')
}

/// Whether `name` is hidden from the file tree listing (superset of
/// [`should_skip_directory`] for tree view: also `_temp_run*`). Applies to both files and
/// directories, as in the TypeScript `readDir`.
pub fn should_skip_entry_in_tree(name: &str) -> bool {
    should_skip_directory(name) || name.starts_with("_temp_run")
}

// ---------------------------------------------------------------------------------------
// Sorting
// ---------------------------------------------------------------------------------------

/// ICU root collation order of the ASCII punctuation we care about.
const PUNCTUATION_ORDER: &str = "_-,;:!?.'\"()[]{}@*/\\&#%`^+<=>|~$";

/// Primary collation key of an already lower-cased character:
/// whitespace < punctuation/symbols < digits < letters.
fn primary_key(c: char) -> (u8, u32) {
    if c.is_whitespace() {
        (0, c as u32)
    } else if c.is_alphabetic() {
        (3, c as u32)
    } else if c.is_numeric() {
        (2, c as u32)
    } else if let Some(position) = PUNCTUATION_ORDER.find(c) {
        (1, position as u32)
    } else {
        (1, 1000 + c as u32)
    }
}

/// Approximation of JavaScript's `String.prototype.localeCompare` for file names; see the
/// module documentation for the exact order.
///
/// Allocation-free and lazy: the comparison stops at the first difference.
pub fn locale_compare(a: &str, b: &str) -> Ordering {
    let primary = a
        .chars()
        .flat_map(char::to_lowercase)
        .map(primary_key)
        .cmp(b.chars().flat_map(char::to_lowercase).map(primary_key));
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

/// Sort `paths` in [`compare_paths`] order. Keys are computed once per path and relative to
/// `root`, so the shared prefix is never compared and nothing is allocated per comparison
/// (O(n log n) cheap comparisons).
pub fn sort_paths(paths: &mut [PathBuf], root: &Path) {
    paths.sort_by_cached_key(|p| {
        let shown = p.strip_prefix(root).unwrap_or(p);
        LocaleKey(shown.to_string_lossy().into_owned())
    });
}

/// Merge two slices that are each sorted in [`compare_paths`] order into one sorted,
/// de-duplicated vector in O(n + m).
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

// ---------------------------------------------------------------------------------------
// Lexical normalisation
// ---------------------------------------------------------------------------------------

/// Resolve `.` and `..` without touching the file system.
///
/// * `..` removes the previous normal component.
/// * `..` at a root (`/..`, `C:\..`) stays at the root, like Node.
/// * Leading `..` of a relative path are kept (`../../a` stays `../../a`).
pub fn normalize_lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    // Number of `Normal` components currently on `out` (the only ones `..` may remove).
    let mut removable = 0usize;
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if removable > 0 {
                    out.pop();
                    removable -= 1;
                } else if !out.has_root() {
                    out.push("..");
                }
            }
            Component::Normal(name) => {
                out.push(name);
                removable += 1;
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Make `path` absolute (relative to the current directory) and normalise it lexically, like
/// Node's `path.resolve`.
pub fn resolve_path(path: &Path) -> PathBuf {
    if path.is_absolute() {
        normalize_lexical(path)
    } else {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        normalize_lexical(&cwd.join(path))
    }
}

// ---------------------------------------------------------------------------------------
// Containment / relative paths
// ---------------------------------------------------------------------------------------

/// Whether two path components (or file names) are equal, folding case when asked.
pub fn names_eq(a: &std::ffi::OsStr, b: &std::ffi::OsStr, case_insensitive: bool) -> bool {
    a == b
        || (case_insensitive
            && a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase())
}

/// The part of `target` below `base` (empty if equal), or `None` if `target` is not inside
/// `base`. Component-wise and purely lexical: both paths must already be normalised (see
/// [`normalize_lexical`] / [`resolve_path`]). The remainder keeps the spelling of `target`.
pub fn relative_to_with(base: &Path, target: &Path, case_insensitive: bool) -> Option<PathBuf> {
    if !case_insensitive {
        return target.strip_prefix(base).ok().map(Path::to_path_buf);
    }
    let mut target_components = target.components();
    for base_component in base.components() {
        let component = target_components.next()?;
        if !names_eq(component.as_os_str(), base_component.as_os_str(), true) {
            return None;
        }
    }
    Some(target_components.as_path().to_path_buf())
}

/// [`relative_to_with`] using the platform's case sensitivity ([`CASE_INSENSITIVE_FS`]).
pub fn relative_to(base: &Path, target: &Path) -> Option<PathBuf> {
    relative_to_with(base, target, CASE_INSENSITIVE_FS)
}

/// `file` relative to `root`, where `file` may itself be relative to `root`. Both sides are
/// normalised first (`..` resolved), so `c/../d.txt` is `d.txt` and an escape such as
/// `../x` yields `None`.
pub fn relative_to_root_with(root: &Path, file: &Path, case_insensitive: bool) -> Option<PathBuf> {
    let root = normalize_lexical(root);
    let file = normalize_lexical(&if file.is_absolute() {
        file.to_path_buf()
    } else {
        root.join(file)
    });
    relative_to_with(&root, &file, case_insensitive)
}

/// [`relative_to_root_with`] using the platform's case sensitivity.
pub fn relative_to_root(root: &Path, file: &Path) -> Option<PathBuf> {
    relative_to_root_with(root, file, CASE_INSENSITIVE_FS)
}

/// `true` if `target` equals `parent` or lies below it (component-wise, lexical). Both paths
/// must already be normalised.
pub fn is_within_with(parent: &Path, target: &Path, case_insensitive: bool) -> bool {
    relative_to_with(parent, target, case_insensitive).is_some()
}

/// [`is_within_with`] using the platform's case sensitivity.
pub fn is_within(parent: &Path, target: &Path) -> bool {
    is_within_with(parent, target, CASE_INSENSITIVE_FS)
}

/// `true` if `file_path` (string form, any separator style) equals `root` or lies below it.
///
/// Plain string prefix, but only at a path-separator boundary: `C:\workspace-old\a` is not
/// under `C:\workspace`. Returns the remainder with leading separators removed. An empty
/// `root` never matches. Case-sensitive; see [`strip_root_prefix_with`].
pub fn strip_root_prefix<'a>(path: &'a str, root: &str) -> Option<&'a str> {
    strip_root_prefix_with(path, root, false)
}

/// [`strip_root_prefix`] with optional per-character case folding. Slicing uses the byte
/// length that was actually consumed from `path`, so folding never mis-slices.
pub fn strip_root_prefix_with<'a>(
    path: &'a str,
    root: &str,
    case_insensitive: bool,
) -> Option<&'a str> {
    if root.is_empty() {
        return None;
    }
    let consumed = if case_insensitive {
        let mut path_chars = path.char_indices();
        let mut end = 0;
        for expected in root.chars() {
            let (index, actual) = path_chars.next()?;
            if actual != expected && !actual.to_lowercase().eq(expected.to_lowercase()) {
                return None;
            }
            end = index + actual.len_utf8();
        }
        end
    } else {
        if !path.starts_with(root) {
            return None;
        }
        root.len()
    };
    let rest = &path[consumed..];
    let boundary =
        rest.is_empty() || root.ends_with(['/', '\\']) || rest.starts_with(['/', '\\']);
    boundary.then(|| rest.trim_start_matches(['/', '\\']))
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- skip lists -------------------------------------------------------------------

    #[test]
    fn skip_rules() {
        for n in ["node_modules", "dist", "dist-electron", "dist-renderer", ".git", ".vscode"] {
            assert!(should_skip_directory(n), "{n}");
        }
        assert!(!should_skip_directory("src"));
        assert!(!should_skip_directory("distribution"));
        assert!(should_skip_entry_in_tree("_temp_run_1"));
        assert!(!should_skip_directory("_temp_run_1"));
    }

    // --- sorting ----------------------------------------------------------------------

    #[test]
    fn locale_compare_is_case_insensitive_first() {
        assert_eq!(locale_compare("apple", "Banana"), Ordering::Less);
        assert_eq!(locale_compare("a", "A"), Ordering::Less);
        assert_eq!(locale_compare("A", "a"), Ordering::Greater);
        assert_eq!(locale_compare("README.md", "app.ts"), Ordering::Greater);
        assert_eq!(locale_compare("same", "same"), Ordering::Equal);
        assert_eq!(locale_compare("file10", "file2"), Ordering::Less);
        assert_eq!(locale_compare("file2", "file10"), Ordering::Greater);
    }

    #[test]
    fn locale_compare_classes_and_icu_punctuation() {
        assert_eq!(locale_compare("a", "B"), Ordering::Less);
        assert_eq!(locale_compare("src/a", "src/b"), Ordering::Less);
        // punctuation < digits < letters
        assert_eq!(locale_compare("_x", "1"), Ordering::Less);
        assert_eq!(locale_compare("1", "a"), Ordering::Less);
        assert_eq!(locale_compare(".env", "a"), Ordering::Less);
        // ICU order: `_` before `-`
        assert_eq!(locale_compare("a_b.ts", "a-b.ts"), Ordering::Less);
        assert_eq!(locale_compare("a-b.ts", "a_b.ts"), Ordering::Greater);
        // whitespace first
        assert_eq!(locale_compare("a b", "a_b"), Ordering::Less);
    }

    #[test]
    fn locale_compare_is_a_total_order_on_a_tricky_list() {
        let names = ["a_b", "a-b", "A.ts", "a.ts", "10.ts", "9.ts", ".env", "ก.ts", "ข.ts", "Ab", "ab"];
        for x in names {
            assert_eq!(locale_compare(x, x), Ordering::Equal);
            for y in names {
                assert_eq!(locale_compare(x, y), locale_compare(y, x).reverse(), "{x} {y}");
                for z in names {
                    if locale_compare(x, y).is_le() && locale_compare(y, z).is_le() {
                        assert!(locale_compare(x, z).is_le(), "{x} {y} {z}");
                    }
                }
            }
        }
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

    // --- normalisation ----------------------------------------------------------------

    #[test]
    fn normalize_resolves_dot_segments() {
        assert_eq!(normalize_lexical(Path::new("/a/b/../c/./d")), PathBuf::from("/a/c/d"));
        assert_eq!(normalize_lexical(Path::new("a/../../b")), PathBuf::from("../b"));
        assert_eq!(normalize_lexical(Path::new("../../a")), PathBuf::from("../../a"));
        assert_eq!(normalize_lexical(Path::new("./a/./b/")), PathBuf::from("a/b"));
        assert_eq!(normalize_lexical(Path::new("")), PathBuf::new());
    }

    #[test]
    fn parent_of_root_stays_at_root() {
        assert_eq!(normalize_lexical(Path::new("/../x")), PathBuf::from("/x"));
        assert_eq!(normalize_lexical(Path::new("/..")), PathBuf::from("/"));
    }

    #[test]
    fn resolve_removes_dot_segments() {
        assert_eq!(resolve_path(Path::new("/a/b/../c/./d")), PathBuf::from("/a/c/d"));
        assert_eq!(resolve_path(Path::new("/../x")), PathBuf::from("/x"));
        assert!(resolve_path(Path::new("rel")).is_absolute());
        assert!(resolve_path(Path::new("../rel")).is_absolute());
    }

    // --- containment ------------------------------------------------------------------

    #[test]
    fn within_and_relative() {
        let root = Path::new("/ws");
        assert!(is_within(root, Path::new("/ws")));
        assert!(is_within(root, Path::new("/ws/src/a.ts")));
        assert!(!is_within(root, Path::new("/ws2/a.ts")));
        assert_eq!(
            relative_to(root, Path::new("/ws/src/a.ts")),
            Some(PathBuf::from("src/a.ts"))
        );
        assert_eq!(relative_to(root, Path::new("/ws")), Some(PathBuf::new()));
        assert_eq!(relative_to(root, Path::new("/x")), None);
    }

    #[test]
    fn containment_can_ignore_case() {
        let parent = Path::new("/Work/Space");
        assert!(!is_within_with(parent, Path::new("/work/space/a"), false));
        assert!(is_within_with(parent, Path::new("/work/space/a"), true));
        assert!(is_within_with(parent, Path::new("/work/space"), true));
        assert!(!is_within_with(parent, Path::new("/work/spaceship"), true));
        assert!(!is_within_with(parent, Path::new("/work"), true));
    }

    #[test]
    fn relative_to_keeps_target_spelling_when_folding() {
        let base = Path::new("/Work/Repo");
        assert_eq!(
            relative_to_with(base, Path::new("/work/repo/Src/A.rs"), true),
            Some(PathBuf::from("Src/A.rs"))
        );
        assert_eq!(relative_to_with(base, Path::new("/work/repo/Src/A.rs"), false), None);
        assert_eq!(relative_to_with(base, Path::new("/work/other/a.rs"), true), None);
    }

    #[test]
    fn relative_to_root_resolves_relative_files() {
        let root = Path::new("/a/b");
        assert_eq!(relative_to_root(root, Path::new("/a/b/c/d.txt")), Some(PathBuf::from("c/d.txt")));
        assert_eq!(relative_to_root(root, Path::new("c/../d.txt")), Some(PathBuf::from("d.txt")));
        assert_eq!(relative_to_root(root, Path::new("/a/x.txt")), None);
        assert_eq!(relative_to_root(root, Path::new("../x.txt")), None);
        assert_eq!(relative_to_root(root, Path::new("/a/b/c/../../x")), None);
        assert_eq!(relative_to_root(root, Path::new("/a/b")), Some(PathBuf::new()));
    }

    #[test]
    fn relative_to_root_is_component_wise() {
        // "/a/bc" is not inside "/a/b"
        assert_eq!(relative_to_root(Path::new("/a/b"), Path::new("/a/bc/x")), None);
        assert_eq!(relative_to_root_with(Path::new("/a/b"), Path::new("/a/BC/x"), true), None);
    }

    #[test]
    fn relative_to_root_folds_case_when_asked() {
        let root = Path::new("/Work/Repo");
        assert_eq!(
            relative_to_root_with(root, Path::new("/work/repo/Src/A.rs"), true),
            Some(PathBuf::from("Src/A.rs"))
        );
        assert_eq!(relative_to_root_with(root, Path::new("/Work/Other/a.rs"), true), None);
        assert_eq!(relative_to_root_with(root, Path::new("/work/repo/a.rs"), false), None);
    }

    #[cfg(windows)]
    #[test]
    fn native_wrappers_fold_case_on_windows() {
        let root = Path::new("C:\\Work\\Repo");
        assert_eq!(
            relative_to_root(root, Path::new("c:\\work\\repo\\Src\\A.rs")),
            Some(PathBuf::from("Src\\A.rs"))
        );
        assert!(is_within(root, Path::new("c:\\WORK\\repo\\x")));
    }

    #[test]
    fn names_eq_folds_only_when_asked() {
        use std::ffi::OsStr;
        assert!(names_eq(OsStr::new("Sub"), OsStr::new("sub"), true));
        assert!(!names_eq(OsStr::new("Sub"), OsStr::new("sub"), false));
        assert!(names_eq(OsStr::new("sub"), OsStr::new("sub"), false));
    }

    // --- strip_root_prefix ------------------------------------------------------------

    #[test]
    fn strip_root_prefix_respects_separator_boundary() {
        assert_eq!(strip_root_prefix("C:\\workspace\\src\\a.ts", "C:\\workspace"), Some("src\\a.ts"));
        assert_eq!(strip_root_prefix("C:/workspace/src", "C:/workspace/"), Some("src"));
        assert_eq!(strip_root_prefix("C:\\workspace", "C:\\workspace"), Some(""));
        assert_eq!(strip_root_prefix("C:\\workspace-old\\a.ts", "C:\\workspace"), None);
        assert_eq!(strip_root_prefix("/a/b", ""), None);
        assert_eq!(strip_root_prefix("C:\\Workspace\\a", "C:\\workspace"), None);
    }

    #[test]
    fn strip_root_prefix_can_fold_case() {
        assert_eq!(strip_root_prefix_with("C:\\Workspace\\a", "c:\\workspace", true), Some("a"));
        assert_eq!(strip_root_prefix_with("C:\\Workspace-old\\a", "c:\\workspace", true), None);
        assert_eq!(strip_root_prefix_with("C:\\Work", "c:\\workspace", true), None);
        // Multi-byte characters: slicing must use the consumed byte length of `path`.
        assert_eq!(strip_root_prefix_with("/Ünï/x", "/ünï", true), Some("x"));
    }
}
