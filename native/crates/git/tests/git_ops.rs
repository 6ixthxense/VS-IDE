//! Integration tests: every operation against real temp repositories.

use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;
use vsm_git::*;

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn init_repo() -> (TempDir, PathBuf) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    git(&root, &["init", "-q", "-b", "main"]);
    git(&root, &["config", "user.name", "Test User"]);
    git(&root, &["config", "user.email", "test@example.com"]);
    git(&root, &["config", "commit.gpgsign", "false"]);
    (tmp, root)
}

fn write(root: &Path, rel: &str, content: &str) -> PathBuf {
    let path = root.join(rel);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("mkdir");
    }
    std::fs::write(&path, content).expect("write");
    path
}

fn commit_all(root: &Path, msg: &str) {
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", msg]);
}

fn req(file: &Path, staged: bool, status: &str) -> DiffRequest {
    DiffRequest {
        file_path: file.to_path_buf(),
        staged,
        status: Some(status.to_string()),
    }
}

#[test]
fn is_repo_detects_repositories() {
    let (_tmp, root) = init_repo();
    assert!(is_repo(&root));
    let plain = tempfile::tempdir().expect("tempdir");
    assert!(!is_repo(plain.path()));
    assert!(!is_repo(&root.join("does-not-exist")));
}

#[test]
fn status_outside_repo_is_error_and_map_is_empty() {
    let plain = tempfile::tempdir().expect("tempdir");
    assert!(matches!(status(plain.path()), Err(GitError::NotARepo(_))));
    assert!(status_map(plain.path()).is_empty());
}

#[test]
fn status_reports_staged_unstaged_untracked_and_branch() {
    let (_tmp, root) = init_repo();
    write(&root, "tracked.txt", "one\n");
    write(&root, "other.txt", "x\n");
    commit_all(&root, "init");

    write(&root, "tracked.txt", "one\ntwo\n"); // unstaged modify
    write(&root, "new.txt", "n\n");
    git(&root, &["add", "new.txt"]); // staged add
    write(&root, "dir/untracked file.txt", "u\n");
    std::fs::remove_file(root.join("other.txt")).expect("rm");

    let st = status(&root).expect("status");
    assert_eq!(st.root, root);
    assert_eq!(st.branch.head.as_deref(), Some("main"));
    assert_eq!(st.branch.upstream, None);
    assert_eq!((st.branch.ahead, st.branch.behind), (0, 0));
    assert_eq!(st.staged.len(), 1);
    assert_eq!(st.staged[0].path, "new.txt");
    assert_eq!(st.staged[0].kind, ChangeKind::Added);
    let unstaged: Vec<_> = st.unstaged.iter().map(|c| (c.path.as_str(), c.kind)).collect();
    assert!(unstaged.contains(&("tracked.txt", ChangeKind::Modified)));
    assert!(unstaged.contains(&("other.txt", ChangeKind::Deleted)));
    assert_eq!(st.untracked, vec!["dir/untracked file.txt"]);
    assert!(st.conflicted.is_empty());

    let map = status_map(&root);
    assert_eq!(map.get(&root.join("new.txt")), Some(&GitFileStatus::Staged));
    assert_eq!(map.get(&root.join("tracked.txt")), Some(&GitFileStatus::Modified));
    assert_eq!(map.get(&root.join("other.txt")), Some(&GitFileStatus::Deleted));
    assert_eq!(
        map.get(&root.join("dir/untracked file.txt")),
        Some(&GitFileStatus::Untracked)
    );
}

#[test]
fn status_handles_unborn_branch_renames_and_unicode_paths() {
    let (_tmp, root) = init_repo();
    let st = status(&root).expect("status");
    assert_eq!(st.branch.head.as_deref(), Some("main"));
    assert_eq!(st.branch.oid, None);

    write(&root, "สวัสดี.txt", "hello\n");
    commit_all(&root, "thai");
    git(&root, &["mv", "สวัสดี.txt", "ชื่อใหม่.txt"]);
    let st = status(&root).expect("status");
    assert_eq!(st.staged.len(), 1);
    assert_eq!(st.staged[0].kind, ChangeKind::Renamed);
    assert_eq!(st.staged[0].path, "ชื่อใหม่.txt");
    assert_eq!(st.staged[0].orig_path.as_deref(), Some("สวัสดี.txt"));
}

