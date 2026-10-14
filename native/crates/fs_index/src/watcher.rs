//! File-system watcher feeding the [`FileIndex`].
//!
//! Port of `startWatcher` / `handleWatchEvent` from `fileIndex.ts`.
//!
//! * **Linux** uses a raw `notify::RecommendedWatcher` with one non-recursive
//!   watch per *indexed* directory (inotify has no recursive mode, and
//!   `node_modules`, `.git`, `target/` and other ignored folders must not eat
//!   the kernel's watch budget) plus a small debounce implemented here. The
//!   debouncer crate is not used because its per-root bookkeeping is
//!   quadratic in the number of watched directories.
//! * **Windows and macOS** use `notify-debouncer-full` on a recursive watch
//!   with `NoCache` (the default file-id cache walks the whole tree and opens
//!   every file).
//!
//! Either way events are only a *hint*: the worker thread re-reads the disk
//! state of each reported path and applies incremental updates to the index,
//! so rename stitching and event kinds are not needed for correctness.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Weak};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use notify::event::ModifyKind;
use notify::{Event, EventKind, RecursiveMode};

use crate::error::FsIndexError;
use crate::ignore_rules::IgnoreRules;
use crate::index::{scan_dirs, scan_files, FileIndex, IndexEvent, Inner};
use crate::paths::{is_within_resolved, relative_to, should_skip_directory};

const PER_DIRECTORY_WATCHES: bool = cfg!(any(target_os = "linux", target_os = "android"));

/// A burst larger than this on Windows is assumed to have overflowed the
/// kernel buffer (notify silently drops `ReadDirectoryChangesW` overflows).
const WINDOWS_OVERFLOW_SUSPECT: usize = 1000;
/// Upper bound for coalescing a continuous stream of events into one batch.
const MAX_COALESCE: Duration = Duration::from_secs(2);

#[cfg(any(target_os = "linux", target_os = "android"))]
mod backend {
    use super::*;
    use notify::Watcher;

    pub(super) type Backend = notify::RecommendedWatcher;

    /// Extra quiet time the worker waits for after the first event.
    pub(super) fn quiet_window(debounce: Duration) -> Duration {
        debounce
    }

    pub(super) fn create(tx: Sender<Msg>, _debounce: Duration) -> notify::Result<Backend> {
        notify::RecommendedWatcher::new(
            move |result: notify::Result<Event>| {
                let _ = tx.send(Msg::Events(Batch::from_notify(result)));
            },
            notify::Config::default(),
        )
    }

    pub(super) fn watch(backend: &mut Backend, path: &Path, recursive: bool) -> notify::Result<()> {
        let mode = if recursive {
            RecursiveMode::Recursive
        } else {
            RecursiveMode::NonRecursive
        };
        backend.watch(path, mode)
    }
}

#[cfg(not(any(target_os = "linux", target_os = "android")))]
mod backend {
    use super::*;
    use notify_debouncer_full::{new_debouncer_opt, DebounceEventResult, Debouncer, NoCache};

    pub(super) type Backend = Debouncer<notify::RecommendedWatcher, NoCache>;

    /// The debouncer already coalesces; only drain what is queued.
    pub(super) fn quiet_window(_debounce: Duration) -> Duration {
        Duration::ZERO
    }

    pub(super) fn create(tx: Sender<Msg>, debounce: Duration) -> notify::Result<Backend> {
        new_debouncer_opt::<_, notify::RecommendedWatcher, NoCache>(
            debounce,
            None,
            move |result: DebounceEventResult| {
                let batch = match result {
                    Ok(events) => {
                        let mut batch = Batch::default();
                        for event in &events {
                            batch.add_event(event);
                        }
                        batch
                    }
                    Err(_) => Batch::rescan(),
                };
                let _ = tx.send(Msg::Events(batch));
            },
            NoCache::new(),
            notify::Config::default(),
        )
    }

    pub(super) fn watch(backend: &mut Backend, path: &Path, recursive: bool) -> notify::Result<()> {
        let mode = if recursive {
            RecursiveMode::Recursive
        } else {
            RecursiveMode::NonRecursive
        };
        backend.watch(path, mode)
    }
}

use backend::Backend;

/// One path reported by the OS.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct Change {
    path: PathBuf,
    /// Create or rename events: the path may be new.
    created_like: bool,
}

/// Events collected over one debounce window.
#[derive(Debug, Default)]
struct Batch {
    changes: Vec<Change>,
    seen: HashSet<Change>,
    /// Number of raw events folded into this batch.
    events: usize,
    /// Events were lost; the index can no longer be trusted.
    rescan: bool,
}

impl Batch {
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    fn rescan() -> Self {
        Self {
            rescan: true,
            ..Self::default()
        }
    }

