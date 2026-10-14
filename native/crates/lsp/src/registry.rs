//! Language -> server mapping.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

/// How to launch a language server and which languages it serves.
#[derive(Debug, Clone, PartialEq)]
pub struct ServerConfig {
    /// Unique id; documents of all `language_ids` share one server process per id.
    pub id: String,
    pub command: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    /// LSP language ids served (`typescript`, `python`, ...).
    pub language_ids: Vec<String>,
    /// Sent as `initializationOptions`.
    pub initialization_options: Option<Value>,
}

impl ServerConfig {
    pub fn new(id: &str, command: &str, args: &[&str], language_ids: &[&str]) -> Self {
        ServerConfig {
            id: id.to_string(),
            command: command.to_string(),
            args: args.iter().map(|s| s.to_string()).collect(),
            env: Vec::new(),
            language_ids: language_ids.iter().map(|s| s.to_string()).collect(),
            initialization_options: None,
        }
    }
}

/// Registry of servers plus a file-extension -> language-id table.
#[derive(Debug, Clone)]
pub struct LanguageRegistry {
    servers: Vec<ServerConfig>,
    extensions: HashMap<String, String>,
}

impl Default for LanguageRegistry {
    fn default() -> Self {
        Self::with_defaults()
    }
}

impl LanguageRegistry {
    /// Empty registry.
    pub fn empty() -> Self {
        LanguageRegistry { servers: Vec::new(), extensions: HashMap::new() }
    }

    /// TypeScript/JavaScript, Python, Rust.
    pub fn with_defaults() -> Self {
        let mut r = Self::empty();
        r.register(ServerConfig::new(
            "typescript",
            "typescript-language-server",
            &["--stdio"],
            &["typescript", "typescriptreact", "javascript", "javascriptreact"],
        ));
        r.register(ServerConfig::new("python", "pyright-langserver", &["--stdio"], &["python"]));
        r.register(ServerConfig::new("rust", "rust-analyzer", &[], &["rust"]));
        for (ext, lang) in [
            ("ts", "typescript"),
            ("mts", "typescript"),
            ("cts", "typescript"),
            ("tsx", "typescriptreact"),
            ("js", "javascript"),
            ("mjs", "javascript"),
            ("cjs", "javascript"),
            ("jsx", "javascriptreact"),
            ("py", "python"),
            ("pyi", "python"),
            ("rs", "rust"),
        ] {
            r.set_extension(ext, lang);
        }
        r
    }

    /// Add or replace (by id) a server.
    pub fn register(&mut self, cfg: ServerConfig) {
        if let Some(existing) = self.servers.iter_mut().find(|s| s.id == cfg.id) {
            *existing = cfg;
        } else {
            self.servers.push(cfg);
        }
    }

    pub fn remove(&mut self, id: &str) -> Option<ServerConfig> {
        let i = self.servers.iter().position(|s| s.id == id)?;
        Some(self.servers.remove(i))
    }

    pub fn set_extension(&mut self, ext: &str, language_id: &str) {
        self.extensions.insert(ext.trim_start_matches('.').to_ascii_lowercase(), language_id.to_string());
    }

    pub fn servers(&self) -> &[ServerConfig] {
        &self.servers
    }

    pub fn get(&self, id: &str) -> Option<&ServerConfig> {
        self.servers.iter().find(|s| s.id == id)
    }

    /// First server serving `language_id`.
    pub fn for_language(&self, language_id: &str) -> Option<&ServerConfig> {
        self.servers.iter().find(|s| s.language_ids.iter().any(|l| l == language_id))
    }

    pub fn language_id_for_path(&self, path: &Path) -> Option<&str> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        self.extensions.get(&ext).map(String::as_str)
    }
}

/// Pure resolver behind [`resolve_executable`]. `windows` selects PATH separator and PATHEXT
/// semantics; `is_exec` answers "is this a runnable file". On Windows the bare name is only
/// tried when it already ends in a PATHEXT extension (an extensionless `npm`-style shim sitting
/// next to `npm.cmd` is not runnable and must not win).
pub(crate) fn resolve_in(
    command: &str,
    windows: bool,
    path_var: Option<&str>,
    pathext: Option<&str>,
    is_exec: &dyn Fn(&Path) -> bool,
) -> Option<PathBuf> {
    let has_sep = command.contains('/') || (windows && command.contains('\\'));
    let mut exts: Vec<String> = Vec::new();
    let mut try_bare = true;
    if windows {
        let raw = pathext.filter(|s| !s.is_empty()).unwrap_or(".COM;.EXE;.BAT;.CMD");
        exts = raw.split(';').filter(|s| !s.is_empty()).map(|s| s.to_ascii_lowercase()).collect();
        let lower = command.to_ascii_lowercase();
        try_bare = exts.iter().any(|e| lower.ends_with(e.as_str()));
    }
    let candidates = |base: PathBuf| -> Option<PathBuf> {
        if try_bare && is_exec(&base) {
            return Some(base);
        }
        let s = base.to_string_lossy().into_owned();
        exts.iter().map(|e| PathBuf::from(format!("{s}{e}"))).find(|p| is_exec(p))
    };
    if has_sep {
        return candidates(PathBuf::from(command));
    }
    let sep = if windows { ';' } else { ':' };
    path_var?
        .split(sep)
        .filter(|d| !d.is_empty())
        .find_map(|d| candidates(Path::new(d).join(command)))
}

