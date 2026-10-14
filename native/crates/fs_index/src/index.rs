//! Cached workspace file index (port of `src/main/services/fileIndex.ts`).

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use ignore::WalkBuilder;

use crate::error::{FsIndexError, FsIndexResult};
use crate::ignore_rules::{lock, IgnoreRules};
use crate::paths::{
    compare_paths, is_within_resolved, merge_sorted, relative_to, resolve_path, should_skip_directory,
    sort_paths,
};
use crate::watcher::{self, WatcherHandle};

/// Notification delivered to index subscribers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IndexEvent {
    /// The set of indexed files changed (file added/removed/renamed, or the
    /// index was invalidated and will be rebuilt on the next
    /// [`FileIndex::get_files`]).
    FilesChanged,
    /// The content of an indexed file changed on disk.
    FileModified(PathBuf),
}

/// Tuning knobs for a [`FileIndex`].
#[derive(Debug, Clone)]
pub struct IndexConfig {
    /// Watch the workspace for changes (default `true`). Without a watcher the
    /// index only changes through explicit calls.
    pub watch: bool,
    /// How long the watcher waits for file-system events to settle before
    /// delivering them (default 200 ms).
    pub debounce: Duration,
}

impl Default for IndexConfig {
    fn default() -> Self {
        Self {
            watch: true,
            debounce: Duration::from_millis(200),
        }
    }
}

type Listener = Arc<dyn Fn(&IndexEvent) + Send + Sync>;

pub(crate) struct State {
    pub(crate) root: Option<PathBuf>,
    pub(crate) files: Arc<Vec<PathBuf>>,
    pub(crate) dirty: bool,
    /// Bumped by every change to the file list or its validity; lets a running
    /// build detect that its result is already stale.
    pub(crate) version: u64,
    pub(crate) rules: Option<Arc<IgnoreRules>>,
    watcher: Option<WatcherHandle>,
}

pub(crate) struct Inner {
    pub(crate) config: IndexConfig,
    pub(crate) state: Mutex<State>,
    build_lock: Mutex<()>,
    listeners: Mutex<Vec<(u64, Listener)>>,
    next_listener_id: AtomicU64,
}

/// Sorted list of every file in the workspace, cached until invalidated and
/// updated incrementally.
///
/// What is indexed: regular files below the root, skipping `node_modules`,
/// `dist`, `dist-electron`, `dist-renderer` and dot-directories, and anything
/// ignored by `.gitignore` files (and `.git/info/exclude`). Symlinks are not
/// followed or listed. Dot-*files* such as `.env` are indexed.
///
/// `FileIndex` is a cheap handle: clones share the same index.
#[derive(Clone)]
pub struct FileIndex {
    pub(crate) inner: Arc<Inner>,
}

/// Keeps a listener registered; dropping it unsubscribes.
#[must_use = "dropping a Subscription unsubscribes the listener"]
pub struct Subscription {
    inner: Weak<Inner>,
    id: u64,
}

impl Drop for Subscription {
    fn drop(&mut self) {
        if let Some(inner) = self.inner.upgrade() {
            lock(&inner.listeners).retain(|(id, _)| *id != self.id);
        }
    }
}

impl Default for FileIndex {
    fn default() -> Self {
        Self::new()
    }
}

impl FileIndex {
    /// An index with the default configuration and no root.
    pub fn new() -> Self {
        Self::with_config(IndexConfig::default())
    }

    /// An index with an explicit configuration.
    pub fn with_config(config: IndexConfig) -> Self {
        Self {
            inner: Arc::new(Inner {
                config,
                state: Mutex::new(State {
                    root: None,
                    files: Arc::new(Vec::new()),
                    dirty: true,
                    version: 0,
                    rules: None,
                    watcher: None,
                }),
                build_lock: Mutex::new(()),
                listeners: Mutex::new(Vec::new()),
                next_listener_id: AtomicU64::new(1),
            }),
        }
    }

