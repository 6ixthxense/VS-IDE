//! Git operations (port of the exported functions in `git.ts`, plus branches/log).

use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};

use crate::cmd::{run, run_raw};
use crate::model::{
    Branch, ChangeKind, CommitInfo, DiffRequest, GitFileStatus, GitStatus, OperationResult,
    RepoStatus,
};
use crate::status::parse_status;
use crate::{GitError, Result};

/// True if `root` is inside a git work tree.
pub fn is_repo(root: &Path) -> bool {
    match run_raw(root, ["rev-parse", "--is-inside-work-tree"]) {
        Ok(r) => r.success && r.stdout_string().trim() == "true",
        Err(_) => false,
    }
}

/// Absolute path of the work-tree top level containing `root`.
pub fn repo_root(root: &Path) -> Result<PathBuf> {
    let out = run(root, ["rev-parse", "--show-toplevel"])?;
    let text = out.stdout_string();
    let line = text.trim_end_matches(['\n', '\r']);
    if line.is_empty() {
        return Err(GitError::Parse("empty --show-toplevel output".into()));
    }
    Ok(PathBuf::from(line))
}

/// Parsed `git status` (staged / unstaged / untracked / conflicted + branch, ahead, behind).
///
/// `RepoStatus::root` is the work-tree top level *expressed through the path the caller used*
/// (`root` with the `--show-prefix` components removed), so a workspace opened via a symlink
/// keeps matching the paths the UI holds.  If that cannot be derived it falls back to git's
/// real top-level path.
pub fn status(root: &Path) -> Result<RepoStatus> {
    let out = run(
        root,
        [
            "status",
            "--porcelain=v2",
            "-z",
            "--branch",
            "--untracked-files=all",
        ],
    )?;
    let top = run(root, ["rev-parse", "--show-toplevel", "--show-prefix"])?.stdout_string();
    let mut lines = top.split('\n').map(|l| l.trim_end_matches('\r'));
    let real_top = lines.next().filter(|l| !l.is_empty()).ok_or_else(|| {
        GitError::Parse("empty --show-toplevel output".into())
    })?;
    let prefix = lines.next().unwrap_or_default();
    let caller_top = caller_top(root, prefix).unwrap_or_else(|| PathBuf::from(real_top));
    parse_status(&out.stdout, caller_top)
}

/// Remove the `prefix` components (e.g. `sub/dir/`) from the end of `root`.
fn caller_top(root: &Path, prefix: &str) -> Option<PathBuf> {
    let segments: Vec<&str> = prefix.split('/').filter(|s| !s.is_empty()).collect();
    let mut top = root.to_path_buf();
    for segment in segments.iter().rev() {
        let last = top.file_name()?.to_string_lossy().into_owned();
        if !names_equal(&last, segment) || !top.pop() {
            return None;
        }
    }
    Some(top)
}

#[cfg(windows)]
fn names_equal(a: &str, b: &str) -> bool {
    a.to_lowercase() == b.to_lowercase()
}
#[cfg(not(windows))]
fn names_equal(a: &str, b: &str) -> bool {
    a == b
}

/// Port of `getGitStatus`: absolute path -> [`GitFileStatus`].  Returns an empty map when
/// `root` is not a repository or git fails (like the TS implementation).
///
/// Mapping (same precedence as the TS, where `staged` is applied last): any path with index
/// changes is `Staged` (even if also modified in the work tree); otherwise work-tree changes map
/// to Added/Deleted/Renamed/Modified; untracked is `Untracked`.  Conflicted paths are reported
/// as `Modified` so they do not silently disappear.  Keys are rooted at the caller's `root`.
pub fn status_map(root: &Path) -> GitStatus {
    let Ok(st) = status(root) else {
        return GitStatus::new();
    };
    let mut map = GitStatus::new();
    let abs = |rel: &str| st.root.join(rel);

    for change in &st.unstaged {
        let value = match change.kind {
            ChangeKind::Added => GitFileStatus::Added,
            ChangeKind::Deleted => GitFileStatus::Deleted,
            ChangeKind::Renamed | ChangeKind::Copied => GitFileStatus::Renamed,
            ChangeKind::Modified | ChangeKind::TypeChanged => GitFileStatus::Modified,
        };
        map.insert(abs(&change.path), value);
    }
    for path in &st.conflicted {
        map.insert(abs(path), GitFileStatus::Modified);
    }
    for path in &st.untracked {
        map.insert(abs(path), GitFileStatus::Untracked);
    }
    // Applied last so it wins for partially staged (MM) files, like simple-git's `staged`.
    for change in &st.staged {
        map.insert(abs(&change.path), GitFileStatus::Staged);
    }
    map
}

