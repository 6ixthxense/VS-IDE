//! Executable lookup for explicit `command` argv (Windows needs PATHEXT handling: the bare
//! name often matches an extensionless Unix shim such as npm's `npm`, which cannot be run).

use std::path::{Path, PathBuf};

/// Pure resolver. `windows` selects PATH separator / PATHEXT semantics; `is_exec` answers
/// "is this an existing runnable file". On Windows the bare name is only tried when it already
/// ends in one of the PATHEXT extensions.
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
        exts = raw
            .split(';')
            .filter(|s| !s.is_empty())
            .map(|s| s.to_ascii_lowercase())
            .collect();
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

/// Real-system `is_exec`: a regular file, with an execute bit on Unix.
pub(crate) fn is_executable_file(p: &Path) -> bool {
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

fn is_cmd_script(p: &Path) -> bool {
    matches!(
        p.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref(),
        Some("cmd") | Some("bat")
    )
}

/// Characters that cmd.exe would interpret (or that need quoting we cannot express safely
/// through `cmd /s /c` when the command line is rebuilt by the PTY layer).
fn unsafe_for_cmd(arg: &str) -> bool {
    arg.chars().any(|c| matches!(c, '"' | '&' | '|' | '<' | '>' | '^' | '%' | '!' | '(' | ')') || c.is_whitespace() || c == '\n')
}

/// Turn a user supplied argv into something `CreateProcess` can run.
///
/// * resolves `argv[0]` via PATH/PATHEXT (never picks an extensionless shim);
/// * `.cmd`/`.bat` targets are wrapped as `cmd.exe /d /s /c <script> args...`, and arguments
///   that cmd.exe would reinterpret (spaces, quotes, `& | < > ^ % ! ( )`) are rejected with an
///   error instead of being silently mangled;
/// * unresolvable commands are returned unchanged so the spawn error mentions the real name.
pub(crate) fn windows_argv(
    argv: &[String],
    path_var: Option<&str>,
    pathext: Option<&str>,
    comspec: Option<&str>,
    is_exec: &dyn Fn(&Path) -> bool,
) -> Result<Vec<String>, String> {
    let Some(first) = argv.first() else { return Ok(Vec::new()) };
    let Some(resolved) = resolve_in(first, true, path_var, pathext, is_exec) else {
        return Ok(argv.to_vec());
    };
    let resolved_s = resolved.to_string_lossy().into_owned();
    if !is_cmd_script(&resolved) {
        let mut out = vec![resolved_s];
        out.extend(argv[1..].iter().cloned());
        return Ok(out);
    }
    if let Some(bad) = argv[1..].iter().find(|a| unsafe_for_cmd(a)) {
        return Err(format!(
            "argument {bad:?} cannot be passed safely to the batch script {resolved_s}"
        ));
    }
    let mut out = vec![
        comspec.filter(|s| !s.is_empty()).unwrap_or("cmd.exe").to_string(),
        "/d".into(),
        "/s".into(),
        "/c".into(),
        resolved_s,
    ];
    out.extend(argv[1..].iter().cloned());
    Ok(out)
}

/// Argv to actually spawn for the current OS.
pub(crate) fn platform_argv(argv: &[String]) -> Result<Vec<String>, String> {
    if cfg!(windows) {
        windows_argv(
            argv,
            std::env::var("PATH").ok().as_deref(),
            std::env::var("PATHEXT").ok().as_deref(),
            std::env::var("ComSpec").ok().as_deref(),
            &is_executable_file,
        )
    } else {
        Ok(argv.to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn fs(paths: &[&str]) -> impl Fn(&Path) -> bool {
        let s: HashSet<PathBuf> = paths.iter().map(PathBuf::from).collect();
        move |p| s.contains(p)
    }

    #[test]
    fn windows_skips_extensionless_shim() {
        // `npm` (sh shim) and `npm.cmd` live side by side, like in the Node installer.
        let exists = fs(&["C:\\node/npm", "C:\\node/npm.cmd"]);
        let r = resolve_in("npm", true, Some("C:\\x;C:\\node"), Some(".EXE;.CMD"), &exists);
        assert_eq!(r, Some(PathBuf::from("C:\\node/npm.cmd")));
        // only the shim exists -> not runnable
        let exists = fs(&["C:\\node/npm"]);
        assert_eq!(resolve_in("npm", true, Some("C:\\node"), Some(".EXE;.CMD"), &exists), None);
    }

    #[test]
    fn windows_pathext_order_and_explicit_extension() {
        let exists = fs(&["C:\\a/tool.exe", "C:\\a/tool.cmd", "C:\\a/x.cmd"]);
        assert_eq!(
            resolve_in("tool", true, Some("C:\\a"), Some(".EXE;.CMD"), &exists),
            Some(PathBuf::from("C:\\a/tool.exe"))
        );
        assert_eq!(
            resolve_in("x.cmd", true, Some("C:\\a"), Some(".EXE;.CMD"), &exists),
            Some(PathBuf::from("C:\\a/x.cmd"))
        );
        // default PATHEXT when unset
        assert!(resolve_in("tool", true, Some("C:\\a"), None, &exists).is_some());
    }

    #[test]
    fn unix_uses_bare_name() {
        let exists = fs(&["/usr/bin/tool"]);
        assert_eq!(
            resolve_in("tool", false, Some("/bin:/usr/bin"), None, &exists),
            Some(PathBuf::from("/usr/bin/tool"))
        );
        assert_eq!(resolve_in("/usr/bin/tool", false, None, None, &exists), Some(PathBuf::from("/usr/bin/tool")));
    }

    #[test]
    fn cmd_scripts_are_wrapped() {
        let exists = fs(&["C:\\node/npm.cmd", "C:\\git/git.exe"]);
        let path = Some("C:\\node;C:\\git");
        let argv = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let out = windows_argv(&argv(&["npm", "run", "build"]), path, Some(".EXE;.CMD"), Some("C:\\W\\cmd.exe"), &exists).unwrap();
        assert_eq!(out, argv(&["C:\\W\\cmd.exe", "/d", "/s", "/c", "C:\\node/npm.cmd", "run", "build"]));
        let out = windows_argv(&argv(&["git", "status"]), path, Some(".EXE;.CMD"), None, &exists).unwrap();
        assert_eq!(out, argv(&["C:\\git/git.exe", "status"]));
        // unresolved -> untouched
        let out = windows_argv(&argv(&["nope", "a b"]), path, None, None, &exists).unwrap();
        assert_eq!(out, argv(&["nope", "a b"]));
        // dangerous args to batch files are refused, but fine for .exe
        assert!(windows_argv(&argv(&["npm", "a&calc"]), path, Some(".EXE;.CMD"), None, &exists).is_err());
        assert!(windows_argv(&argv(&["npm", "two words"]), path, Some(".EXE;.CMD"), None, &exists).is_err());
        assert!(windows_argv(&argv(&["git", "two words"]), path, Some(".EXE;.CMD"), None, &exists).is_ok());
    }
}