    pub(crate) fn from_inner(inner: Arc<Inner>) -> Self {
        Self { inner }
    }

    /// Register `listener`; it runs on whichever thread caused the change
    /// (the caller for explicit mutations, the watcher thread otherwise) and
    /// must not block for long.
    pub fn subscribe(&self, listener: impl Fn(&IndexEvent) + Send + Sync + 'static) -> Subscription {
        let id = self.inner.next_listener_id.fetch_add(1, Ordering::Relaxed);
        lock(&self.inner.listeners).push((id, Arc::new(listener)));
        Subscription {
            inner: Arc::downgrade(&self.inner),
            id,
        }
    }

    /// Like [`FileIndex::subscribe`] but delivers events on a channel.
    pub fn subscribe_channel(&self) -> (Subscription, Receiver<IndexEvent>) {
        let (tx, rx) = mpsc::channel();
        let tx = Mutex::new(tx);
        let subscription = self.subscribe(move |event| {
            let _ = lock(&tx).send(event.clone());
        });
        (subscription, rx)
    }

    pub(crate) fn notify(&self, event: &IndexEvent) {
        let listeners: Vec<Listener> = lock(&self.inner.listeners)
            .iter()
            .map(|(_, l)| Arc::clone(l))
            .collect();
        for listener in listeners {
            listener(event);
        }
    }

    /// The current root, if any.
    pub fn root_path(&self) -> Option<PathBuf> {
        lock(&self.inner.state).root.clone()
    }

    /// Whether the next [`FileIndex::get_files`] has to (re)build the index.
    pub fn is_dirty(&self) -> bool {
        lock(&self.inner.state).dirty
    }

    /// Point the index at `root_path` (`None` clears it). Drops the cache and
    /// restarts the watcher, which registers watches synchronously - on large
    /// trees call this from a background thread. If the watcher cannot start
    /// the index still works but is never updated automatically.
    pub fn set_root_path(&self, root_path: Option<&Path>) {
        let root = root_path.map(resolve_path);
        let rules = root.clone().map(|r| Arc::new(IgnoreRules::new(r)));

        // Stop the old watcher outside the state lock (it joins its thread).
        let old_watcher = {
            let mut state = lock(&self.inner.state);
            state.root = root.clone();
            state.files = Arc::new(Vec::new());
            state.dirty = true;
            state.version += 1;
            state.rules = rules.clone();
            state.watcher.take()
        };
        drop(old_watcher);

        if let (Some(root), Some(rules), true) = (root, rules, self.inner.config.watch) {
            let started = watcher::start(
                Arc::downgrade(&self.inner),
                root.clone(),
                rules,
                self.inner.config.debounce,
            );
            let mut state = lock(&self.inner.state);
            // Ignore a watcher for a root that was replaced meanwhile.
            let installed = if state.root.as_deref() == Some(root.as_path()) {
                // A failed watcher leaves the index dirty, i.e. `invalidate()`.
                started.ok()
            } else {
                None
            };
            let replaced = std::mem::replace(&mut state.watcher, installed);
            drop(state);
            // Dropping a handle joins its thread: never do that under the lock.
            drop(replaced);
        }
    }

    /// Mark the cache stale if `target_path` is inside the root (or when no
    /// path is given). The next [`FileIndex::get_files`] rebuilds it. Does not
    /// notify subscribers.
    pub fn invalidate(&self, target_path: Option<&Path>) {
        let mut state = lock(&self.inner.state);
        let applies = match (&state.root, target_path) {
            (None, _) | (_, None) => true,
            (Some(root), Some(target)) => is_within_resolved(root, &resolve_path(target)),
        };
        if applies {
            state.dirty = true;
            state.version += 1;
        }
    }

    /// Add one file to the index (no-op if already present, outside the root,
    /// or excluded by the skip list / `.gitignore`). Notifies
    /// [`IndexEvent::FilesChanged`] if the index changed.
    pub fn add_file(&self, file_path: &Path) {
        if self.add_file_silent(file_path) {
            self.notify(&IndexEvent::FilesChanged);
        }
    }