#[test]
fn status_reports_conflicts() {
    let (_tmp, root) = init_repo();
    write(&root, "f.txt", "base\n");
    commit_all(&root, "base");
    git(&root, &["checkout", "-q", "-b", "other"]);
    write(&root, "f.txt", "other\n");
    commit_all(&root, "other");
    git(&root, &["checkout", "-q", "main"]);
    write(&root, "f.txt", "main\n");
    commit_all(&root, "main");
    let merge = Command::new("git")
        .arg("-C")
        .arg(&root)
        .args(["merge", "other"])
        .output()
        .expect("merge");
    assert!(!merge.status.success());

    let st = status(&root).expect("status");
    assert_eq!(st.conflicted, vec!["f.txt"]);
    assert!(st.staged.is_empty());
    assert_eq!(status_map(&root).get(&root.join("f.txt")), Some(&GitFileStatus::Modified));
}

#[test]
fn add_and_unstage_round_trip_including_special_names() {
    let (_tmp, root) = init_repo();
    write(&root, "base.txt", "b\n");
    commit_all(&root, "init");

    let weird = write(&root, "we ird [1]*.txt", "w\n");
    let other = write(&root, "other.txt", "o\n");
    // Option-looking name must not be treated as a flag.
    let dashed = write(&root, "-n.txt", "d\n");

    let result = add(&root, &[weird.clone(), PathBuf::from("-n.txt")]);
    assert!(result.success, "{result:?}");
    let st = status(&root).expect("status");
    let staged: Vec<_> = st.staged.iter().map(|c| c.path.as_str()).collect();
    assert_eq!(staged, vec!["-n.txt", "we ird [1]*.txt"]);
    assert_eq!(st.untracked, vec!["other.txt"]); // `*` was not expanded

    let result = unstage(&root, &[weird, dashed]);
    assert!(result.success, "{result:?}");
    assert!(status(&root).expect("status").staged.is_empty());
    let _ = other;

    assert!(add(&root, &[]).success);
    let failure = add(&root, &[PathBuf::from("missing.txt")]);
    assert!(!failure.success);
    assert_eq!(failure.error.as_deref(), Some("Unable to stage the selected changes"));
    assert!(failure.details.is_some());
}

#[test]
fn unstage_works_before_first_commit() {
    let (_tmp, root) = init_repo();
    let f = write(&root, "a.txt", "a\n");
    assert!(add(&root, std::slice::from_ref(&f)).success);
    assert_eq!(status(&root).expect("status").staged.len(), 1);
    let result = unstage(&root, &[f]);
    assert!(result.success, "{result:?}");
    let st = status(&root).expect("status");
    assert!(st.staged.is_empty());
    assert_eq!(st.untracked, vec!["a.txt"]);
}

#[test]
fn commit_and_log() {
    let (_tmp, root) = init_repo();
    assert!(log(&root, 10).expect("empty log").is_empty());

    write(&root, "a.txt", "a\n");
    assert!(add(&root, &[PathBuf::from("a.txt")]).success);
    let result = commit(&root, "first: add a");
    assert!(result.success, "{result:?}");

    write(&root, "a.txt", "a2\n");
    assert!(add(&root, &[PathBuf::from("a.txt")]).success);
    assert!(commit(&root, "second\n\nwith body \"quoted\" $(rm -rf /)").success);

    let commits = log(&root, 10).expect("log");
    assert_eq!(commits.len(), 2);
    assert_eq!(commits[0].subject, "second");
    assert_eq!(commits[1].subject, "first: add a");
    assert_eq!(commits[0].author_name, "Test User");
    assert_eq!(commits[0].author_email, "test@example.com");
    assert_eq!(commits[0].hash.len(), 40);
    assert!(commits[0].hash.starts_with(&commits[0].short_hash));
    assert_eq!(log(&root, 1).expect("log").len(), 1);

    // nothing to commit -> failure result
    let none = commit(&root, "again");
    assert!(!none.success);
    assert_eq!(none.error.as_deref(), Some("Commit failed"));
    // empty message
    write(&root, "b.txt", "b\n");
    add(&root, &[PathBuf::from("b.txt")]);
    assert!(!commit(&root, "   ").success);
}

