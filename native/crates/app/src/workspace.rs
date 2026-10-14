//! Thin filesystem facade used by the UI.
//!
//! Everything the app needs from disk goes through this module so the UI never
//! touches `std::fs` directly. It owns the rules that keep user data safe:
//!
//! * [`normalize_rel`]: every path entering the app is reduced to one canonical
//!   workspace-relative form, and anything outside the root is rejected;
//! * [`decode_text`] / [`encode_text`]: line-ending and BOM handling that never
//!   produces `\r\r\n` and never flips a whole file because of one stray CRLF;
//! * [`Workspace::save`]: atomic, permission- and symlink-preserving writes that
//!   refuse to clobber a file changed on disk since it was opened;
//! * [`scan_dir`]: the explorer/Quick Open walk (pure, `Send`, run off-thread).

use std::{
    cmp::Reverse,
    collections::hash_map::DefaultHasher,
    fs,
    hash::{Hash, Hasher},
    io,
    path::{Component, Path, PathBuf},
    time::SystemTime,
};

use vsm_fs_index::paths::{relative_to, resolve_path};

/// Hard cap on scanned entries so a huge folder cannot stall start-up.
pub const MAX_ENTRIES: usize = 200_000;
/// Files above this size are refused by the editor (spike limit).
const MAX_FILE_BYTES: u64 = 32 * 1024 * 1024;
const UTF8_BOM: &[u8] = b"\xEF\xBB\xBF";

// ------------------------------------------------------------------ paths

fn invalid(msg: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, msg.into())
}

/// Reduce `input` (relative to `root`, or absolute) to a canonical
/// workspace-relative path with `/` separators.
///
/// `.` and empty segments are dropped, `..` is resolved lexically, and anything
/// that ends up outside `root` (via `..` or an absolute path elsewhere) or that
/// names the root itself is rejected. `root` must be absolute (canonicalised).
pub fn normalize_rel(root: &Path, input: &Path) -> io::Result<String> {
    let mut parts: Vec<String> = Vec::new();
    if input.is_absolute() {
        let abs = fs::canonicalize(input).unwrap_or_else(|_| resolve_path(input));
        let rel = relative_to(root, &abs)
            .ok_or_else(|| invalid(format!("{} is outside the workspace", input.display())))?;
        for c in rel.components() {
            if let Component::Normal(s) = c {
                parts.push(s.to_string_lossy().into_owned());
            }
        }
    } else {
        for c in input.components() {
            match c {
                Component::Normal(s) => parts.push(s.to_string_lossy().into_owned()),
                Component::CurDir => {}
                Component::ParentDir => {
                    if parts.pop().is_none() {
                        return Err(invalid(format!(
                            "{} is outside the workspace",
                            input.display()
                        )));
                    }
                }
                Component::RootDir | Component::Prefix(_) => {
                    return Err(invalid(format!(
                        "{} is not a relative path",
                        input.display()
                    )))
                }
            }
        }
    }
    parts.retain(|p| !p.is_empty());
    if parts.is_empty() {
        return Err(invalid("path does not name a file"));
    }
    Ok(parts.join("/"))
}

/// Key used to decide whether two relative paths are the same tab.
/// Case-insensitive where the usual filesystems are (Windows, macOS).
pub fn rel_key(rel: &str) -> String {
    if cfg!(any(windows, target_os = "macos")) {
        rel.to_lowercase()
    } else {
        rel.to_string()
    }
}

// ------------------------------------------------------------------- scan

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
    /// True when the entry cap was hit.
    pub truncated: bool,
}

impl Scan {
    pub fn files(&self) -> impl Iterator<Item = &FileEntry> {
        self.entries.iter().filter(|e| !e.is_dir)
    }
}

/// Whether a directory called `name` is left out of the listing.
///
/// Shares the rule with the file index (`vsm_fs_index`), plus Cargo's `target`
/// directory, which is only skipped at the workspace root next to a
/// `Cargo.toml` so a source folder that happens to be named `target` is shown.
fn skip_dir(name: &str, at_root: bool, root_has_cargo_toml: bool) -> bool {
    vsm_fs_index::should_skip_directory(name)
        || (at_root && root_has_cargo_toml && name == "target")
}

