//! Thin wrapper around `std::process::Command` for the git CLI.

use std::ffi::OsStr;
use std::path::Path;
use std::process::{Command, Output, Stdio};

use crate::{GitError, Result};

pub(crate) struct Run {
    pub stdout: Vec<u8>,
    pub stderr: String,
    pub success: bool,
    pub code: String,
}

impl Run {
    pub fn stdout_string(&self) -> String {
        String::from_utf8_lossy(&self.stdout).into_owned()
    }
}

fn base_command(root: &Path) -> Command {
    let mut command = Command::new("git");
    // `--no-optional-locks`: never take the index lock for read-only commands, so
    // polling does not race with a user commit.
    // `core.quotepath=off`: print non-ASCII paths verbatim (Thai file names!).
    command
        .arg("--no-optional-locks")
        // Treat every pathspec literally: file names may contain `*`, `[`, `:` ...
        .arg("--literal-pathspecs")
        .args(["-c", "core.quotepath=off"])
        // Opening an untrusted repository must not run a hook program from its config
        // (`core.fsmonitor` is executed by `git status`).
        .args(["-c", "core.fsmonitor=false"])
        .arg("-C")
        .arg(root)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("LC_ALL", "C")
        .stdin(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW
        command.creation_flags(0x0800_0000);
    }
    command
}

fn finish(output: Output) -> Run {
    let code = output
        .status
        .code()
        .map(|c| c.to_string())
        .unwrap_or_else(|| "signal".to_string());
    Run {
        success: output.status.success(),
        stdout: output.stdout,
        stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        code,
    }
}

/// Run git and return the raw result regardless of exit status.
pub(crate) fn run_raw<I, S>(root: &Path, args: I) -> Result<Run>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut command = base_command(root);
    command.args(args);
    Ok(finish(command.output()?))
}

/// Run git and fail with [`GitError::Command`] on a non-zero exit status.
pub(crate) fn run<I, S>(root: &Path, args: I) -> Result<Run>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let args: Vec<_> = args.into_iter().collect();
    let result = run_raw(root, args.iter())?;
    if result.success {
        return Ok(result);
    }
    let lowered = result.stderr.to_lowercase();
    if lowered.contains("not a git repository") {
        return Err(GitError::NotARepo(root.display().to_string()));
    }
    let command = args
        .first()
        .map(|a| a.as_ref().to_string_lossy().into_owned())
        .unwrap_or_default();
    // Some failures (commit with nothing staged) only print to stdout.
    let stderr = if result.stderr.is_empty() {
        result.stdout_string().trim().to_string()
    } else {
        result.stderr.clone()
    };
    Err(GitError::Command {
        command,
        code: result.code,
        stderr,
    })
}
