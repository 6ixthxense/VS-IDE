//! Thin filesystem facade used by the UI.
//!
//! Everything the app needs from disk goes through [`Workspace`], so the
//! implementation can later be swapped for the `vsm-fs_index` crate (watcher,
//! gitignore-aware walk) without touching any UI code.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

/// Directories that are never descended into.
const SKIP_DIRS: &[&str] = &[".git", "node_modules", "target"];
/// Hard cap on scanned entries so a huge folder cannot stall start-up.
const MAX_ENTRIES: usize = 200_000;
/// Files above this size are refused by the editor (spike limit).
const MAX_FILE_BYTES: u64 = 32 * 1024 * 1024;

/// One file or directory below the workspace root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    /// Path relative to the root, always `/`-separated.
    pub rel: String,
    pub is_dir: bool,
}

impl FileEntry {
    pub fn name(&self) -> &str {
        self.rel.rsplit('/').next().unwrap_or(&self.rel)
    }
}

/// Result of a directory walk.
#[derive(Debug, Default)]
pub struct Scan {
    /// Grouped per directory (parents before children); inside one directory
    /// directories come first, then names case-insensitively.
    pub entries: Vec<FileEntry>,
    /// True when [`MAX_ENTRIES`] was hit.
    pub truncated: bool,
}

impl Scan {
    pub fn files(&self) -> impl Iterator<Item = &FileEntry> {
        self.entries.iter().filter(|e| !e.is_dir)
    }
}

/// Text loaded from disk, normalised for the editor.
#[derive(Debug)]
pub struct LoadedText {
    /// Content with `\r\n` converted to `\n`.
    pub text: String,
    /// The file used CRLF line endings (restored on save).
    pub crlf: bool,
}

#[derive(Debug, Clone)]
pub struct Workspace {
    root: PathBuf,
}

impl Workspace {
    pub fn open(path: impl AsRef<Path>) -> io::Result<Self> {
        let root = fs::canonicalize(path.as_ref())?;
        if !root.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::NotADirectory,
                format!("{} is not a directory", root.display()),
            ));
        }
        Ok(Self { root })
    }

    /// Folder name for display.
    pub fn name(&self) -> String {
        self.root
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.root.display().to_string())
    }

    pub fn abs(&self, rel: &str) -> PathBuf {
        let mut p = self.root.clone();
        p.extend(rel.split('/'));
        p
    }

    /// Walk the workspace. Symlinks are listed as files but never followed.
    pub fn scan(&self) -> Scan {
        let mut scan = Scan::default();
        let mut stack: Vec<(PathBuf, String)> = vec![(self.root.clone(), String::new())];
        // Depth-first with sorted children so output order is the display order.
        while let Some((dir, prefix)) = stack.pop() {
            let Ok(read) = fs::read_dir(&dir) else { continue };
            let mut children: Vec<(String, bool)> = read
                .filter_map(Result::ok)
                .filter_map(|e| {
                    let name = e.file_name().to_string_lossy().into_owned();
                    let ft = e.file_type().ok()?;
                    let is_dir = ft.is_dir();
                    if is_dir && SKIP_DIRS.contains(&name.as_str()) {
                        return None;
                    }
                    Some((name, is_dir))
                })
                .collect();
            children.sort_by(|a, b| {
                b.1.cmp(&a.1)
                    .then_with(|| a.0.to_lowercase().cmp(&b.0.to_lowercase()))
            });

            let mut subdirs = Vec::new();
            let mut flat = Vec::new();
            for (name, is_dir) in children {
                let rel = if prefix.is_empty() {
                    name.clone()
                } else {
                    format!("{prefix}/{name}")
                };
                if is_dir {
                    subdirs.push((dir.join(&name), rel.clone()));
                }
                flat.push(FileEntry { rel, is_dir });
            }
            // Entries are emitted one directory at a time; within a directory
            // they are already in display order. Consumers group by parent.
            scan.entries.extend(flat);
            if scan.entries.len() >= MAX_ENTRIES {
                scan.truncated = true;
                break;
            }
            // Reverse so the first directory is popped first.
            stack.extend(subdirs.into_iter().rev());
        }
        scan
    }

    pub fn read_text(&self, rel: &str) -> io::Result<LoadedText> {
        let path = self.abs(rel);
        let meta = fs::metadata(&path)?;
        if meta.len() > MAX_FILE_BYTES {
            return Err(io::Error::other(format!(
                "file is too large ({} MB)",
                meta.len() / (1024 * 1024)
            )));
        }
        let bytes = fs::read(&path)?;
        if bytes[..bytes.len().min(8192)].contains(&0) {
            return Err(io::Error::other("binary file"));
        }
        let text = String::from_utf8(bytes)
            .map_err(|_| io::Error::other("file is not valid UTF-8"))?;
        let crlf = text.contains("\r\n");
        let text = if crlf { text.replace("\r\n", "\n") } else { text };
        Ok(LoadedText { text, crlf })
    }

    pub fn write_text(&self, rel: &str, text: &str, crlf: bool) -> io::Result<()> {
        let path = self.abs(rel);
        if crlf {
            fs::write(path, text.replace('\n', "\r\n"))
        } else {
            fs::write(path, text)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("vsm-ws-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn scan_skips_ignored_dirs_and_sorts_dirs_first() {
        let root = tmp("scan");
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir_all(root.join("node_modules/x")).unwrap();
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::write(root.join("b.txt"), "b").unwrap();
        fs::write(root.join("A.txt"), "a").unwrap();
        fs::write(root.join("src/main.rs"), "fn main(){}").unwrap();
        let ws = Workspace::open(&root).unwrap();
        let scan = ws.scan();
        let rels: Vec<_> = scan.entries.iter().map(|e| e.rel.as_str()).collect();
        assert_eq!(rels, ["src", "A.txt", "b.txt", "src/main.rs"]);
        assert!(!scan.truncated);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn crlf_roundtrip() {
        let root = tmp("crlf");
        fs::write(root.join("f.txt"), "a\r\nb\r\n").unwrap();
        let ws = Workspace::open(&root).unwrap();
        let loaded = ws.read_text("f.txt").unwrap();
        assert_eq!(loaded.text, "a\nb\n");
        assert!(loaded.crlf);
        ws.write_text("f.txt", &loaded.text, loaded.crlf).unwrap();
        assert_eq!(fs::read(root.join("f.txt")).unwrap(), b"a\r\nb\r\n");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn binary_is_rejected() {
        let root = tmp("bin");
        fs::write(root.join("x.bin"), [0u8, 1, 2]).unwrap();
        let ws = Workspace::open(&root).unwrap();
        assert!(ws.read_text("x.bin").is_err());
        let _ = fs::remove_dir_all(root);
    }
}