    /// Remove `target_path` and, if it is a directory, everything below it.
    /// Notifies if anything was removed.
    pub fn remove_path(&self, target_path: &Path) {
        if self.remove_path_silent(target_path) {
            self.notify(&IndexEvent::FilesChanged);
        }
    }

    /// Re-map `old_path` (file or directory) to `new_path` in the index.
    /// Entries that end up outside the root or in excluded locations are
    /// dropped. Notifies if the index changed.
    pub fn rename_path(&self, old_path: &Path, new_path: &Path) {
        if self.rename_path_silent(old_path, new_path) {
            self.notify(&IndexEvent::FilesChanged);
        }
    }

    pub(crate) fn add_file_silent(&self, file_path: &Path) -> bool {
        let resolved = resolve_path(file_path);
        let mut state = lock(&self.inner.state);
        let Some(rules) = state.rules.clone() else {
            return false;
        };
        if !rules.should_track_file(&resolved) {
            return false;
        }
        match state.files.binary_search_by(|f| compare_paths(f, &resolved)) {
            Ok(_) => false,
            Err(position) => {
                Arc::make_mut(&mut state.files).insert(position, resolved);
                state.version += 1;
                true
            }
        }
    }

    pub(crate) fn remove_path_silent(&self, target_path: &Path) -> bool {
        let mut state = lock(&self.inner.state);
        let Some(root) = state.root.clone() else {
            return false;
        };
        let target = resolve_path(target_path);
        if !is_within_resolved(&root, &target) {
            return false;
        }
        let before = state.files.len();
        let kept: Vec<PathBuf> = state
            .files
            .iter()
            .filter(|f| !is_within_resolved(&target, f))
            .cloned()
            .collect();
        if kept.len() == before {
            return false;
        }
        state.files = Arc::new(kept);
        state.version += 1;
        true
    }

    pub(crate) fn rename_path_silent(&self, old_path: &Path, new_path: &Path) -> bool {
        let old = resolve_path(old_path);
        let new = resolve_path(new_path);
        let mut state = lock(&self.inner.state);
        let (Some(root), Some(rules)) = (state.root.clone(), state.rules.clone()) else {
            return false;
        };
        if !is_within_resolved(&root, &old) {
            return false;
        }

        // Split into untouched entries (still sorted) and the moved ones.
        let mut kept = Vec::with_capacity(state.files.len());
        let mut moved = Vec::new();
        let mut affected = false;
        for file in state.files.iter() {
            match relative_to(&old, file) {
                None => kept.push(file.clone()),
                Some(rel) => {
                    affected = true;
                    let target = if rel.as_os_str().is_empty() {
                        new.clone()
                    } else {
                        new.join(rel)
                    };
                    if rules.should_track_file(&target) {
                        moved.push(target);
                    }
                }
            }
        }
        if !affected {
            return false;
        }
        state.files = Arc::new(insert_sorted(kept, moved, &root));
        state.version += 1;
        true
    }

    /// Replace everything below `dir` with `scanned` (used when a directory
    /// appears or moves). Returns whether the index changed.
    pub(crate) fn replace_subtree_silent(&self, dir: &Path, scanned: Vec<PathBuf>) -> bool {
        let mut state = lock(&self.inner.state);
        let Some(root) = state.root.clone() else {
            return false;
        };
        let kept: Vec<PathBuf> = state
            .files
            .iter()
            .filter(|f| !is_within_resolved(dir, f))
            .cloned()
            .collect();
        let next = insert_sorted(kept, scanned, &root);
        if next == *state.files {
            return false;
        }
        state.files = Arc::new(next);
        state.version += 1;
        true
    }

