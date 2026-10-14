//! `.gitignore` evaluation for a workspace, built on the `ignore` crate.
//!
//! The full index build and the incremental watcher updates share this matcher
//! so both agree on what is indexed.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use ignore::gitignore::{Gitignore, GitignoreBuilder};
use ignore::Match;

use crate::paths::{relative_to, should_skip_directory};

pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Add an ignore file, keeping the valid lines of a partly malformed one
/// (`Error::Partial`). Returns whether any rules may have been added.
fn add_ignore_file(builder: &mut GitignoreBuilder, path: &Path) -> bool {
    match builder.add(path) {
        None => true,
        Some(ignore::Error::Partial(_)) => true,
        Some(_) => false, // unreadable
    }
}

/// Lazily loaded `.gitignore` matchers for the directories of one workspace.
#[derive(Debug)]
pub(crate) struct IgnoreRules {
    root: PathBuf,
    cache: Mutex<HashMap<PathBuf, Option<Arc<Gitignore>>>>,
}

impl IgnoreRules {
    pub(crate) fn new(root: PathBuf) -> Self {
        Self {
            root,
            cache: Mutex::new(HashMap::new()),
        }
    }

    /// Forget all loaded ignore files (call when one changed).
    pub(crate) fn clear(&self) {
        lock(&self.cache).clear();
    }

    /// Whether `path` is a `.gitignore` file.
    pub(crate) fn is_gitignore_file(&self, path: &Path) -> bool {
        path.file_name().is_some_and(|n| n == ".gitignore")
    }

    /// Whether `path` is the workspace's `.git/info/exclude`.
    pub(crate) fn is_exclude_file(&self, path: &Path) -> bool {
        path == self.exclude_path()
    }

    pub(crate) fn exclude_path(&self) -> PathBuf {
        self.root.join(".git").join("info").join("exclude")
    }

    fn matcher_for(&self, dir: &Path) -> Option<Arc<Gitignore>> {
        let mut cache = lock(&self.cache);
        if let Some(found) = cache.get(dir) {
            return found.clone();
        }
        let mut builder = GitignoreBuilder::new(dir);
        // Match like the platform's file system does.
        let _ = builder.case_insensitive(cfg!(any(windows, target_os = "macos")));
        let mut any = false;
        let gitignore = dir.join(".gitignore");
        if gitignore.is_file() {
            any |= add_ignore_file(&mut builder, &gitignore);
        }
        if dir == self.root {
            let exclude = self.exclude_path();
            if exclude.is_file() {
                any |= add_ignore_file(&mut builder, &exclude);
            }
        }
        let matcher = if any {
            builder.build().ok().map(Arc::new)
        } else {
            None
        };
        cache.insert(dir.to_path_buf(), matcher.clone());
        matcher
    }

    /// Whether the ignore files from the workspace root down to `path`'s parent
    /// ignore `path` itself (ancestors are not examined; see
    /// [`IgnoreRules::is_ignored_with_ancestors`]).
    pub(crate) fn is_ignored(&self, path: &Path, is_dir: bool) -> bool {
        let Some(relative) = relative_to(&self.root, path) else {
            return false;
        };
        let depth = relative.components().count();
        if depth == 0 {
            return false;
        }
        // Directories whose ignore files apply: root, root/a, ... parent of path.
        let mut dirs = Vec::with_capacity(depth);
        let mut dir = self.root.clone();
        dirs.push(dir.clone());
        for component in relative.components().take(depth - 1) {
            dir.push(component.as_os_str());
            dirs.push(dir.clone());
        }
        // Deepest ignore file wins; a whitelist match stops the search.
        for dir in dirs.iter().rev() {
            if let Some(matcher) = self.matcher_for(dir) {
                match matcher.matched(path, is_dir) {
                    Match::Ignore(_) => return true,
                    Match::Whitelist(_) => return false,
                    Match::None => {}
                }
            }
        }
        false
    }