#[test]
fn diff_unstaged_staged_untracked_and_added() {
    let (_tmp, root) = init_repo();
    let f = write(&root, "a.txt", "one\n");
    commit_all(&root, "init");

    // no change
    assert_eq!(
        diff(&root, &req(&f, false, "modified")),
        "No diff available for this change."
    );

    write(&root, "a.txt", "one\ntwo\n");
    let unstaged = diff(&root, &req(&f, false, "modified"));
    assert!(unstaged.contains("diff --git a/a.txt b/a.txt"), "{unstaged}");
    assert!(unstaged.contains("+two"));
    assert!(!unstaged.contains('\u{1b}'));

    assert_eq!(
        diff(&root, &req(&f, true, "staged")),
        "No diff available for this change."
    );
    git(&root, &["add", "a.txt"]);
    let staged = diff(&root, &req(&f, true, "staged"));
    assert!(staged.contains("+two"));

    let untracked = write(&root, "sub/new.txt", "x\ny");
    let synthetic = diff(&root, &req(&untracked, false, "untracked"));
    assert_eq!(
        synthetic,
        "diff --git a/sub/new.txt b/sub/new.txt\n--- a/sub/new.txt\n+++ b/sub/new.txt\n+x\n+y"
    );

    // 'added' with an unreadable-by-git diff (empty added file): falls back to synthetic diff
    let empty = write(&root, "empty.txt", "");
    git(&root, &["add", "empty.txt"]);
    let added = diff(&root, &req(&empty, false, "added"));
    assert!(added.starts_with("diff --git a/empty.txt b/empty.txt"), "{added}");

    // missing untracked file
    assert_eq!(
        diff(&root, &req(&root.join("gone.txt"), false, "untracked")),
        "Unable to load diff preview."
    );
}

#[test]
fn discard_restores_removes_and_rejects() {
    let (_tmp, root) = init_repo();
    let f = write(&root, "a.txt", "orig\n");
    commit_all(&root, "init");

    // unstaged modification
    write(&root, "a.txt", "changed\n");
    let result = discard(&root, &req(&f, false, "modified"));
    assert!(result.success, "{result:?}");
    assert_eq!(std::fs::read_to_string(&f).expect("read"), "orig\n");

    // staged modification (both index and work tree restored)
    write(&root, "a.txt", "staged change\n");
    git(&root, &["add", "a.txt"]);
    let result = discard(&root, &req(&f, true, "staged"));
    assert!(result.success, "{result:?}");
    assert_eq!(std::fs::read_to_string(&f).expect("read"), "orig\n");
    let st = status(&root).expect("status");
    assert!(st.staged.is_empty() && st.unstaged.is_empty());

    // deleted file
    std::fs::remove_file(&f).expect("rm");
    assert!(discard(&root, &req(&f, false, "deleted")).success);
    assert!(f.exists());

    // untracked file and directory
    let u = write(&root, "dir/u.txt", "u\n");
    assert!(discard(&root, &req(&u, false, "untracked")).success);
    assert!(!u.exists());
    assert!(discard(&root, &req(&root.join("dir"), false, "untracked")).success);
    assert!(!root.join("dir").exists());
    // already gone: force semantics
    assert!(discard(&root, &req(&u, false, "untracked")).success);

    // renamed unsupported
    let renamed = discard(&root, &req(&f, false, "renamed"));
    assert!(!renamed.success);
    assert_eq!(
        renamed.error.as_deref(),
        Some("Discard for renamed files is not supported yet")
    );

    // outside of the repo is refused
    let outside = tempfile::tempdir().expect("tempdir");
    let victim = outside.path().join("keep.txt");
    std::fs::write(&victim, "keep").expect("write");
    let refused = discard(&root, &req(&victim, false, "untracked"));
    assert!(!refused.success);
    assert!(victim.exists());
}