    /// All indexed files of `root_path`, sorted. Returns the cache when it is
    /// valid, otherwise (re)builds it; concurrent callers share one build.
    /// Switching to a different root resets the index and restarts the watcher.
    ///
    /// # Errors
    /// [`FsIndexError::Io`] if the root cannot be read.
    pub fn get_files(&self, root_path: &Path) -> FsIndexResult<Arc<Vec<PathBuf>>> {
        let root = resolve_path(root_path);
        if self.root_path().as_deref() != Some(root.as_path()) {
            self.set_root_path(Some(&root));
        }

        {
            let state = lock(&self.inner.state);
            if !state.dirty && state.root.as_deref() == Some(root.as_path()) {
                return Ok(Arc::clone(&state.files));
            }
        }

        let _build_guard = lock(&self.inner.build_lock);
        let (version, rules) = {
            let state = lock(&self.inner.state);
            if !state.dirty && state.root.as_deref() == Some(root.as_path()) {
                return Ok(Arc::clone(&state.files));
            }
            let rules = match (&state.root, &state.rules) {
                (Some(r), Some(rules)) if *r == root => Arc::clone(rules),
                _ => Arc::new(IgnoreRules::new(root.clone())),
            };
            (state.version, rules)
        };

        rules.clear(); // pick up edited .gitignore files
        let files = Arc::new(scan_files(&root, &root, &rules)?);

        let mut state = lock(&self.inner.state);
        if state.version == version && state.root.as_deref() == Some(root.as_path()) {
            state.files = Arc::clone(&files);
            state.dirty = false;
        }
        Ok(files)
    }
}

/// Add `extra` (unsorted, may contain duplicates) to the sorted `kept` list:
/// binary insertion for a handful of entries, a linear merge otherwise.
fn insert_sorted(mut kept: Vec<PathBuf>, mut extra: Vec<PathBuf>, root: &Path) -> Vec<PathBuf> {
    const BINARY_INSERT_LIMIT: usize = 16;
    if extra.len() <= BINARY_INSERT_LIMIT {
        for path in extra {
            if let Err(position) = kept.binary_search_by(|f| compare_paths(f, &path)) {
                kept.insert(position, path);
            }
        }
        return kept;
    }
    sort_paths(&mut extra, root);
    merge_sorted(kept, extra)
}

/// Walk `start` (inside `root`) and return all indexable files, sorted.
pub(crate) fn scan_files(
    root: &Path,
    start: &Path,
    rules: &Arc<IgnoreRules>,
) -> FsIndexResult<Vec<PathBuf>> {
    debug_assert!(is_within_resolved(root, start));
    // Fail loudly if the starting directory itself is unreadable.
    fs::read_dir(start).map_err(|e| FsIndexError::io(start, e))?;

    let filter_rules = Arc::clone(rules);
    let walker = WalkBuilder::new(start)
        .standard_filters(false) // we apply our own rules below
        .follow_links(false)
        .filter_entry(move |entry| {
            if entry.depth() == 0 {
                return true;
            }
            let is_dir = entry.file_type().is_some_and(|t| t.is_dir());
            if is_dir && should_skip_directory(&entry.file_name().to_string_lossy()) {
                return false;
            }
            !filter_rules.is_ignored(entry.path(), is_dir)
        })
        .build();

    let mut files: Vec<PathBuf> = walker
        .filter_map(Result::ok) // unreadable subdirectories are skipped
        .filter(|entry| entry.file_type().is_some_and(|t| t.is_file()))
        .map(ignore::DirEntry::into_path)
        .collect();
    sort_paths(&mut files, root);
    Ok(files)
}