/// Walk `root`. Symlinks are listed as files but never followed.
pub fn scan_dir(root: &Path) -> Scan {
    scan_dir_limited(root, MAX_ENTRIES)
}

fn scan_dir_limited(root: &Path, max_entries: usize) -> Scan {
    let mut scan = Scan::default();
    let root_has_cargo_toml = root.join("Cargo.toml").is_file();
    let mut stack: Vec<(PathBuf, String)> = vec![(root.to_path_buf(), String::new())];
    'walk: while let Some((dir, prefix)) = stack.pop() {
        let Ok(read) = fs::read_dir(&dir) else {
            continue;
        };
        let at_root = prefix.is_empty();
        let mut children: Vec<(String, bool)> = read
            .filter_map(Result::ok)
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                let is_dir = e.file_type().ok()?.is_dir();
                (!(is_dir && skip_dir(&name, at_root, root_has_cargo_toml)))
                    .then_some((name, is_dir))
            })
            .collect();
        children.sort_by_cached_key(|(name, is_dir)| (Reverse(*is_dir), name.to_lowercase()));

        let mut subdirs = Vec::new();
        for (name, is_dir) in children {
            // Enforce the cap per entry, not per directory.
            if scan.entries.len() >= max_entries {
                scan.truncated = true;
                break 'walk;
            }
            let rel = if at_root {
                name.clone()
            } else {
                format!("{prefix}/{name}")
            };
            if is_dir {
                subdirs.push((dir.join(&name), rel.clone()));
            }
            scan.entries.push(FileEntry { rel, is_dir });
        }
        // Reverse so the first directory is popped first.
        stack.extend(subdirs.into_iter().rev());
    }
    scan
}

// ------------------------------------------------------------ text format

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineEnding {
    Lf,
    Crlf,
}

/// How a document is stored on disk (restored on save).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DocFormat {
    pub eol: LineEnding,
    pub bom: bool,
}

/// Decode file bytes: strip a UTF-8 BOM, pick the dominant line ending and
/// normalise every `\r\n` / lone `\r` to `\n` for the editor buffer.
///
/// The ending is CRLF only if CRLF lines outnumber bare-LF lines, so one stray
/// CRLF in an LF file (or the reverse) does not flip the file.
pub fn decode_text(bytes: Vec<u8>) -> io::Result<(String, DocFormat)> {
    if bytes[..bytes.len().min(8192)].contains(&0) {
        return Err(io::Error::other("binary file"));
    }
    let bom = bytes.starts_with(UTF8_BOM);
    let body = if bom {
        &bytes[UTF8_BOM.len()..]
    } else {
        &bytes[..]
    };
    let text =
        std::str::from_utf8(body).map_err(|_| io::Error::other("file is not valid UTF-8"))?;

    let crlf = text.matches("\r\n").count();
    let lf_total = text.bytes().filter(|&b| b == b'\n').count();
    let eol = if crlf > lf_total - crlf {
        LineEnding::Crlf
    } else {
        LineEnding::Lf
    };
    Ok((normalize_newlines(text), DocFormat { eol, bom }))
}

/// `\r\n` and lone `\r` become `\n`.
fn normalize_newlines(text: &str) -> String {
    if !text.contains('\r') {
        return text.to_string();
    }
    text.replace("\r\n", "\n").replace('\r', "\n")
}

/// Encode the editor buffer for disk. The buffer is normalised first, so text
/// pasted with CRLF into a CRLF file cannot become `\r\r\n`.
pub fn encode_text(text: &str, format: &DocFormat) -> Vec<u8> {
    let normalized = normalize_newlines(text);
    let mut out = Vec::with_capacity(normalized.len() + 8);
    if format.bom {
        out.extend_from_slice(UTF8_BOM);
    }
    match format.eol {
        LineEnding::Lf => out.extend_from_slice(normalized.as_bytes()),
        LineEnding::Crlf => out.extend_from_slice(normalized.replace('\n', "\r\n").as_bytes()),
    }
    out
}

