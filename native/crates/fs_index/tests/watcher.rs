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
