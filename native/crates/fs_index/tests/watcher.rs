//! End-to-end tests of the debounced watcher (real file system events).

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use vsm_fs_index::{FileIndex, IndexConfig, IndexEvent};

const TIMEOUT: Duration = Duration::from_secs(10);

fn watched_index() -> FileIndex {
    FileIndex::with_config(IndexConfig {
        watch: true,
        debounce: Duration::from_millis(50),
    })
}

fn wait_for(what: &str, mut condition: impl FnMut() -> bool) {
    let start = Instant::now();
    while start.elapsed() < TIMEOUT {
        if condition() {
            return;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    panic!("timed out waiting for: {what}");
}

fn indexed(index: &FileIndex, root: &Path) -> Vec<PathBuf> {
    index.get_files(root).unwrap().to_vec()
}

fn drain(rx: &Receiver<IndexEvent>) -> Vec<IndexEvent> {
    rx.try_iter().collect()
}

#[test]
fn picks_up_created_modified_and_deleted_files() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    fs::write(root.join("existing.txt"), "one").unwrap();

    let index = watched_index();
    assert_eq!(indexed(&index, &root).len(), 1);
    let (_sub, rx) = index.subscribe_channel();

    let created = root.join("created.txt");
    fs::write(&created, "hi").unwrap();
    wait_for("created file to be indexed", || indexed(&index, &root).contains(&created));
    // The index is updated before listeners are notified, so the event can lag behind.
    let mut changes = Vec::new();
    wait_for("files-changed event", || {
        changes.extend(drain(&rx));
        changes.contains(&IndexEvent::FilesChanged)
    });
    assert!(!index.is_dirty(), "creation must be applied incrementally");

    fs::write(root.join("existing.txt"), "two, longer").unwrap();
    let existing = root.join("existing.txt");
    let mut seen = Vec::new();
    wait_for("modification event", || {
        seen.extend(drain(&rx));
        seen.contains(&IndexEvent::FileModified(existing.clone()))
    });

    fs::remove_file(&created).unwrap();
    wait_for("deleted file to leave the index", || !indexed(&index, &root).contains(&created));
}

#[test]
fn renames_and_atomic_saves_are_tracked() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let a = root.join("a.txt");
    fs::write(&a, "a").unwrap();

    let index = watched_index();
    index.get_files(&root).unwrap();
    let (_sub, rx) = index.subscribe_channel();

    let b = root.join("b.txt");
    fs::rename(&a, &b).unwrap();
    wait_for("rename", || {
        let files = indexed(&index, &root);
        files.contains(&b) && !files.contains(&a)
    });

    // Write-temp-then-rename over an existing file must surface as a modification.
    drain(&rx);
    let tmp = root.join("b.txt.tmp");
    fs::write(&tmp, "new content").unwrap();
    fs::rename(&tmp, &b).unwrap();
    let mut seen = Vec::new();
    wait_for("atomic save to be reported as modified", || {
        seen.extend(drain(&rx));
        seen.contains(&IndexEvent::FileModified(b.clone()))
    });
    assert_eq!(indexed(&index, &root), vec![b]);
}

#[test]
fn new_directories_are_scanned_and_watched() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let index = watched_index();
    index.get_files(&root).unwrap();

    let nested = root.join("pkg").join("deep");
    fs::create_dir_all(&nested).unwrap();
    fs::write(nested.join("first.rs"), "").unwrap();
    wait_for("files in a new directory tree", || {
        indexed(&index, &root).contains(&nested.join("first.rs"))
    });

    // A file created later inside the new directory is still noticed.
    fs::write(nested.join("second.rs"), "").unwrap();
    wait_for("later file in a new directory", || {
        indexed(&index, &root).contains(&nested.join("second.rs"))
    });

    // Renaming a populated directory re-maps its files.
    fs::rename(root.join("pkg"), root.join("pkg2")).unwrap();
    wait_for("directory rename", || {
        let files = indexed(&index, &root);
        files.contains(&root.join("pkg2").join("deep").join("first.rs"))
            && !files.contains(&nested.join("first.rs"))
    });
}

#[test]
fn ignored_and_skipped_paths_do_not_enter_the_index() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    fs::write(root.join(".gitignore"), "*.log\ngenerated/\n").unwrap();
    fs::create_dir(root.join("generated")).unwrap();
    fs::create_dir(root.join("node_modules")).unwrap();

    let index = watched_index();
    let before = indexed(&index, &root);
    assert_eq!(before.len(), 1); // only .gitignore

    fs::write(root.join("debug.log"), "").unwrap();
    fs::write(root.join("generated").join("x.rs"), "").unwrap();
    fs::write(root.join("node_modules").join("m.js"), "").unwrap();
    let marker = root.join("marker.txt");
    fs::write(&marker, "").unwrap();
    wait_for("marker file", || indexed(&index, &root).contains(&marker));

    let files = indexed(&index, &root);
    assert_eq!(files.len(), 2, "unexpected entries: {files:?}");
}

#[test]
fn editing_gitignore_invalidates_the_index() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    fs::write(root.join("keep.txt"), "").unwrap();
    fs::write(root.join("secret.txt"), "").unwrap();

    let index = watched_index();
    assert_eq!(indexed(&index, &root).len(), 2);

    fs::write(root.join(".gitignore"), "secret.txt\n").unwrap();
    wait_for("rebuild after .gitignore change", || {
        let files = indexed(&index, &root);
        !files.contains(&root.join("secret.txt")) && files.contains(&root.join("keep.txt"))
    });
}

