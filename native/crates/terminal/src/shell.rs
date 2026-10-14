//! Shell selection per OS (port of the `process.platform === 'win32' ? 'powershell.exe' : 'bash'`
//! logic, extended with pwsh / powershell / cmd fallback on Windows).

use std::path::{Path, PathBuf};

/// Operating system family used for shell selection. Exposed so selection can be unit tested for
/// every OS from any host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    Windows,
    MacOs,
    Unix,
}

impl Os {
    pub fn current() -> Os {
        if cfg!(windows) {
            Os::Windows
        } else if cfg!(target_os = "macos") {
            Os::MacOs
        } else {
            Os::Unix
        }
    }
}

/// Family of the shell, used for `cd` quoting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellKind {
    Posix,
    PowerShell,
    Cmd,
    Other,
}

/// A resolved shell program and its arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellSpec {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub kind: ShellKind,
}

impl ShellSpec {
    pub fn new(program: impl Into<PathBuf>, args: Vec<String>) -> Self {
        let program = program.into();
        let kind = kind_of(&program);
        ShellSpec { program, args, kind }
    }
}

fn kind_of(program: &Path) -> ShellKind {
    // Split on both separators so Windows-style paths classify correctly on any host.
    let name = program.to_string_lossy();
    let file = name.rsplit(['/', '\\']).next().unwrap_or("");
    let stem = file.rsplit_once('.').map_or(file, |(s, _)| s).to_ascii_lowercase();
    match stem.as_str() {
        "pwsh" | "powershell" => ShellKind::PowerShell,
        "cmd" => ShellKind::Cmd,
        "bash" | "sh" | "zsh" | "fish" | "dash" | "ash" | "ksh" | "tcsh" | "csh" | "nu" => {
            ShellKind::Posix
        }
        _ => ShellKind::Other,
    }
}

fn find_in_path(
    name: &str,
    os: Os,
    env: &dyn Fn(&str) -> Option<String>,
    is_file: &dyn Fn(&Path) -> bool,
) -> Option<PathBuf> {
    let path_var = env("PATH").or_else(|| env("Path"))?;
    let sep = if os == Os::Windows { ';' } else { ':' };
    path_var
        .split(sep)
        .filter(|d| !d.is_empty())
        .map(|d| Path::new(d).join(name))
        .find(|p| is_file(p))
}

/// Pure shell detection. `env` looks up environment variables and `is_file` checks existence, so
/// the logic can be tested without touching the real system.
///
/// * Windows: `pwsh.exe` -> `powershell.exe` -> `%COMSPEC%`/`cmd.exe`.
/// * macOS: `$SHELL` (login shell, `-l`) -> `/bin/zsh` -> `/bin/bash` -> `/bin/sh`.
/// * Other Unix: `$SHELL` -> `/bin/bash` -> `/usr/bin/bash` -> `/bin/sh`.
pub fn detect_shell(
    os: Os,
    env: &dyn Fn(&str) -> Option<String>,
    is_file: &dyn Fn(&Path) -> bool,
) -> ShellSpec {
    match os {
        Os::Windows => {
            for name in ["pwsh.exe", "powershell.exe"] {
                if let Some(p) = find_in_path(name, os, env, is_file) {
                    return ShellSpec::new(p, vec!["-NoLogo".into()]);
                }
            }
            let mut fixed: Vec<PathBuf> = Vec::new();
            if let Some(pf) = env("ProgramFiles") {
                fixed.push(Path::new(&pf).join("PowerShell").join("7").join("pwsh.exe"));
            }
            if let Some(root) = env("SystemRoot").or_else(|| env("windir")) {
                fixed.push(
                    Path::new(&root)
                        .join("System32")
                        .join("WindowsPowerShell")
                        .join("v1.0")
                        .join("powershell.exe"),
                );
            }
            if let Some(p) = fixed.into_iter().find(|p| is_file(p)) {
                return ShellSpec::new(p, vec!["-NoLogo".into()]);
            }
            let cmd = env("COMSPEC")
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "cmd.exe".to_string());
            ShellSpec::new(cmd, Vec::new())
        }
        Os::MacOs | Os::Unix => {
            let login_args = if os == Os::MacOs { vec!["-l".to_string()] } else { Vec::new() };
            if let Some(sh) = env("SHELL").filter(|s| !s.is_empty()) {
                if is_file(Path::new(&sh)) {
                    return ShellSpec::new(sh, login_args);
                }
            }
            let candidates: &[&str] = if os == Os::MacOs {
                &["/bin/zsh", "/bin/bash", "/bin/sh"]
            } else {
                &["/bin/bash", "/usr/bin/bash", "/bin/sh"]
            };
            for c in candidates {
                if is_file(Path::new(c)) {
                    return ShellSpec::new(*c, login_args);
                }
            }
            ShellSpec::new("/bin/sh", Vec::new())
        }
    }
}