/// Cheap content fingerprint used to detect "buffer equals what was loaded".
pub fn text_hash(text: &str) -> u64 {
    let mut h = DefaultHasher::new();
    text.hash(&mut h);
    h.finish()
}

// ------------------------------------------------------------- file state

/// Modification time and length of a file, used to detect external changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileStamp {
    pub mtime: Option<SystemTime>,
    pub len: u64,
}

impl FileStamp {
    pub fn of(path: &Path) -> io::Result<Self> {
        let meta = fs::metadata(path)?;
        Ok(Self {
            mtime: meta.modified().ok(),
            len: meta.len(),
        })
    }
}

/// A file read from disk, ready for the editor.
#[derive(Debug)]
pub struct LoadedText {
    /// Normalised (`\n` only, no BOM).
    pub text: String,
    pub format: DocFormat,
    /// State of the file when it was read (taken *before* reading, so a change
    /// during the read is reported as a conflict later rather than missed).
    pub stamp: FileStamp,
    pub hash: u64,
}

/// Read and decode a file. Binary, non-UTF-8 and oversized files are refused.
pub fn load(path: &Path) -> io::Result<LoadedText> {
    let stamp = FileStamp::of(path)?;
    if stamp.len > MAX_FILE_BYTES {
        return Err(io::Error::other(format!(
            "file is too large ({} MB)",
            stamp.len / (1024 * 1024)
        )));
    }
    let (text, format) = decode_text(fs::read(path)?)?;
    let hash = text_hash(&text);
    Ok(LoadedText {
        text,
        format,
        stamp,
        hash,
    })
}

#[derive(Debug)]
pub enum SaveError {
    /// The file changed on disk since it was opened (or last saved).
    Conflict,
    Io(io::Error),
}

impl std::fmt::Display for SaveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Conflict => f.write_str("file changed on disk"),
            Self::Io(e) => e.fmt(f),
        }
    }
}

/// Atomically replace the contents of `path`.
///
/// * a symlink is written *through* (its target is replaced, the link stays);
/// * the original permissions are restored on the new file;
/// * readers never see a partial file (temp file + rename in `vsm_core`).
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let target = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let permissions = fs::metadata(&target).ok().map(|m| m.permissions());
    vsm_core::fsutil::atomic_write(&target, bytes).map_err(|e| io::Error::other(e.to_string()))?;
    if let Some(p) = permissions {
        // Best effort: the content is already safely written.
        let _ = fs::set_permissions(&target, p);
    }
    Ok(())
}