#[test]
fn closing_the_workspace_stops_events() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let index = watched_index();
    index.get_files(&root).unwrap();
    let (_sub, rx) = index.subscribe_channel();

    index.set_root_path(None);
    fs::write(root.join("late.txt"), "").unwrap();
    std::thread::sleep(Duration::from_millis(400));
    assert!(drain(&rx).is_empty());
    drop(index); // must not hang or panic
}

/// Waits until a `FilesChanged` event arrives (events lag the index update).
fn wait_for_files_changed(rx: &Receiver<IndexEvent>) {
    wait_for("FilesChanged event", || drain(rx).contains(&IndexEvent::FilesChanged));
}

#[test]
fn a_file_created_while_the_index_is_dirty_reports_files_changed() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let index = watched_index();
    index.set_root_path(Some(&root)); // never built: the index is dirty
    let (_sub, rx) = index.subscribe_channel();

    fs::write(root.join("new.ts"), "x").unwrap();
    let mut events = Vec::new();
    wait_for("FilesChanged for a create while dirty", || {
        events.extend(drain(&rx));
        events.contains(&IndexEvent::FilesChanged)
    });
    assert!(
        !events.contains(&IndexEvent::FileModified(root.join("new.ts"))),
        "a brand-new file must not be reported as modified: {events:?}"
    );
    assert!(indexed(&index, &root).contains(&root.join("new.ts")));
}

#[test]
fn a_file_created_during_the_first_build_is_not_lost() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    for i in 0..30 {
        for j in 0..30 {
            let sub = root.join(format!("d{i}/e{j}"));
            fs::create_dir_all(&sub).unwrap();
            for k in 0..10 {
                fs::write(sub.join(format!("f{k}.txt")), "").unwrap();
            }
        }
    }
    let index = watched_index();
    index.set_root_path(Some(&root));

    let builder = {
        let (index, root) = (index.clone(), root.clone());
        std::thread::spawn(move || index.get_files(&root).unwrap().len())
    };
    std::thread::sleep(Duration::from_millis(2));
    let late = root.join("late.ts");
    fs::write(&late, "x").unwrap();
    builder.join().unwrap();

    wait_for("late file to appear in the index", || indexed(&index, &root).contains(&late));
}

#[test]
fn registering_thousands_of_directories_is_fast() {
    // 50 x 100 = 5050 directories. Registration used to be quadratic.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    for i in 0..50 {
        for j in 0..100 {
            fs::create_dir_all(root.join(format!("d{i}/e{j}"))).unwrap();
        }
    }
    let index = watched_index();
    let started = Instant::now();
    index.set_root_path(Some(&root));
    let elapsed = started.elapsed();
    let limit = if cfg!(debug_assertions) {
        Duration::from_secs(5)
    } else {
        Duration::from_secs(1)
    };
    assert!(elapsed < limit, "set_root_path took {elapsed:?} (limit {limit:?})");

    // And the deepest directories really are watched.
    let file = root.join("d49/e99/x.txt");
    index.get_files(&root).unwrap();
    fs::write(&file, "").unwrap();
    wait_for("file in the last registered directory", || indexed(&index, &root).contains(&file));
}

#[test]
fn directories_that_become_unignored_get_watched() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    fs::create_dir(root.join("later")).unwrap();
    fs::write(root.join("later").join("old.txt"), "").unwrap();
    fs::write(root.join(".gitignore"), "later/\n").unwrap();

    let index = watched_index();
    assert!(!indexed(&index, &root).contains(&root.join("later/old.txt")));

    fs::write(root.join(".gitignore"), "").unwrap();
    wait_for("directory to appear after un-ignoring", || {
        indexed(&index, &root).contains(&root.join("later/old.txt"))
    });

    let new = root.join("later").join("new.txt");
    fs::write(&new, "").unwrap();
    wait_for("file created in the newly watched directory", || indexed(&index, &root).contains(&new));
}

#[test]
fn edits_to_git_info_exclude_are_noticed() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    fs::create_dir_all(root.join(".git/info")).unwrap();
    fs::write(root.join(".git/info/exclude"), "").unwrap();
    fs::write(root.join("scratch.txt"), "").unwrap();

    let index = watched_index();
    assert!(indexed(&index, &root).contains(&root.join("scratch.txt")));

    fs::write(root.join(".git/info/exclude"), "scratch.txt\n").unwrap();
    wait_for("exclude rule to take effect", || {
        !indexed(&index, &root).contains(&root.join("scratch.txt"))
    });
}

#[test]
fn ignore_files_inside_skipped_directories_do_not_invalidate() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    fs::create_dir_all(root.join("node_modules/pkg")).unwrap();

    let index = watched_index();
    indexed(&index, &root);
    let (_sub, rx) = index.subscribe_channel();

    fs::write(root.join("node_modules/pkg/.gitignore"), "*\n").unwrap();
    fs::write(root.join("marker.txt"), "").unwrap();
    wait_for_files_changed(&rx);
    assert!(!index.is_dirty(), "a .gitignore in node_modules must be ignored");
    assert!(indexed(&index, &root).contains(&root.join("marker.txt")));
}