/// Shell for the current OS using the real environment and filesystem.
pub fn default_shell() -> ShellSpec {
    detect_shell(Os::current(), &|k| std::env::var(k).ok(), &|p| p.is_file())
}

/// Build the line that changes the shell's directory (port of `setWorkingDirectory`).
pub(crate) fn cd_command(kind: ShellKind, path: &str) -> String {
    match kind {
        ShellKind::Cmd => format!("cd /d \"{path}\"\r"),
        ShellKind::PowerShell => format!("Set-Location -LiteralPath '{}'\r", path.replace('\'', "''")),
        ShellKind::Posix | ShellKind::Other => {
            format!("cd '{}'\r", path.replace('\'', "'\\''"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{HashMap, HashSet};

    fn env_of(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let m: HashMap<String, String> =
            pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        move |k| m.get(k).cloned()
    }

    fn files(paths: &[&str]) -> impl Fn(&Path) -> bool {
        let s: HashSet<PathBuf> = paths.iter().map(PathBuf::from).collect();
        move |p| s.contains(p)
    }

    #[test]
    fn windows_prefers_pwsh() {
        let env = env_of(&[("PATH", "C:\\a;C:\\b")]);
        let fs = files(&["C:\\b/pwsh.exe", "C:\\a/powershell.exe"]);
        let s = detect_shell(Os::Windows, &env, &fs);
        assert_eq!(s.program, PathBuf::from("C:\\b/pwsh.exe"));
        assert_eq!(s.kind, ShellKind::PowerShell);
    }

    #[test]
    fn windows_falls_back_to_powershell_then_cmd() {
        let env = env_of(&[("PATH", "C:\\a"), ("COMSPEC", "C:\\Windows\\System32\\cmd.exe")]);
        let s = detect_shell(Os::Windows, &env, &files(&["C:\\a/powershell.exe"]));
        assert_eq!(s.kind, ShellKind::PowerShell);
        let s = detect_shell(Os::Windows, &env, &files(&[]));
        assert_eq!(s.program, PathBuf::from("C:\\Windows\\System32\\cmd.exe"));
        assert_eq!(s.kind, ShellKind::Cmd);
        let s = detect_shell(Os::Windows, &env_of(&[]), &files(&[]));
        assert_eq!(s.program, PathBuf::from("cmd.exe"));
    }

    #[test]
    fn unix_uses_shell_env_then_bash_then_sh() {
        let env = env_of(&[("SHELL", "/usr/bin/fish")]);
        let s = detect_shell(Os::Unix, &env, &files(&["/usr/bin/fish", "/bin/bash"]));
        assert_eq!(s.program, PathBuf::from("/usr/bin/fish"));
        let s = detect_shell(Os::Unix, &env, &files(&["/bin/bash"]));
        assert_eq!(s.program, PathBuf::from("/bin/bash"));
        let s = detect_shell(Os::Unix, &env_of(&[]), &files(&[]));
        assert_eq!(s.program, PathBuf::from("/bin/sh"));
    }

    #[test]
    fn macos_is_login_shell() {
        let s = detect_shell(Os::MacOs, &env_of(&[("SHELL", "/bin/zsh")]), &files(&["/bin/zsh"]));
        assert_eq!(s.args, vec!["-l".to_string()]);
    }

    #[test]
    fn cd_quoting() {
        assert_eq!(cd_command(ShellKind::Posix, "/a b/it's"), "cd '/a b/it'\\''s'\r");
        assert_eq!(cd_command(ShellKind::Cmd, "C:\\x"), "cd /d \"C:\\x\"\r");
        assert!(cd_command(ShellKind::PowerShell, "C:\\x'y").contains("x''y"));
    }
}