#[test]
fn branches_checkout_and_create() {
    let (_tmp, root) = init_repo();
    assert_eq!(current_branch(&root).expect("branch"), Some("main".into()));
    assert!(branches(&root, false).expect("branches").is_empty());

    write(&root, "a.txt", "a\n");
    commit_all(&root, "init");

    let result = create_branch(&root, "feature/x", None, false);
    assert!(result.success, "{result:?}");
    let list = branches(&root, false).expect("branches");
    let names: Vec<_> = list.iter().map(|b| (b.name.as_str(), b.current)).collect();
    assert_eq!(names, vec![("feature/x", false), ("main", true)]);
    assert_eq!(list[0].commit.len(), 40);

    assert!(checkout(&root, "feature/x").success);
    assert_eq!(current_branch(&root).expect("branch"), Some("feature/x".into()));
    assert!(create_branch(&root, "topic", None, true).success);
    assert_eq!(current_branch(&root).expect("branch"), Some("topic".into()));

    // invalid / hostile names
    assert!(!checkout(&root, "nope").success);
    assert!(!checkout(&root, "--orphan").success);
    assert!(!create_branch(&root, "-x", None, false).success);
    assert!(!create_branch(&root, "bad name", None, false).success);
    assert!(!create_branch(&root, "topic", None, false).success); // exists

    // detached head
    let head = git(&root, &["rev-parse", "HEAD"]);
    git(&root, &["checkout", "-q", "--detach", head.trim()]);
    assert_eq!(current_branch(&root).expect("branch"), None);
    assert_eq!(status(&root).expect("status").branch.head, None);

    let plain = tempfile::tempdir().expect("tempdir");
    assert!(current_branch(plain.path()).is_err());
}

#[test]
fn ahead_behind_push_and_pull_with_local_remote() {
    let remote = tempfile::tempdir().expect("tempdir");
    let remote_path = remote.path().canonicalize().expect("canon");
    git(&remote_path, &["init", "-q", "--bare", "-b", "main"]);

    let (_tmp, root) = init_repo();
    git(&root, &["remote", "add", "origin", remote_path.to_str().expect("utf8")]);
    write(&root, "a.txt", "a\n");
    commit_all(&root, "init");
    git(&root, &["push", "-q", "-u", "origin", "main"]);

    write(&root, "b.txt", "b\n");
    commit_all(&root, "local only");
    let st = status(&root).expect("status");
    assert_eq!(st.branch.upstream.as_deref(), Some("origin/main"));
    assert_eq!((st.branch.ahead, st.branch.behind), (1, 0));

    let result = push(&root);
    assert!(result.success, "{result:?}");
    let st = status(&root).expect("status");
    assert_eq!((st.branch.ahead, st.branch.behind), (0, 0));
    let remote_branches = branches(&root, true).expect("branches");
    assert!(remote_branches.iter().any(|b| b.remote && b.name == "origin/main"));
    assert!(!remote_branches.iter().any(|b| b.name.ends_with("HEAD")));

    // A second clone pushes a commit; pull in the first.
    let clone = tempfile::tempdir().expect("tempdir");
    let clone_path = clone.path().canonicalize().expect("canon");
    let out = Command::new("git")
        .args(["clone", "-q"])
        .arg(&remote_path)
        .arg(&clone_path)
        .output()
        .expect("clone");
    assert!(out.status.success());
    git(&clone_path, &["config", "user.name", "Other"]);
    git(&clone_path, &["config", "user.email", "o@example.com"]);
    git(&clone_path, &["config", "commit.gpgsign", "false"]);
    write(&clone_path, "c.txt", "c\n");
    commit_all(&clone_path, "from clone");
    git(&clone_path, &["push", "-q"]);

    git(&root, &["fetch", "-q"]);
    assert_eq!(status(&root).expect("status").branch.behind, 1);
    let result = pull(&root);
    assert!(result.success, "{result:?}");
    assert!(root.join("c.txt").exists());

    // push with no remote fails gracefully
    let (_tmp2, lonely) = init_repo();
    write(&lonely, "x", "x");
    commit_all(&lonely, "x");
    let failed = push(&lonely);
    assert!(!failed.success);
    assert_eq!(failed.error.as_deref(), Some("Push failed"));
}