fn pathspecs(paths: &[PathBuf]) -> Vec<OsString> {
    let mut args: Vec<OsString> = vec!["--".into()];
    args.extend(paths.iter().map(|p| p.as_os_str().to_owned()));
    args
}

/// `git add -- <paths>`.
pub fn add(root: &Path, paths: &[PathBuf]) -> OperationResult {
    OperationResult::from_result(add_inner(root, paths), "Unable to stage the selected changes")
}

fn add_inner(root: &Path, paths: &[PathBuf]) -> Result<()> {
    if paths.is_empty() {
        return Ok(());
    }
    let mut args: Vec<OsString> = vec!["add".into()];
    args.extend(pathspecs(paths));
    run(root, args).map(|_| ())
}

fn has_head(root: &Path) -> bool {
    run_raw(root, ["rev-parse", "--verify", "-q", "HEAD"])
        .map(|r| r.success)
        .unwrap_or(false)
}

/// `git reset -q -- <paths>` (works with and without commits; leaves the work tree alone).
pub fn unstage(root: &Path, paths: &[PathBuf]) -> OperationResult {
    OperationResult::from_result(
        unstage_inner(root, paths),
        "Unable to unstage the selected changes",
    )
}

fn unstage_inner(root: &Path, paths: &[PathBuf]) -> Result<()> {
    if paths.is_empty() {
        return Ok(());
    }
    let mut args: Vec<OsString> = vec!["reset".into(), "-q".into()];
    args.extend(pathspecs(paths));
    run(root, args).map(|_| ())
}

/// Lexically normalise a path (no filesystem access).
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

fn component_eq(a: &std::ffi::OsStr, b: &std::ffi::OsStr) -> bool {
    #[cfg(windows)]
    {
        a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase()
    }
    #[cfg(not(windows))]
    {
        a == b
    }
}

/// `file` relative to `root` (lexical; case-insensitive on Windows).  `None` if outside.
fn relative_to(root: &Path, file: &Path) -> Option<PathBuf> {
    let root = normalize(root);
    let file = normalize(&if file.is_absolute() {
        file.to_path_buf()
    } else {
        root.join(file)
    });
    let mut root_components = root.components();
    let mut file_components = file.components();
    loop {
        match (root_components.next(), file_components.next()) {
            (None, None) => return Some(PathBuf::new()),
            (None, Some(first)) => {
                let mut rest = PathBuf::from(first.as_os_str());
                rest.extend(file_components.map(|c| c.as_os_str().to_owned()));
                return Some(rest);
            }
            (Some(_), None) => return None,
            (Some(r), Some(f)) => {
                if !component_eq(r.as_os_str(), f.as_os_str()) {
                    return None;
                }
            }
        }
    }
}

/// Canonical form of the nearest existing ancestor of `path` (with the missing tail re-appended).
fn canonical_existing_prefix(path: &Path) -> PathBuf {
    let mut missing = Vec::new();
    let mut current = path.to_path_buf();
    loop {
        if let Ok(real) = std::fs::canonicalize(&current) {
            return missing.iter().rev().fold(real, |acc, part: &std::ffi::OsString| acc.join(part));
        }
        match (current.parent().map(Path::to_path_buf), current.file_name().map(ToOwned::to_owned)) {
            (Some(parent), Some(name)) => {
                missing.push(name);
                current = parent;
            }
            _ => return path.to_path_buf(),
        }
    }
}

fn has_git_component(path: &Path) -> bool {
    path.components()
        .any(|c| component_eq(c.as_os_str(), std::ffi::OsStr::new(".git")))
}

/// Path with `/` separators, as git prints it.
fn slash_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// Port of `gitDiscard`.
///
/// Safer than the TS version: paths inside `.git` are refused, the (canonicalised) parent must
/// stay inside the repository root (no symlink escapes), and untracked/added files are only
/// deleted after git itself confirms they are untracked / newly added.
pub fn discard(root: &Path, request: &DiffRequest) -> OperationResult {
    let status = request.status.as_deref();
    if status == Some("renamed") {
        return OperationResult {
            success: false,
            error: Some("Discard for renamed files is not supported yet".into()),
            details: None,
        };
    }
    OperationResult::from_result(discard_inner(root, request), "Unable to discard local changes")
}

