//! Port of `ProblemsService`: run the workspace's local `tsc` / `eslint` and parse the output.

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;

use super::parse::{parse_eslint_problems, parse_typescript_problems};
use super::types::{ProblemEntry, ProblemScanResult};
use crate::locale::locale_compare;
use crate::timeutil::now_iso;

struct ProcessResult {
    stdout: String,
    stderr: String,
    exit_code: Option<i32>,
}

/// Port of `resolveWorkspaceBinary`: `node_modules/.bin/<name>` (`.cmd` on Windows).
pub fn resolve_workspace_binary(root_path: &Path, name: &str) -> Option<PathBuf> {
    let extension = if cfg!(windows) { ".cmd" } else { "" };
    let binary = root_path
        .join("node_modules")
        .join(".bin")
        .join(format!("{name}{extension}"));
    binary.exists().then_some(binary)
}

fn run_process(binary: &Path, args: &[&str], cwd: &Path) -> io::Result<ProcessResult> {
    let mut command = Command::new(binary);
    command
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let output = command.output()?;
    Ok(ProcessResult {
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        exit_code: output.status.code(),
    })
}

/// Port of `sortProblems`.
fn sort_problems(mut entries: Vec<ProblemEntry>) -> Vec<ProblemEntry> {
    entries.sort_by(|left, right| {
        locale_compare(
            &left.file_path.to_string_lossy(),
            &right.file_path.to_string_lossy(),
        )
        .then_with(|| left.line.cmp(&right.line))
        .then_with(|| left.column.cmp(&right.column))
        .then_with(|| {
            let is_warning = |e: &ProblemEntry| e.severity == super::types::ProblemSeverity::Warning;
            is_warning(left).cmp(&is_warning(right))
        })
        .then_with(|| locale_compare(left.source.as_str(), right.source.as_str()))
    });
    entries
}

/// Port of `dedupeProblems`: keep the first entry per id.
fn dedupe_problems(entries: Vec<ProblemEntry>) -> Vec<ProblemEntry> {
    let mut seen = std::collections::HashSet::new();
    entries
        .into_iter()
        .filter(|entry| seen.insert(entry.id.clone()))
        .collect()
}

#[derive(Default)]
pub struct ProblemsService {
    last_results: Mutex<HashMap<PathBuf, ProblemScanResult>>,
}

impl ProblemsService {
    pub fn new() -> Self {
        Self::default()
    }

    fn empty_result(root_path: &Path) -> ProblemScanResult {
        ProblemScanResult {
            root_path: root_path.to_path_buf(),
            scanned_at: now_iso(),
            entries: Vec::new(),
            notes: Vec::new(),
            error: None,
        }
    }

    /// The result of the previous scan of `root_path`, or an empty result.
    pub fn get_last_result(&self, root_path: &Path) -> ProblemScanResult {
        self.last_results
            .lock()
            .ok()
            .and_then(|results| results.get(root_path).cloned())
            .unwrap_or_else(|| Self::empty_result(root_path))
    }

    /// Run `tsc --noEmit --pretty false` and `eslint . --format json` from the workspace's
    /// `node_modules/.bin` and collect the problems. Blocks until both tools finish.
    /// Failures are reported in `notes` / `error` of the result, never as `Err`.
    pub fn scan(&self, root_path: &Path) -> ProblemScanResult {
        let mut notes = Vec::new();
        let mut collected: Vec<ProblemEntry> = Vec::new();
        let mut error = None;

        if !root_path.is_absolute() {
            let result = ProblemScanResult {
                root_path: root_path.to_path_buf(),
                scanned_at: now_iso(),
                entries: Vec::new(),
                notes,
                error: Some(format!(
                    "workspace root must be an absolute path: {}",
                    root_path.display()
                )),
            };
            return result;
        }

        let outcome = (|| -> io::Result<()> {
            match resolve_workspace_binary(root_path, "tsc") {
                Some(tsc) => {
                    let result = run_process(&tsc, &["--noEmit", "--pretty", "false"], root_path)?;
                    collected.extend(parse_typescript_problems(
                        &format!("{}\n{}", result.stdout, result.stderr),
                        root_path,
                    ));
                    if result.exit_code.unwrap_or(0) > 1 && collected.is_empty() {
                        notes.push(
                            "TypeScript scan could not finish cleanly. Check tsconfig.json and local TypeScript dependencies."
                                .to_string(),
                        );
                    }
                }
                None => notes.push(
                    "Skipped TypeScript scan because this workspace has no local \"tsc\" binary."
                        .to_string(),
                ),
            }

            match resolve_workspace_binary(root_path, "eslint") {
                Some(eslint) => {
                    let result = run_process(&eslint, &[".", "--format", "json"], root_path)?;
                    let problems = parse_eslint_problems(&result.stdout, root_path);
                    let empty = problems.is_empty();
                    collected.extend(problems);
                    if result.exit_code.unwrap_or(0) > 1 && empty {
                        notes.push(
                            "ESLint scan could not finish cleanly. Check eslint config and local dependencies."
                                .to_string(),
                        );
                    }
                }
                None => notes.push(
                    "Skipped ESLint scan because this workspace has no local \"eslint\" binary."
                        .to_string(),
                ),
            }
            Ok(())
        })();
        if let Err(scan_error) = outcome {
            error = Some(scan_error.to_string());
        }

        let result = ProblemScanResult {
            root_path: root_path.to_path_buf(),
            scanned_at: now_iso(),
            entries: sort_problems(dedupe_problems(collected)),
            notes,
            error,
        };
        if let Ok(mut results) = self.last_results.lock() {
            results.insert(root_path.to_path_buf(), result.clone());
        }
        result
    }
}