#[test]
fn operations_work_from_subdirectory_root() {
    let (_tmp, root) = init_repo();
    write(&root, "sub/a.txt", "a\n");
    commit_all(&root, "init");
    write(&root, "sub/a.txt", "a2\n");
    let map = status_map(&root.join("sub"));
    assert_eq!(map.get(&root.join("sub/a.txt")), Some(&GitFileStatus::Modified));
    assert_eq!(repo_root(&root.join("sub")).expect("root"), root);
}

// ---- review fixes -------------------------------------------------------------------------

#[cfg(unix)]
fn write_script(dir: &Path, name: &str, marker: &Path) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let script = dir.join(name);
    // Records that it ran; prints nothing (an empty fsmonitor/textconv answer is harmless).
    std::fs::write(&script, format!("#!/bin/sh\necho ran >> '{}'\ncat \"$@\" 2>/dev/null\n", marker.display()))
        .expect("write script");
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    script
}

#[cfg(unix)]
#[test]
fn repo_config_programs_are_not_run_by_status_or_diff() {
    let (_tmp, root) = init_repo();
    let scripts = tempfile::tempdir().expect("tempdir");
    let fs_marker = scripts.path().join("fsmonitor.ran");
    let tc_marker = scripts.path().join("textconv.ran");
    let fsmonitor = write_script(scripts.path(), "fsm.sh", &fs_marker);
    let textconv = write_script(scripts.path(), "tc.sh", &tc_marker);

    write(&root, "a.txt", "one\n");
    write(&root, ".gitattributes", "*.txt diff=evil\n");
    commit_all(&root, "init");
    git(&root, &["config", "core.fsmonitor", fsmonitor.to_str().expect("utf8")]);
    git(&root, &["config", "diff.evil.textconv", textconv.to_str().expect("utf8")]);
    write(&root, "a.txt", "two\n");

    // Control: plain git DOES run both programs, so the assertions below are meaningful.
    git(&root, &["status", "--porcelain"]);
    git(&root, &["diff", "--", "a.txt"]);
    assert!(fs_marker.exists(), "control: git should have run core.fsmonitor");
    assert!(tc_marker.exists(), "control: git should have run textconv");
    std::fs::remove_file(&fs_marker).expect("rm");
    std::fs::remove_file(&tc_marker).expect("rm");

    let st = status(&root).expect("status");
    assert_eq!(st.unstaged.len(), 1);
    let _ = status_map(&root);
    let text = diff(&root, &req(&root.join("a.txt"), false, "modified"));
    assert!(text.contains("+two"), "{text}");
    assert!(!fs_marker.exists(), "core.fsmonitor program was executed");
    assert!(!tc_marker.exists(), "textconv program was executed");
}