fn discard_inner(root: &Path, request: &DiffRequest) -> Result<()> {
    let Some(relative) = relative_to(root, &request.file_path) else {
        return Err(GitError::InvalidArgument(
            "file is outside the repository root".into(),
        ));
    };
    if relative.as_os_str().is_empty() {
        return Err(GitError::InvalidArgument(
            "refusing to discard the repository root".into(),
        ));
    }
    if has_git_component(&relative) {
        return Err(GitError::InvalidArgument(
            "refusing to touch the .git directory".into(),
        ));
    }
    let absolute = root.join(&relative);
    // Symlink escape: the real parent directory must still be inside the real root.
    let real_root = canonical_existing_prefix(root);
    let real_parent = canonical_existing_prefix(absolute.parent().unwrap_or(root));
    if relative_to(&real_root, &real_parent).is_none() {
        return Err(GitError::InvalidArgument(
            "file resolves outside the repository root".into(),
        ));
    }

    let status = request.status.as_deref();
    if status == Some("untracked") || status == Some("added") {
        let meta = match std::fs::symlink_metadata(&absolute) {
            Ok(meta) => meta,
            // `force: true` semantics: nothing to remove is success.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e.into()),
        };
        let wanted = slash_path(&relative);
        return if status == Some("untracked") {
            remove_untracked(root, &absolute, &relative, &wanted, meta.is_dir())
        } else {
            if meta.is_dir() || !is_newly_added(root, &wanted)? {
                return Err(GitError::InvalidArgument(
                    "path is not a newly added file".into(),
                ));
            }
            std::fs::remove_file(&absolute).map_err(Into::into)
        };
    }

    let mut args: Vec<OsString> = vec!["restore".into(), "--source=HEAD".into()];
    if request.staged {
        args.push("--staged".into());
    }
    args.push("--worktree".into());
    args.push("--".into());
    args.push(relative.into_os_string());
    run(root, args).map(|_| ())
}

/// Files below `relative` that git lists as untracked (not ignored), relative to `root`.
fn untracked_files(root: &Path, relative: &Path) -> Result<Vec<String>> {
    let args: Vec<OsString> = vec![
        "ls-files".into(),
        "--others".into(),
        "--exclude-standard".into(),
        "-z".into(),
        "--".into(),
        relative.as_os_str().to_owned(),
    ];
    let out = run(root, args)?;
    Ok(String::from_utf8_lossy(&out.stdout)
        .split('\0')
        .filter(|p| !p.is_empty())
        .map(str::to_string)
        .collect())
}

fn remove_untracked(
    root: &Path,
    absolute: &Path,
    relative: &Path,
    wanted: &str,
    is_dir: bool,
) -> Result<()> {
    let listed = untracked_files(root, relative)?;
    if is_dir {
        // Only the untracked files are removed; tracked files in the same directory survive.
        let prefix = format!("{wanted}/");
        let files: Vec<&String> = listed.iter().filter(|p| p.starts_with(&prefix)).collect();
        if files.is_empty() {
            // Only an empty directory may still be removed; anything else is not untracked.
            return std::fs::remove_dir(absolute).map_err(|_| {
                GitError::InvalidArgument("directory contains no untracked files".into())
            });
        }
        for file in files {
            let path = root.join(file);
            std::fs::remove_file(&path)?;
            // prune now-empty directories up to and including the requested one
            let mut dir = path.parent().map(Path::to_path_buf);
            while let Some(d) = dir {
                if !d.starts_with(absolute) || std::fs::remove_dir(&d).is_err() {
                    break;
                }
                dir = d.parent().map(Path::to_path_buf);
            }
        }
        Ok(())
    } else if listed.iter().any(|p| p == wanted) {
        std::fs::remove_file(absolute).map_err(Into::into)
    } else {
        Err(GitError::InvalidArgument("path is not untracked".into()))
    }
}

/// True if `wanted` (root-relative, `/`) is staged as newly added or marked intent-to-add.
fn is_newly_added(root: &Path, wanted: &str) -> Result<bool> {
    for cached in [true, false] {
        let mut args: Vec<OsString> = vec!["diff".into()];
        if cached {
            args.push("--cached".into());
        }
        args.extend(
            ["--name-only", "--diff-filter=A", "--relative", "--no-renames", "-z", "--no-ext-diff", "--"]
                .map(OsString::from),
        );
        args.push(wanted.into());
        let out = run(root, args)?;
        if String::from_utf8_lossy(&out.stdout).split('\0').any(|p| p == wanted) {
            return Ok(true);
        }
    }
    Ok(false)
}