    #[cfg(any(target_os = "linux", target_os = "android"))]
    fn from_notify(result: notify::Result<Event>) -> Self {
        let mut batch = Self::default();
        match result {
            Ok(event) => batch.add_event(&event),
            Err(_) => batch.rescan = true,
        }
        batch
    }

    fn add_event(&mut self, event: &Event) {
        self.events += 1;
        if event.need_rescan() {
            self.rescan = true;
        }
        let created_like = match event.kind {
            // Reads and chmod/utimes are not content or structure changes.
            EventKind::Access(_) | EventKind::Modify(ModifyKind::Metadata(_)) => return,
            EventKind::Create(_) | EventKind::Modify(ModifyKind::Name(_)) => true,
            _ => false,
        };
        for path in &event.paths {
            let change = Change {
                path: path.clone(),
                created_like,
            };
            if self.seen.insert(change.clone()) {
                self.changes.push(change);
            }
        }
    }

    fn merge(&mut self, other: Batch) {
        self.events += other.events;
        self.rescan |= other.rescan;
        for change in other.changes {
            if self.seen.insert(change.clone()) {
                self.changes.push(change);
            }
        }
    }
}

enum Msg {
    Events(Batch),
    Stop,
}

/// Owns the watcher thread; dropping it stops watching.
pub(crate) struct WatcherHandle {
    tx: Sender<Msg>,
    thread: Option<JoinHandle<()>>,
}

impl Drop for WatcherHandle {
    fn drop(&mut self) {
        let _ = self.tx.send(Msg::Stop);
        if let Some(thread) = self.thread.take() {
            // The worker itself may hold the last reference to the index.
            if thread.thread().id() != thread::current().id() {
                let _ = thread.join();
            }
        }
    }
}

struct Context {
    root: PathBuf,
    /// Canonical form of `root`, for platforms that report resolved paths.
    canonical_root: Option<PathBuf>,
    rules: Arc<IgnoreRules>,
}

impl Context {
    /// Express an event path relative to the (non-canonical) workspace root.
    fn rebase(&self, path: &Path) -> Option<PathBuf> {
        if is_within_resolved(&self.root, path) {
            return Some(path.to_path_buf());
        }
        let canonical = self.canonical_root.as_deref()?;
        relative_to(canonical, path).map(|rel| self.root.join(rel))
    }
}

/// Register watches for `dir` and, where the platform needs it, every
/// indexed directory below it. Returns the number of directories that could
/// not be watched (e.g. inotify limit reached). Failing to watch `dir`
/// itself is an error.
fn watch_tree(backend: &mut Backend, dir: &Path, rules: &Arc<IgnoreRules>) -> notify::Result<usize> {
    if !PER_DIRECTORY_WATCHES {
        backend::watch(backend, dir, true)?;
        return Ok(0);
    }
    backend::watch(backend, dir, false)?;
    let mut failures = 0;
    for sub in scan_dirs(dir, rules).into_iter().filter(|d| d != dir) {
        if let Err(error) = backend::watch(backend, &sub, false) {
            failures += 1;
            log::warn!("cannot watch {}: {error}", sub.display());
        }
    }
    Ok(failures)
}

/// Watch `.git/info` (a skipped dot-directory) so `exclude` edits are seen.
fn watch_git_info(backend: &mut Backend, rules: &IgnoreRules) {
    if PER_DIRECTORY_WATCHES {
        if let Some(info) = rules.exclude_path().parent() {
            if info.is_dir() {
                let _ = backend::watch(backend, info, false);
            }
        }
    }
}

/// Start watching `root`; events are applied to the index behind `inner`.
pub(crate) fn start(
    inner: Weak<Inner>,
    root: PathBuf,
    rules: Arc<IgnoreRules>,
    debounce: Duration,
) -> Result<WatcherHandle, FsIndexError> {
    let watch_error = |source| FsIndexError::Watch {
        path: root.clone(),
        source,
    };
    let (tx, rx) = mpsc::channel::<Msg>();
    let mut backend = backend::create(tx.clone(), debounce).map_err(watch_error)?;

    let failures = watch_tree(&mut backend, &root, &rules).map_err(watch_error)?;
    if failures > 0 {
        log::warn!(
            "{failures} directories under {} are not watched; the file index may miss changes",
            root.display()
        );
    }
    watch_git_info(&mut backend, &rules);

    let ctx = Context {
        canonical_root: fs::canonicalize(&root).ok().filter(|c| *c != root),
        root: root.clone(),
        rules,
    };
    let quiet = backend::quiet_window(debounce);

    let thread = thread::Builder::new()
        .name("vsm-fs-watch".into())
        .spawn(move || {
            while let Ok(Msg::Events(mut batch)) = rx.recv() {
                // Coalesce follow-up events until the tree is quiet.
                let started = Instant::now();
                let mut stop = false;
                loop {
                    let next = if quiet.is_zero() {
                        rx.try_recv().ok()
                    } else {
                        rx.recv_timeout(quiet).ok()
                    };
                    match next {
                        Some(Msg::Events(more)) => {
                            batch.merge(more);
                            if started.elapsed() >= MAX_COALESCE {
                                break;
                            }
                        }
                        Some(Msg::Stop) => {
                            stop = true;
                            break;
                        }
                        None => break,
                    }
                }
                if stop {
                    break;
                }
                let Some(inner) = inner.upgrade() else { break };
                let index = FileIndex::from_inner(inner);
                process_batch(&index, &mut backend, &ctx, batch);
            }
            // `backend` drops here, which stops the OS watcher.
        })
        .map_err(|e| FsIndexError::io(root, e))?;

    Ok(WatcherHandle {
        tx,
        thread: Some(thread),
    })
}