fn is_executable_file(p: &Path) -> bool {
    let Ok(md) = std::fs::metadata(p) else { return false };
    if !md.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        md.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

/// Locate `command` like the shell would: explicit paths are checked directly, otherwise `PATH`
/// is searched. Unix requires the execute bit; Windows tries `PATHEXT` extensions (never the
/// bare name unless it already carries one).
pub fn resolve_executable(command: &str) -> Option<PathBuf> {
    resolve_in(
        command,
        cfg!(windows),
        std::env::var("PATH").ok().as_deref(),
        std::env::var("PATHEXT").ok().as_deref(),
        &is_executable_file,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_and_overrides() {
        let mut r = LanguageRegistry::default();
        assert_eq!(r.for_language("typescriptreact").unwrap().command, "typescript-language-server");
        assert_eq!(r.for_language("python").unwrap().command, "pyright-langserver");
        assert_eq!(r.for_language("rust").unwrap().command, "rust-analyzer");
        assert!(r.for_language("cobol").is_none());
        assert_eq!(r.language_id_for_path(Path::new("/x/a.TSX")), Some("typescriptreact"));
        assert_eq!(r.language_id_for_path(Path::new("a.rs")), Some("rust"));
        assert_eq!(r.language_id_for_path(Path::new("a.unknown")), None);

        r.register(ServerConfig::new("python", "pylsp", &[], &["python"]));
        assert_eq!(r.for_language("python").unwrap().command, "pylsp");
        assert_eq!(r.servers().len(), 3);
        r.register(ServerConfig::new("go", "gopls", &[], &["go"]));
        r.set_extension(".go", "go");
        assert_eq!(r.language_id_for_path(Path::new("main.go")), Some("go"));
        assert!(r.remove("go").is_some());
        assert!(r.for_language("go").is_none());
    }

    fn fake_fs(paths: &[&str]) -> impl Fn(&Path) -> bool {
        let set: std::collections::HashSet<PathBuf> = paths.iter().map(PathBuf::from).collect();
        move |p| set.contains(p)
    }

    #[test]
    fn windows_resolution_prefers_pathext_and_skips_shims() {
        let fs = fake_fs(&["C:\\n/typescript-language-server", "C:\\n/typescript-language-server.cmd", "C:\\n/a.exe"]);
        let r = resolve_in("typescript-language-server", true, Some("C:\\x;C:\\n"), Some(".EXE;.CMD"), &fs);
        assert_eq!(r, Some(PathBuf::from("C:\\n/typescript-language-server.cmd")));
        let only_shim = fake_fs(&["C:\\n/typescript-language-server"]);
        assert_eq!(resolve_in("typescript-language-server", true, Some("C:\\n"), Some(".EXE;.CMD"), &only_shim), None);
        assert_eq!(resolve_in("a.exe", true, Some("C:\\n"), Some(".EXE"), &fs), Some(PathBuf::from("C:\\n/a.exe")));
        // unix semantics: bare name, ':' separated
        let unix = fake_fs(&["/usr/bin/rust-analyzer"]);
        assert_eq!(
            resolve_in("rust-analyzer", false, Some("/bin:/usr/bin"), None, &unix),
            Some(PathBuf::from("/usr/bin/rust-analyzer"))
        );
    }

    #[cfg(unix)]
    #[test]
    fn unix_requires_execute_bit() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("vsm-lsp-resolve-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("tool-noexec");
        std::fs::write(&f, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(resolve_executable(f.to_str().unwrap()).is_none());
        std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(resolve_executable(f.to_str().unwrap()), Some(f.clone()));
        assert!(!is_executable_file(&dir), "directories are not executables");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn resolve_missing_and_existing() {
        assert!(resolve_executable("definitely-not-a-real-binary-xyz").is_none());
        if cfg!(unix) {
            assert!(resolve_executable("sh").is_some());
            assert!(resolve_executable("/bin/sh").is_some());
        }
    }
}