/// `git commit -m <message>`.
pub fn commit(root: &Path, message: &str) -> OperationResult {
    OperationResult::from_result(commit_inner(root, message), "Commit failed")
}

fn commit_inner(root: &Path, message: &str) -> Result<()> {
    if message.trim().is_empty() {
        return Err(GitError::InvalidArgument("commit message is empty".into()));
    }
    run(root, ["commit", "-m", message]).map(|_| ())
}

/// `git push` (current branch to its upstream).
pub fn push(root: &Path) -> OperationResult {
    OperationResult::from_result(run(root, ["push"]).map(|_| ()), "Push failed")
}

/// `git pull`.
pub fn pull(root: &Path) -> OperationResult {
    OperationResult::from_result(run(root, ["pull", "--no-edit"]).map(|_| ()), "Pull failed")
}

fn synthetic_diff(file_path: &str, content: &str, status: Option<&str>) -> String {
    let path = file_path.replace('\\', "/");
    let prefix = if status == Some("deleted") { '-' } else { '+' };
    let mut lines = vec![
        format!("diff --git a/{path} b/{path}"),
        format!("--- a/{path}"),
        format!("+++ b/{path}"),
    ];
    lines.extend(content.split('\n').map(|l| format!("{prefix}{l}")));
    lines.join("\n")
}

/// Port of `getGitDiff`.  Never fails: on errors returns the same placeholder strings as TS.
pub fn diff(root: &Path, request: &DiffRequest) -> String {
    match diff_inner(root, request) {
        Ok(text) => text,
        Err(_) => "Unable to load diff preview.".to_string(),
    }
}

fn diff_inner(root: &Path, request: &DiffRequest) -> Result<String> {
    // A path outside the root must not be read from disk: show the placeholder instead.
    let relative = relative_to(root, &request.file_path)
        .ok_or_else(|| GitError::InvalidArgument("file is outside the repository root".into()))?;
    let relative_text = relative.to_string_lossy();
    let status = request.status.as_deref();

    let read = |path: &Path| -> Result<String> {
        let bytes = std::fs::read(path)?;
        String::from_utf8(bytes).map_err(|e| GitError::Parse(e.to_string()))
    };

    if status == Some("untracked") {
        let content = read(&root.join(&relative))?;
        return Ok(synthetic_diff(&relative_text, &content, status));
    }

    let mut args: Vec<OsString> = vec![
        "diff".into(),
        "--no-color".into(),
        "--no-ext-diff".into(),
        "--no-textconv".into(),
    ];
    if request.staged {
        args.push("--cached".into());
    }
    args.push("--".into());
    args.push(relative.clone().into_os_string());
    let out = run(root, args)?.stdout_string();
    if !out.trim().is_empty() {
        return Ok(out);
    }

    if status == Some("added") {
        let content = read(&root.join(&relative))?;
        return Ok(synthetic_diff(&relative_text, &content, status));
    }
    Ok("No diff available for this change.".to_string())
}

/// Name of the current branch, `None` when HEAD is detached.  Works on an unborn branch.
pub fn current_branch(root: &Path) -> Result<Option<String>> {
    let out = run_raw(root, ["symbolic-ref", "--short", "-q", "HEAD"])?;
    if out.success {
        return Ok(Some(out.stdout_string().trim().to_string()));
    }
    if is_repo(root) {
        Ok(None)
    } else {
        Err(GitError::NotARepo(root.display().to_string()))
    }
}

/// List local (and optionally remote) branches.
pub fn branches(root: &Path, include_remote: bool) -> Result<Vec<Branch>> {
    let mut args = vec![
        "for-each-ref",
        "--format=%(HEAD)%00%(refname)%00%(refname:short)%00%(objectname)%00%(upstream:short)",
        "--sort=refname",
        "refs/heads",
    ];
    if include_remote {
        args.push("refs/remotes");
    }
    let out = run(root, args)?.stdout_string();
    let mut result = Vec::new();
    for line in out.lines() {
        let parts: Vec<&str> = line.split('\0').collect();
        if parts.len() < 5 {
            continue;
        }
        let remote = parts[1].starts_with("refs/remotes/");
        if remote && parts[1].ends_with("/HEAD") {
            continue;
        }
        result.push(Branch {
            name: parts[2].to_string(),
            current: parts[0] == "*",
            remote,
            commit: parts[3].to_string(),
            upstream: (!parts[4].is_empty()).then(|| parts[4].to_string()),
        });
    }
    Ok(result)
}