fn process_batch(index: &FileIndex, backend: &mut Backend, ctx: &Context, batch: Batch) {
    let overflow_suspected = cfg!(windows) && batch.events > WINDOWS_OVERFLOW_SUSPECT;
    if batch.rescan || overflow_suspected {
        // Events were lost: rescan lazily.
        index.invalidate(None);
        index.notify(&IndexEvent::FilesChanged);
        return;
    }

    let mut files_changed = false;
    // Set when a structural change cannot be applied incrementally.
    let mut needs_rebuild = false;
    let mut modified: Vec<PathBuf> = Vec::new();
    let mut modified_seen: HashSet<PathBuf> = HashSet::new();
    let mut scanned_dirs: Vec<PathBuf> = Vec::new();

    for Change { path: raw_path, created_like } in &batch.changes {
        let created_like = *created_like;
        let Some(path) = ctx.rebase(raw_path) else {
            continue;
        };
        let Some(relative) = relative_to(&ctx.root, &path) else {
            continue;
        };
        let components: Vec<String> = relative
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect();
        let Some((own_name, parents)) = components.split_last() else {
            continue; // the root itself
        };

        let is_exclude = ctx.rules.is_exclude_file(&path);
        if !is_exclude && parents.iter().any(|name| should_skip_directory(name)) {
            continue;
        }

        if is_exclude || ctx.rules.is_gitignore_file(&path) {
            // Ignore rules changed: drop the matcher cache, rebuild lazily and
            // (re)watch directories that may just have become visible.
            ctx.rules.clear();
            index.invalidate(None);
            files_changed = true;
            if PER_DIRECTORY_WATCHES {
                match watch_tree(backend, &ctx.root, &ctx.rules) {
                    Ok(0) => {}
                    Ok(n) => log::warn!("{n} directories could not be watched"),
                    Err(error) => log::warn!("cannot re-watch {}: {error}", ctx.root.display()),
                }
                watch_git_info(backend, &ctx.rules);
            }
            continue;
        }

        let Ok(meta) = fs::symlink_metadata(&path) else {
            // Gone (deleted or renamed away).
            if index.is_dirty() {
                // A build may be in flight and have seen the file: make sure
                // its result is discarded.
                needs_rebuild |= !ctx.rules.is_ignored_with_ancestors(&path, false);
            } else {
                files_changed |= index.remove_path_silent(&path);
            }
            continue;
        };
        let file_type = meta.file_type();

        if file_type.is_dir() {
            if should_skip_directory(own_name) || ctx.rules.is_ignored_with_ancestors(&path, true) {
                continue;
            }
            if !created_like || scanned_dirs.iter().any(|d| is_within_resolved(d, &path)) {
                continue; // directory content changes arrive as their own events
            }
            if PER_DIRECTORY_WATCHES {
                match watch_tree(backend, &path, &ctx.rules) {
                    Ok(0) => {}
                    Ok(n) => {
                        log::warn!("{n} new directories under {} could not be watched", path.display());
                        needs_rebuild = true;
                    }
                    Err(error) => {
                        log::warn!("cannot watch {}: {error}", path.display());
                        needs_rebuild = true;
                    }
                }
            }
            if index.is_dirty() {
                needs_rebuild = true;
            } else if let Ok(found) = scan_files(&ctx.root, &path, &ctx.rules) {
                files_changed |= index.replace_subtree_silent(&path, found);
            }
            scanned_dirs.push(path);
        } else if file_type.is_file() {
            if !ctx.rules.should_track_file(&path) {
                continue;
            }
            if created_like && index.is_dirty() {
                needs_rebuild = true;
                continue;
            }
            let added = created_like && index.add_file_silent(&path);
            if added {
                files_changed = true;
            } else if modified_seen.insert(path.clone()) {
                // Content change, or a rename/create over an existing file
                // (the usual "atomic save" pattern).
                modified.push(path);
            }
        }
    }

    if needs_rebuild {
        index.invalidate(None);
        files_changed = true;
    }
    if files_changed {
        index.notify(&IndexEvent::FilesChanged);
    }
    for path in modified {
        index.notify(&IndexEvent::FileModified(path));
    }
}