    /// Like [`IgnoreRules::is_ignored`] but also true if any parent directory
    /// below the root is ignored (git never descends into ignored folders).
    pub(crate) fn is_ignored_with_ancestors(&self, path: &Path, is_dir: bool) -> bool {
        let Some(relative) = relative_to(&self.root, path) else {
            return false;
        };
        let count = relative.components().count();
        let mut current = self.root.clone();
        for component in relative.components().take(count.saturating_sub(1)) {
            current.push(component.as_os_str());
            if self.is_ignored(&current, true) {
                return true;
            }
        }
        self.is_ignored(path, is_dir)
    }

    /// Whether a *file* at `path` belongs in the index: inside the root, no
    /// skipped directory (`node_modules`, dot-directories, ...) on the way,
    /// and not ignored by git.
    pub(crate) fn should_track_file(&self, path: &Path) -> bool {
        let Some(relative) = relative_to(&self.root, path) else {
            return false;
        };
        let components: Vec<_> = relative.components().collect();
        let Some((_file, parents)) = components.split_last() else {
            return false;
        };
        if parents
            .iter()
            .any(|c| should_skip_directory(&c.as_os_str().to_string_lossy()))
        {
            return false;
        }
        !self.is_ignored_with_ancestors(path, false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn honours_nested_gitignore_and_whitelists() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fs::create_dir_all(root.join("src/gen")).unwrap();
        fs::write(root.join(".gitignore"), "*.log\ntarget/\n").unwrap();
        fs::write(root.join("src/.gitignore"), "gen/\n!keep.log\n").unwrap();

        let rules = IgnoreRules::new(root.clone());
        assert!(rules.is_ignored(&root.join("a.log"), false));
        assert!(rules.is_ignored(&root.join("target"), true));
        assert!(!rules.is_ignored(&root.join("target"), false));
        assert!(rules.is_ignored(&root.join("src/gen"), true));
        assert!(!rules.is_ignored(&root.join("src/keep.log"), false));
        assert!(rules.is_ignored_with_ancestors(&root.join("src/gen/x.rs"), false));
        assert!(!rules.is_ignored_with_ancestors(&root.join("src/main.rs"), false));

        assert!(rules.should_track_file(&root.join("src/main.rs")));
        assert!(!rules.should_track_file(&root.join("node_modules/a.js")));
        assert!(!rules.should_track_file(&root.join(".hidden/a.js")));
        assert!(rules.should_track_file(&root.join(".env")));
        assert!(!rules.should_track_file(&root));
        assert!(!rules.should_track_file(Path::new("/elsewhere/a.rs")));
    }

    #[test]
    fn clear_picks_up_changed_ignore_files() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let rules = IgnoreRules::new(root.clone());
        assert!(!rules.is_ignored(&root.join("x.tmp"), false));
        fs::write(root.join(".gitignore"), "*.tmp\n").unwrap();
        assert!(!rules.is_ignored(&root.join("x.tmp"), false)); // cached
        rules.clear();
        assert!(rules.is_ignored(&root.join("x.tmp"), false));
        assert!(rules.is_gitignore_file(&root.join("sub/.gitignore")));
        assert!(rules.is_exclude_file(&root.join(".git/info/exclude")));
        assert!(!rules.is_exclude_file(&root.join(".git/info/other")));
    }

    #[test]
    fn malformed_lines_do_not_drop_the_valid_rules() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fs::write(root.join(".gitignore"), "*.log\nfoo[\n{a,b\nbuild/\n").unwrap();
        let rules = IgnoreRules::new(root.clone());
        assert!(rules.is_ignored(&root.join("a.log"), false));
        assert!(rules.is_ignored(&root.join("build"), true));
    }

    #[test]
    fn git_info_exclude_is_honoured() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fs::create_dir_all(root.join(".git/info")).unwrap();
        fs::write(root.join(".git/info/exclude"), "scratch.txt\n").unwrap();
        let rules = IgnoreRules::new(root.clone());
        assert!(rules.is_ignored(&root.join("scratch.txt"), false));
    }

    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn patterns_are_case_insensitive_on_case_insensitive_platforms() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fs::write(root.join(".gitignore"), "Build/\n").unwrap();
        let rules = IgnoreRules::new(root.clone());
        assert!(rules.is_ignored(&root.join("build"), true));
    }
}