fn validate_ref_name(name: &str) -> Result<()> {
    if name.trim().is_empty() || name.starts_with('-') {
        return Err(GitError::InvalidArgument(format!("invalid branch name: {name:?}")));
    }
    Ok(())
}

/// `git checkout <branch> --`.
pub fn checkout(root: &Path, branch: &str) -> OperationResult {
    let result = validate_ref_name(branch)
        .and_then(|()| run(root, ["checkout", branch, "--"]).map(|_| ()));
    OperationResult::from_result(result, "Unable to switch branches")
}

/// Create a branch (optionally from `start_point`) and optionally switch to it.
pub fn create_branch(
    root: &Path,
    name: &str,
    start_point: Option<&str>,
    switch: bool,
) -> OperationResult {
    let inner = || -> Result<()> {
        validate_ref_name(name)?;
        run(root, ["check-ref-format", "--branch", name])?;
        let mut args: Vec<&str> = if switch {
            vec!["checkout", "-b", name]
        } else {
            vec!["branch", name]
        };
        if let Some(start) = start_point {
            validate_ref_name(start)?;
            args.push(start);
        }
        run(root, args).map(|_| ())
    };
    OperationResult::from_result(inner(), "Unable to create the branch")
}

/// Most recent commits (newest first) reachable from HEAD; empty for an unborn branch.
pub fn log(root: &Path, limit: usize) -> Result<Vec<CommitInfo>> {
    if !is_repo(root) {
        return Err(GitError::NotARepo(root.display().to_string()));
    }
    if !has_head(root) {
        return Ok(Vec::new());
    }
    let out = run(
        root,
        [
            "log".to_string(),
            format!("--max-count={limit}"),
            "-z".to_string(),
            "--format=%H%x1f%h%x1f%an%x1f%ae%x1f%aI%x1f%s".to_string(),
        ],
    )?
    .stdout_string();
    let mut commits = Vec::new();
    for record in out.split('\0').filter(|r| !r.trim().is_empty()) {
        let fields: Vec<&str> = record.trim_start_matches('\n').split('\u{1f}').collect();
        if fields.len() < 6 {
            return Err(GitError::Parse(format!("bad log record: {record:?}")));
        }
        commits.push(CommitInfo {
            hash: fields[0].to_string(),
            short_hash: fields[1].to_string(),
            author_name: fields[2].to_string(),
            author_email: fields[3].to_string(),
            date: fields[4].to_string(),
            subject: fields[5].to_string(),
        });
    }
    Ok(commits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_path_handling() {
        let root = Path::new("/a/b");
        assert_eq!(relative_to(root, Path::new("/a/b/c/d.txt")), Some(PathBuf::from("c/d.txt")));
        assert_eq!(relative_to(root, Path::new("c/../d.txt")), Some(PathBuf::from("d.txt")));
        assert_eq!(relative_to(root, Path::new("/a/x.txt")), None);
    }

    #[test]
    fn caller_top_strips_the_prefix() {
        assert_eq!(caller_top(Path::new("/w/link/sub/dir"), "sub/dir/"), Some(PathBuf::from("/w/link")));
        assert_eq!(caller_top(Path::new("/w/link"), ""), Some(PathBuf::from("/w/link")));
        assert_eq!(caller_top(Path::new("/w/other"), "sub/"), None);
    }

    #[cfg(windows)]
    #[test]
    fn relative_to_folds_case_on_windows() {
        let root = Path::new("C:\\Work\\Repo");
        assert_eq!(relative_to(root, Path::new("c:\\work\\repo\\Src\\A.rs")), Some(PathBuf::from("Src\\A.rs")));
        assert_eq!(relative_to(root, Path::new("C:\\Work\\Other\\a.rs")), None);
        assert!(has_git_component(Path::new("sub\\.GIT\\config")));
    }

    #[test]
    fn relative_to_is_component_wise() {
        // "/a/bc" is not inside "/a/b"
        assert_eq!(relative_to(Path::new("/a/b"), Path::new("/a/bc/x")), None);
        assert_eq!(relative_to(Path::new("/a/b"), Path::new("/a/b")), Some(PathBuf::new()));
    }

    #[test]
    fn synthetic_diff_shape() {
        let d = synthetic_diff("dir\\f.txt", "a\nb", Some("untracked"));
        assert_eq!(d, "diff --git a/dir/f.txt b/dir/f.txt\n--- a/dir/f.txt\n+++ b/dir/f.txt\n+a\n+b");
        assert!(synthetic_diff("f", "x", Some("deleted")).ends_with("\n-x"));
    }
}
