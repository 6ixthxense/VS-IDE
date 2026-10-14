//! Parser for `git status --porcelain=v2 -z --branch`.

use crate::model::{BranchInfo, ChangeKind, FileChange, RepoStatus};
use crate::{GitError, Result};

fn kind_from_code(code: u8) -> Option<ChangeKind> {
    match code {
        b'M' => Some(ChangeKind::Modified),
        b'A' => Some(ChangeKind::Added),
        b'D' => Some(ChangeKind::Deleted),
        b'R' => Some(ChangeKind::Renamed),
        b'C' => Some(ChangeKind::Copied),
        b'T' => Some(ChangeKind::TypeChanged),
        _ => None, // '.' = unchanged
    }
}

fn parse_branch_header(line: &str, branch: &mut BranchInfo) {
    if let Some(rest) = line.strip_prefix("# branch.oid ") {
        branch.oid = (rest != "(initial)").then(|| rest.to_string());
    } else if let Some(rest) = line.strip_prefix("# branch.head ") {
        branch.head = (rest != "(detached)").then(|| rest.to_string());
    } else if let Some(rest) = line.strip_prefix("# branch.upstream ") {
        branch.upstream = Some(rest.to_string());
    } else if let Some(rest) = line.strip_prefix("# branch.ab ") {
        for token in rest.split_whitespace() {
            if let Some(n) = token.strip_prefix('+') {
                branch.ahead = n.parse().unwrap_or(0);
            } else if let Some(n) = token.strip_prefix('-') {
                branch.behind = n.parse().unwrap_or(0);
            }
        }
    }
}

/// Split `n` space separated fields off the front of `line`; returns the fields and the remainder.
fn split_fields(line: &str, n: usize) -> Option<(Vec<&str>, &str)> {
    let mut fields = Vec::with_capacity(n);
    let mut rest = line;
    for _ in 0..n {
        let idx = rest.find(' ')?;
        fields.push(&rest[..idx]);
        rest = &rest[idx + 1..];
    }
    Some((fields, rest))
}

pub(crate) fn parse_status(output: &[u8], root: std::path::PathBuf) -> Result<RepoStatus> {
    let text = String::from_utf8_lossy(output);
    let mut status = RepoStatus {
        root,
        ..RepoStatus::default()
    };
    let mut records = text.split('\0').filter(|r| !r.is_empty());

    while let Some(record) = records.next() {
        let bytes = record.as_bytes();
        match bytes.first() {
            Some(b'#') => parse_branch_header(record, &mut status.branch),
            Some(b'?') => {
                if let Some(path) = record.strip_prefix("? ") {
                    status.untracked.push(path.to_string());
                }
            }
            Some(b'!') => {} // ignored files (not requested)
            Some(b'1') => {
                // 1 XY sub mH mI mW hH hI path
                let (fields, path) = split_fields(record, 8)
                    .ok_or_else(|| GitError::Parse(format!("bad ordinary entry: {record:?}")))?;
                push_changes(&mut status, fields[1].as_bytes(), path, None);
            }
            Some(b'2') => {
                // 2 XY sub mH mI mW hH hI Xscore path \0 origPath
                let (fields, path) = split_fields(record, 9)
                    .ok_or_else(|| GitError::Parse(format!("bad rename entry: {record:?}")))?;
                let orig = records.next().map(str::to_string);
                push_changes(&mut status, fields[1].as_bytes(), path, orig);
            }
            Some(b'u') => {
                // u XY sub m1 m2 m3 mW h1 h2 h3 path
                let (_, path) = split_fields(record, 10)
                    .ok_or_else(|| GitError::Parse(format!("bad unmerged entry: {record:?}")))?;
                status.conflicted.push(path.to_string());
            }
            _ => return Err(GitError::Parse(format!("unknown status record: {record:?}"))),
        }
    }
    Ok(status)
}

fn push_changes(status: &mut RepoStatus, xy: &[u8], path: &str, orig: Option<String>) {
    if xy.len() < 2 {
        return;
    }
    if let Some(kind) = kind_from_code(xy[0]) {
        status.staged.push(FileChange {
            path: path.to_string(),
            orig_path: orig.clone(),
            kind,
        });
    }
    if let Some(kind) = kind_from_code(xy[1]) {
        status.unstaged.push(FileChange {
            path: path.to_string(),
            orig_path: orig,
            kind,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_record_types() {
        let out = b"# branch.oid abc123\0# branch.head main\0# branch.upstream origin/main\0# branch.ab +2 -3\0\
1 MM N... 100644 100644 100644 h h src/a b.rs\0\
2 R. N... 100644 100644 100644 h h R100 new name.txt\0old.txt\0\
u UU N... 100644 100644 100644 100644 h h h conflict.txt\0\
? new \xe0\xb8\x81.txt\0";
        let s = parse_status(out, "/r".into()).unwrap();
        assert_eq!(s.branch.head.as_deref(), Some("main"));
        assert_eq!(s.branch.upstream.as_deref(), Some("origin/main"));
        assert_eq!((s.branch.ahead, s.branch.behind), (2, 3));
        assert_eq!(s.staged.len(), 2);
        assert_eq!(s.staged[1].path, "new name.txt");
        assert_eq!(s.staged[1].orig_path.as_deref(), Some("old.txt"));
        assert_eq!(s.unstaged.len(), 1);
        assert_eq!(s.unstaged[0].path, "src/a b.rs");
        assert_eq!(s.conflicted, vec!["conflict.txt"]);
        assert_eq!(s.untracked, vec!["new \u{e01}.txt"]);
    }

    #[test]
    fn detached_and_initial() {
        let out = b"# branch.oid (initial)\0# branch.head (detached)\0";
        let s = parse_status(out, "/r".into()).unwrap();
        assert_eq!(s.branch.oid, None);
        assert_eq!(s.branch.head, None);
    }
}