#[test]
fn unstage_before_first_commit_modified_after_add_and_with_untracked() {
    let (_tmp, root) = init_repo();
    let a = write(&root, "a.txt", "v1\n");
    assert!(add(&root, std::slice::from_ref(&a)).success);
    write(&root, "a.txt", "v2 (modified after add)\n");
    let u = write(&root, "u.txt", "untracked\n");
    let b = write(&root, "b.txt", "b\n");
    assert!(add(&root, std::slice::from_ref(&b)).success);

    // staged + modified-after-add + untracked all at once, no HEAD
    let result = unstage(&root, &[a.clone(), b.clone()]);
    assert!(result.success, "{result:?}");
    let st = status(&root).expect("status");
    assert!(st.staged.is_empty(), "{st:?}");
    let mut untracked = st.untracked.clone();
    untracked.sort();
    assert_eq!(untracked, vec!["a.txt", "b.txt", "u.txt"]);
    // the work tree content is untouched
    assert_eq!(std::fs::read_to_string(&a).expect("read"), "v2 (modified after add)\n");
    assert!(u.exists());

    // unstaging something that is not staged is a no-op, not an error
    assert!(unstage(&root, &[u]).success);
}

#[test]
fn unstage_after_commit_keeps_work_tree_changes() {
    let (_tmp, root) = init_repo();
    let a = write(&root, "a.txt", "v1\n");
    commit_all(&root, "init");
    write(&root, "a.txt", "v2\n");
    add(&root, std::slice::from_ref(&a));
    write(&root, "a.txt", "v3\n");
    assert!(unstage(&root, std::slice::from_ref(&a)).success);
    let st = status(&root).expect("status");
    assert!(st.staged.is_empty());
    assert_eq!(st.unstaged.len(), 1);
    assert_eq!(std::fs::read_to_string(&a).expect("read"), "v3\n");
}

#[test]
fn partially_staged_files_map_to_staged_like_the_ts() {
    let (_tmp, root) = init_repo();
    write(&root, "mm.txt", "1\n");
    write(&root, "wt.txt", "1\n");
    commit_all(&root, "init");
    write(&root, "mm.txt", "2\n");
    git(&root, &["add", "mm.txt"]);
    write(&root, "mm.txt", "3\n"); // MM
    write(&root, "wt.txt", "2\n"); // only work tree
    let map = status_map(&root);
    assert_eq!(map.get(&root.join("mm.txt")), Some(&GitFileStatus::Staged));
    assert_eq!(map.get(&root.join("wt.txt")), Some(&GitFileStatus::Modified));
}

#[cfg(unix)]
#[test]
fn status_map_keys_follow_a_symlinked_workspace_path() {
    let (_tmp, root) = init_repo();
    write(&root, "sub/a.txt", "a\n");
    write(&root, "top.txt", "t\n");
    commit_all(&root, "init");
    write(&root, "sub/a.txt", "a2\n");
    write(&root, "top.txt", "t2\n");
    write(&root, "sub/new.txt", "n\n");

    let links = tempfile::tempdir().expect("tempdir");
    let link = links.path().join("ws-link");
    std::os::unix::fs::symlink(&root, &link).expect("symlink");

    // workspace opened through the symlink: keys are rooted at the link, not the real path
    let map = status_map(&link);
    assert_eq!(map.get(&link.join("sub/a.txt")), Some(&GitFileStatus::Modified));
    assert_eq!(map.get(&link.join("top.txt")), Some(&GitFileStatus::Modified));
    assert_eq!(map.get(&link.join("sub/new.txt")), Some(&GitFileStatus::Untracked));
    assert!(!map.contains_key(&root.join("sub/a.txt")));
    assert_eq!(status(&link).expect("status").root, link);

    // a subdirectory opened through the symlink
    let sub = link.join("sub");
    let map = status_map(&sub);
    assert_eq!(map.get(&sub.join("a.txt")), Some(&GitFileStatus::Modified));
    assert_eq!(map.get(&link.join("top.txt")), Some(&GitFileStatus::Modified));
    assert_eq!(status(&sub).expect("status").root, link);
}