// -------------------------------------------------------------- workspace

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

    pub fn root(&self) -> &Path {
        &self.root
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

    /// See [`normalize_rel`].
    pub fn normalize(&self, input: &str) -> io::Result<String> {
        normalize_rel(&self.root, Path::new(input))
    }

    /// Write `text` to `rel`.
    ///
    /// Unless `force`, refuses with [`SaveError::Conflict`] when the file's
    /// mtime/length no longer match `expected`. A file deleted in the meantime
    /// is simply recreated. Returns the new on-disk stamp.
    pub fn save(
        &self,
        rel: &str,
        text: &str,
        format: &DocFormat,
        expected: &FileStamp,
        force: bool,
    ) -> Result<FileStamp, SaveError> {
        let path = self.abs(rel);
        if !force {
            match FileStamp::of(&path) {
                Ok(current) if current != *expected => return Err(SaveError::Conflict),
                Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(SaveError::Io(e)),
                _ => {}
            }
        }
        write_atomic(&path, &encode_text(text, format)).map_err(SaveError::Io)?;
        FileStamp::of(&path).map_err(SaveError::Io)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn dir() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    fn root_of(d: &tempfile::TempDir) -> PathBuf {
        fs::canonicalize(d.path()).unwrap()
    }

    // ---- 5: path normalisation -----------------------------------------

    #[test]
    fn normalize_drops_dot_and_empty_segments() {
        let d = dir();
        let root = root_of(&d);
        for input in [
            "a.txt",
            "./a.txt",
            ".//a.txt",
            "x/../a.txt",
            "./x/./../a.txt",
        ] {
            assert_eq!(
                normalize_rel(&root, Path::new(input)).unwrap(),
                "a.txt",
                "{input}"
            );
        }
        assert_eq!(
            normalize_rel(&root, Path::new("a//b/./c")).unwrap(),
            "a/b/c"
        );
    }

    #[test]
    fn normalize_rejects_escapes_and_the_root_itself() {
        let d = dir();
        let root = root_of(&d);
        for input in ["../secret.txt", "a/../../secret.txt", "..", ".", "", "./"] {
            assert!(normalize_rel(&root, Path::new(input)).is_err(), "{input:?}");
        }
    }

    #[test]
    fn normalize_resolves_absolute_paths_against_the_root() {
        let d = dir();
        let root = root_of(&d);
        fs::create_dir(root.join("src")).unwrap();
        assert_eq!(
            normalize_rel(&root, &root.join("src/m.rs")).unwrap(),
            "src/m.rs"
        );
        assert_eq!(
            normalize_rel(&root, &root.join("src/../src/m.rs")).unwrap(),
            "src/m.rs"
        );
        let other = tempfile::tempdir().unwrap();
        assert!(normalize_rel(&root, &root_of(&other).join("x.txt")).is_err());
        assert!(normalize_rel(&root, &root.join("../x.txt")).is_err());
    }

    #[test]
    fn rel_key_matches_platform_case_rules() {
        let same = rel_key("Src/A.ts") == rel_key("src/a.ts");
        assert_eq!(same, cfg!(any(windows, target_os = "macos")));
        assert_eq!(rel_key("a/b"), "a/b");
    }

    // ---- 11: scan rules ------------------------------------------------

    #[test]
    fn scan_skips_ignored_dirs_and_sorts_dirs_first() {
        let d = dir();
        let root = root_of(&d);
        for p in ["src", "node_modules/x", ".git", ".github", "dist"] {
            fs::create_dir_all(root.join(p)).unwrap();
        }
        fs::write(root.join("b.txt"), "b").unwrap();
        fs::write(root.join("A.txt"), "a").unwrap();
        fs::write(root.join(".gitignore"), "").unwrap();
        fs::write(root.join("src/main.rs"), "fn main(){}").unwrap();
        let scan = scan_dir(&root);
        let rels: Vec<_> = scan.entries.iter().map(|e| e.rel.as_str()).collect();
        assert_eq!(rels, ["src", ".gitignore", "A.txt", "b.txt", "src/main.rs"]);
        assert!(!scan.truncated);
    }

    #[test]
    fn target_is_skipped_only_at_a_cargo_root() {
        let d = dir();
        let root = root_of(&d);
        fs::create_dir_all(root.join("target/debug")).unwrap();
        fs::create_dir_all(root.join("src/target")).unwrap();
        fs::write(root.join("src/target/keep.txt"), "").unwrap();

        // No Cargo.toml: root `target` is an ordinary folder.
        let rels: Vec<_> = scan_dir(&root).entries.into_iter().map(|e| e.rel).collect();
        assert!(rels.contains(&"target".to_string()));
        assert!(rels.contains(&"src/target".to_string()));

        // With Cargo.toml: root `target` is hidden, nested `target` stays.
        fs::write(root.join("Cargo.toml"), "[package]").unwrap();
        let rels: Vec<_> = scan_dir(&root).entries.into_iter().map(|e| e.rel).collect();
        assert!(!rels
            .iter()
            .any(|r| r == "target" || r.starts_with("target/")));
        assert!(rels.contains(&"src/target".to_string()));
        assert!(rels.contains(&"src/target/keep.txt".to_string()));
    }

    #[test]
    fn scan_cap_is_enforced_per_entry() {
        let d = dir();
        let root = root_of(&d);
        for i in 0..20 {
            fs::write(root.join(format!("f{i:02}.txt")), "").unwrap();
        }
        let scan = scan_dir_limited(&root, 7);
        assert_eq!(scan.entries.len(), 7);
        assert!(scan.truncated);
        assert!(!scan_dir_limited(&root, 20).truncated);
    }

    // ---- 2: line endings / BOM -----------------------------------------

    fn roundtrip(bytes: &[u8]) -> Vec<u8> {
        let (text, fmt) = decode_text(bytes.to_vec()).unwrap();
        encode_text(&text, &fmt)
    }

    #[test]
    fn pure_files_round_trip_byte_identical() {
        for bytes in [
            &b"a\nb\n"[..],
            b"a\r\nb\r\n",
            b"",
            b"no newline",
            b"\xEF\xBB\xBFa\r\nb",
        ] {
            assert_eq!(roundtrip(bytes), bytes);
        }
    }

    #[test]
    fn one_stray_crlf_does_not_flip_the_file() {
        let (_, fmt) = decode_text(b"a\nb\nc\r\nd\ne\n".to_vec()).unwrap();
        assert_eq!(fmt.eol, LineEnding::Lf);
        let (_, fmt) = decode_text(b"a\r\nb\r\nc\nd\r\ne\r\n".to_vec()).unwrap();
        assert_eq!(fmt.eol, LineEnding::Crlf);
        // Mixed files never gain `\r\r\n` and stay line-for-line identical.
        let out = roundtrip(b"a\nb\nc\r\nd\ne\n");
        assert_eq!(out, b"a\nb\nc\nd\ne\n");
        assert!(!out.windows(3).any(|w| w == b"\r\r\n"));
    }

    #[test]
    fn pasted_crlf_into_a_crlf_file_is_not_doubled() {
        let (text, fmt) = decode_text(b"one\r\ntwo\r\n".to_vec()).unwrap();
        assert_eq!(fmt.eol, LineEnding::Crlf);
        // The editor keeps pasted "\r\n" verbatim in its buffer.
        let pasted = format!("{text}three\r\nfour\r\n");
        let out = encode_text(&pasted, &fmt);
        assert_eq!(out, b"one\r\ntwo\r\nthree\r\nfour\r\n");
        assert!(!out.windows(3).any(|w| w == b"\r\r\n"));
    }

    #[test]
    fn lone_cr_becomes_a_line_break_and_bom_is_kept() {
        let (text, fmt) = decode_text(b"\xEF\xBB\xBFa\rb\n".to_vec()).unwrap();
        assert_eq!(text, "a\nb\n");
        assert!(fmt.bom);
        assert_eq!(encode_text(&text, &fmt), b"\xEF\xBB\xBFa\nb\n");
    }

    #[test]
    fn binary_and_invalid_utf8_are_rejected() {
        assert!(decode_text(vec![0, 1, 2]).is_err());
        assert!(decode_text(vec![0xFF, 0xFE, b'a']).is_err());
    }

    // ---- 3: atomic, permission- and symlink-preserving writes ----------

    #[test]
    fn save_writes_atomically_without_leaving_temp_files() {
        let d = dir();
        let root = root_of(&d);
        fs::write(root.join("f.txt"), "old").unwrap();
        let ws = Workspace::open(&root).unwrap();
        let stamp = FileStamp::of(&root.join("f.txt")).unwrap();
        let fmt = DocFormat {
            eol: LineEnding::Lf,
            bom: false,
        };
        ws.save("f.txt", "new", &fmt, &stamp, false).unwrap();
        assert_eq!(fs::read_to_string(root.join("f.txt")).unwrap(), "new");
        let names: Vec<_> = fs::read_dir(&root)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names.len(), 1, "{names:?}");
    }

    #[cfg(unix)]
    #[test]
    fn save_preserves_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let d = dir();
        let root = root_of(&d);
        let f = root.join("run.sh");
        fs::write(&f, "#!/bin/sh\n").unwrap();
        fs::set_permissions(&f, fs::Permissions::from_mode(0o755)).unwrap();
        let ws = Workspace::open(&root).unwrap();
        let stamp = FileStamp::of(&f).unwrap();
        let fmt = DocFormat {
            eol: LineEnding::Lf,
            bom: false,
        };
        ws.save("run.sh", "#!/bin/sh\necho hi\n", &fmt, &stamp, false)
            .unwrap();
        assert_eq!(
            fs::metadata(&f).unwrap().permissions().mode() & 0o777,
            0o755
        );
    }

    #[cfg(unix)]
    #[test]
    fn save_writes_through_symlinks() {
        let d = dir();
        let root = root_of(&d);
        fs::write(root.join("real.txt"), "old").unwrap();
        std::os::unix::fs::symlink("real.txt", root.join("link.txt")).unwrap();
        let ws = Workspace::open(&root).unwrap();
        let stamp = FileStamp::of(&root.join("link.txt")).unwrap();
        let fmt = DocFormat {
            eol: LineEnding::Lf,
            bom: false,
        };
        ws.save("link.txt", "new", &fmt, &stamp, false).unwrap();
        assert!(fs::symlink_metadata(root.join("link.txt"))
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(fs::read_to_string(root.join("real.txt")).unwrap(), "new");
    }

    // ---- 4: external change detection ----------------------------------

    #[test]
    fn save_refuses_when_the_file_changed_on_disk_unless_forced() {
        let d = dir();
        let root = root_of(&d);
        let f = root.join("f.txt");
        fs::write(&f, "abc").unwrap();
        let ws = Workspace::open(&root).unwrap();
        let loaded = load(&f).unwrap();
        let fmt = loaded.format;

        // Same length, different mtime (another program rewrote it).
        let file = fs::OpenOptions::new().write(true).open(&f).unwrap();
        file.set_modified(loaded.stamp.mtime.unwrap() + Duration::from_secs(5))
            .unwrap();
        drop(file);
        assert!(matches!(
            ws.save("f.txt", "mine", &fmt, &loaded.stamp, false),
            Err(SaveError::Conflict)
        ));
        assert_eq!(
            fs::read_to_string(&f).unwrap(),
            "abc",
            "refused save must not write"
        );

        // Different length is caught too.
        fs::write(&f, "abcdef").unwrap();
        assert!(matches!(
            ws.save("f.txt", "mine", &fmt, &loaded.stamp, false),
            Err(SaveError::Conflict)
        ));

        // A forced save overwrites and returns a fresh stamp that saves cleanly.
        let stamp = ws.save("f.txt", "mine", &fmt, &loaded.stamp, true).unwrap();
        assert_eq!(fs::read_to_string(&f).unwrap(), "mine");
        ws.save("f.txt", "mine2", &fmt, &stamp, false).unwrap();
    }

    #[test]
    fn save_recreates_a_deleted_file() {
        let d = dir();
        let root = root_of(&d);
        let f = root.join("f.txt");
        fs::write(&f, "abc").unwrap();
        let ws = Workspace::open(&root).unwrap();
        let stamp = FileStamp::of(&f).unwrap();
        fs::remove_file(&f).unwrap();
        let fmt = DocFormat {
            eol: LineEnding::Lf,
            bom: false,
        };
        ws.save("f.txt", "back", &fmt, &stamp, false).unwrap();
        assert_eq!(fs::read_to_string(&f).unwrap(), "back");
    }

    #[test]
    fn load_reports_stamp_and_hash() {
        let d = dir();
        let root = root_of(&d);
        fs::write(root.join("f.txt"), "a\r\nb\r\n").unwrap();
        let loaded = load(&root.join("f.txt")).unwrap();
        assert_eq!(loaded.text, "a\nb\n");
        assert_eq!(loaded.stamp.len, 6);
        assert_eq!(loaded.hash, text_hash("a\nb\n"));
    }
}