/// Directories below `start` (inclusive) that the watcher should observe.
pub(crate) fn scan_dirs(start: &Path, rules: &Arc<IgnoreRules>) -> Vec<PathBuf> {
    let filter_rules = Arc::clone(rules);
    WalkBuilder::new(start)
        .standard_filters(false)
        .follow_links(false)
        .filter_entry(move |entry| {
            if entry.depth() == 0 {
                return true;
            }
            let is_dir = entry.file_type().is_some_and(|t| t.is_dir());
            if !is_dir {
                return true;
            }
            !should_skip_directory(&entry.file_name().to_string_lossy())
                && !filter_rules.is_ignored(entry.path(), true)
        })
        .build()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_some_and(|t| t.is_dir()))
        .map(ignore::DirEntry::into_path)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(files: &[PathBuf]) -> Vec<String> {
        files
            .iter()
            .map(|f| f.file_name().unwrap().to_string_lossy().into_owned())
            .collect()
    }

    fn sorted_names(files: &[PathBuf]) -> Vec<String> {
        let mut n = names(files);
        n.sort();
        n
    }

    fn fixture() -> tempfile::TempDir {
        let dir = tempfile::Builder::new()
            .prefix("vs-monitor-ide-index-")
            .tempdir()
            .unwrap();
        let root = dir.path();
        fs::create_dir(root.join("src")).unwrap();
        fs::create_dir(root.join("node_modules")).unwrap();
        fs::write(root.join("src").join("app.ts"), "export const app = true\n").unwrap();
        fs::write(root.join("README.md"), "# test\n").unwrap();
        fs::write(root.join("node_modules").join("skip.js"), "module.exports = {}\n").unwrap();
        dir
    }

    fn no_watch() -> FileIndex {
        FileIndex::with_config(IndexConfig {
            watch: false,
            ..IndexConfig::default()
        })
    }

    #[test]
    fn reuses_the_cached_index_until_a_workspace_change_is_invalidated() {
        let dir = fixture();
        let root = dir.path();
        let index = no_watch();

        let first = index.get_files(root).unwrap();
        assert_eq!(sorted_names(&first), ["README.md", "app.ts"]);

        fs::remove_file(root.join("README.md")).unwrap();
        let cached = index.get_files(root).unwrap();
        assert_eq!(sorted_names(&cached), ["README.md", "app.ts"]);

        index.invalidate(Some(&root.join("README.md")));
        let rebuilt = index.get_files(root).unwrap();
        assert_eq!(names(&rebuilt), ["app.ts"]);
    }

    #[test]
    fn applies_incremental_add_rename_and_remove_updates_without_rebuilding() {
        let dir = fixture();
        let root = dir.path();
        let index = no_watch();
        index.get_files(root).unwrap();
        let (_sub, rx) = index.subscribe_channel();

        let created = root.join("src").join("notes.ts");
        fs::write(&created, "export const notes = true\n").unwrap();
        index.add_file(&created);
        assert!(index.get_files(root).unwrap().contains(&created));
        assert_eq!(rx.try_recv().unwrap(), IndexEvent::FilesChanged);
        // Adding twice is a no-op and does not notify.
        index.add_file(&created);
        assert!(rx.try_recv().is_err());

        let renamed = root.join("src").join("notes-renamed.ts");
        fs::rename(&created, &renamed).unwrap();
        index.rename_path(&created, &renamed);
        let files = index.get_files(root).unwrap();
        assert!(files.contains(&renamed));
        assert!(!files.contains(&created));
        assert_eq!(rx.try_recv().unwrap(), IndexEvent::FilesChanged);

        fs::remove_file(&renamed).unwrap();
        index.remove_path(&renamed);
        assert!(!index.get_files(root).unwrap().contains(&renamed));
        assert!(!index.is_dirty(), "incremental updates must not invalidate");
    }

    #[test]
    fn renaming_a_directory_moves_every_file_below_it() {
        let dir = fixture();
        let root = dir.path();
        let index = no_watch();
        index.get_files(root).unwrap();

        let (old, new) = (root.join("src"), root.join("lib"));
        fs::rename(&old, &new).unwrap();
        index.rename_path(&old, &new);
        let files = index.get_files(root).unwrap();
        assert!(files.contains(&new.join("app.ts")));
        assert!(!files.contains(&old.join("app.ts")));

        // Renaming into a skipped directory removes the files from the index.
        let hidden = root.join(".hidden");
        fs::rename(&new, &hidden).unwrap();
        index.rename_path(&new, &hidden);
        assert_eq!(names(&index.get_files(root).unwrap()), ["README.md"]);

        index.remove_path(root);
        assert!(index.get_files(root).unwrap().is_empty());
    }

    #[test]
    fn respects_gitignore_skip_list_and_keeps_dotfiles() {
        let dir = fixture();
        let root = dir.path();
        fs::write(root.join(".gitignore"), "*.log\nbuild/\n").unwrap();
        fs::write(root.join(".env"), "A=1\n").unwrap();
        fs::write(root.join("debug.log"), "x").unwrap();
        fs::create_dir_all(root.join("build")).unwrap();
        fs::write(root.join("build").join("out.js"), "x").unwrap();
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::write(root.join(".git").join("HEAD"), "x").unwrap();
        fs::create_dir_all(root.join("dist")).unwrap();
        fs::write(root.join("dist").join("bundle.js"), "x").unwrap();

        let index = no_watch();
        let files = index.get_files(root).unwrap();
        assert_eq!(sorted_names(&files), [".env", ".gitignore", "README.md", "app.ts"]);

        // Incremental adds apply the same rules.
        for path in [
            root.join("debug.log"),
            root.join("build").join("out.js"),
            root.join("node_modules").join("skip.js"),
            root.join("dist").join("bundle.js"),
            root.join("..").join("outside.txt"),
        ] {
            index.add_file(&path);
        }
        assert_eq!(index.get_files(root).unwrap().len(), 4);
        let env2 = root.join("src").join(".env.local");
        fs::write(&env2, "x").unwrap();
        index.add_file(&env2);
        assert!(index.get_files(root).unwrap().contains(&env2));
    }

    #[test]
    fn files_are_sorted_in_locale_order_and_binary_insert_keeps_order() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        for f in ["b.txt", "A.txt", "c.txt"] {
            fs::write(root.join(f), "").unwrap();
        }
        let index = no_watch();
        assert_eq!(names(&index.get_files(root).unwrap()), ["A.txt", "b.txt", "c.txt"]);
        let new = root.join("B2.txt");
        fs::write(&new, "").unwrap();
        index.add_file(&new);
        assert_eq!(
            names(&index.get_files(root).unwrap()),
            ["A.txt", "b.txt", "B2.txt", "c.txt"]
        );
    }

    #[test]
    fn switching_root_resets_the_index_and_unreadable_root_errors() {
        let (a, b) = (fixture(), tempfile::tempdir().unwrap());
        fs::write(b.path().join("only.txt"), "").unwrap();
        let index = no_watch();
        assert_eq!(index.get_files(a.path()).unwrap().len(), 2);
        assert_eq!(names(&index.get_files(b.path()).unwrap()), ["only.txt"]);
        assert!(index.get_files(&b.path().join("missing")).is_err());

        index.set_root_path(None);
        assert!(index.root_path().is_none());
        index.add_file(&b.path().join("only.txt")); // ignored without a root
    }

    #[test]
    fn concurrent_callers_share_a_consistent_result() {
        let dir = fixture();
        let index = no_watch();
        let handles: Vec<_> = (0..4)
            .map(|_| {
                let index = index.clone();
                let root = dir.path().to_path_buf();
                std::thread::spawn(move || index.get_files(&root).unwrap().len())
            })
            .collect();
        for h in handles {
            assert_eq!(h.join().unwrap(), 2);
        }
    }

    #[test]
    fn unsubscribing_stops_notifications() {
        let dir = fixture();
        let index = no_watch();
        index.get_files(dir.path()).unwrap();
        let (sub, rx) = index.subscribe_channel();
        drop(sub);
        let f = dir.path().join("new.txt");
        fs::write(&f, "").unwrap();
        index.add_file(&f);
        assert!(rx.recv_timeout(Duration::from_millis(50)).is_err());
    }
}