#[test]
fn discard_refuses_git_dir_and_unconfirmed_untracked_paths() {
    let (_tmp, root) = init_repo();
    let tracked = write(&root, "tracked.txt", "t\n");
    commit_all(&root, "init");

    // .git is never touched
    let config = root.join(".git/config");
    for status_name in ["untracked", "added", "modified"] {
        let r = discard(&root, &req(&config, false, status_name));
        assert!(!r.success, "{status_name}: {r:?}");
    }
    assert!(root.join(".git/config").exists());
    assert!(!discard(&root, &req(&root.join(".git"), false, "untracked")).success);
    assert!(root.join(".git").exists());

    // A tracked file lying about being untracked / added is not deleted
    assert!(!discard(&root, &req(&tracked, false, "untracked")).success);
    assert!(!discard(&root, &req(&tracked, false, "added")).success);
    assert!(tracked.exists());

    // the repository root itself
    assert!(!discard(&root, &req(&root, false, "untracked")).success);
    assert!(root.join(".git").exists());

    // ignored files are not "untracked" for the purpose of deletion
    write(&root, ".gitignore", "ignored.txt\n");
    commit_all(&root, "ignore");
    let ignored = write(&root, "ignored.txt", "i\n");
    assert!(!discard(&root, &req(&ignored, false, "untracked")).success);
    assert!(ignored.exists());
}

#[test]
fn discard_untracked_directory_keeps_tracked_files() {
    let (_tmp, root) = init_repo();
    let tracked = write(&root, "dir/tracked.txt", "t\n");
    commit_all(&root, "init");
    let u1 = write(&root, "dir/u1.txt", "1\n");
    let u2 = write(&root, "dir/nested/u2.txt", "2\n");
    let r = discard(&root, &req(&root.join("dir"), false, "untracked"));
    assert!(r.success, "{r:?}");
    assert!(tracked.exists());
    assert!(!u1.exists() && !u2.exists());
    assert!(!root.join("dir/nested").exists(), "emptied directories are pruned");

    // a fully untracked directory disappears completely
    write(&root, "fresh/a.txt", "a\n");
    assert!(discard(&root, &req(&root.join("fresh"), false, "untracked")).success);
    assert!(!root.join("fresh").exists());
}

#[test]
fn discard_added_file_requires_a_newly_added_entry() {
    let (_tmp, root) = init_repo();
    write(&root, "base.txt", "b\n");
    commit_all(&root, "init");
    let added = write(&root, "new.txt", "n\n");
    git(&root, &["add", "new.txt"]);
    let r = discard(&root, &req(&added, true, "added"));
    assert!(r.success, "{r:?}");
    assert!(!added.exists());

    let base = root.join("base.txt");
    assert!(!discard(&root, &req(&base, true, "added")).success);
    assert!(base.exists());
}

#[cfg(unix)]
#[test]
fn discard_and_diff_do_not_follow_symlinks_out_of_the_root() {
    let (_tmp, root) = init_repo();
    write(&root, "a.txt", "a\n");
    commit_all(&root, "init");

    let outside = tempfile::tempdir().expect("tempdir");
    let victim = outside.path().join("victim.txt");
    std::fs::write(&victim, "keep me\n").expect("write");
    std::os::unix::fs::symlink(outside.path(), root.join("escape")).expect("symlink");

    for status_name in ["untracked", "added", "modified"] {
        let r = discard(&root, &req(&root.join("escape/victim.txt"), false, status_name));
        assert!(!r.success, "{status_name}: {r:?}");
    }
    assert_eq!(std::fs::read_to_string(&victim).expect("read"), "keep me\n");

    // diff of a path outside the root yields the placeholder and never reads that file
    let text = diff(&root, &req(&victim, false, "untracked"));
    assert_eq!(text, "Unable to load diff preview.");
    assert!(!text.contains("keep me"));
}

#[test]
fn diff_outside_the_root_returns_the_placeholder() {
    let (_tmp, root) = init_repo();
    let outside = tempfile::tempdir().expect("tempdir");
    let secret = outside.path().join("secret.txt");
    std::fs::write(&secret, "secret\n").expect("write");
    for status_name in ["untracked", "added", "modified"] {
        assert_eq!(diff(&root, &req(&secret, false, status_name)), "Unable to load diff preview.");
    }
}
