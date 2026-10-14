//! Problem matchers: tool output to [`ProblemEntry`] (ports of `parseTypeScriptProblems`
//! and `parseEslintProblems`).

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use regex::Regex;
use serde::Deserialize;
use serde_json::Value;

use super::types::{ProblemEntry, ProblemSeverity, ProblemSource};
use crate::paths;

fn typescript_line_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        // `^(.+)\((\d+),(\d+)\): (error|warning) (TS\d+): (.+)$`, case-insensitive.
        Regex::new(r"(?i)^(.+)\((\d+),(\d+)\): (error|warning) (TS\d+): (.+)$")
            .expect("static TypeScript problem regex is valid")
    })
}

/// Port of `createProblemId`: `source:file:line:column:code-or-message`.
pub fn create_problem_id(
    source: ProblemSource,
    file_path: &Path,
    line: u32,
    column: u32,
    message: &str,
    code: Option<&str>,
) -> String {
    format!(
        "{}:{}:{}:{}:{}",
        source.as_str(),
        file_path.display(),
        line,
        column,
        code.unwrap_or(message)
    )
}

/// Port of `resolveProblemFilePath`: `path.resolve(rootPath, rawPath.trim())`.
fn resolve_problem_file_path(root_path: &Path, raw_path: &str) -> PathBuf {
    paths::resolve(root_path, Path::new(raw_path.trim()))
}

/// Port of `normalizeSeverity`. Like the TS code, only the exact lowercase text
/// `warning` (or the number 1) is a warning; anything else is an error.
fn severity_from_text(value: &str) -> ProblemSeverity {
    if value == "warning" {
        ProblemSeverity::Warning
    } else {
        ProblemSeverity::Error
    }
}

fn severity_from_number(value: f64) -> ProblemSeverity {
    if value == 1.0 {
        ProblemSeverity::Warning
    } else {
        ProblemSeverity::Error
    }
}

/// Parse `tsc --pretty false` output. Lines that do not look like diagnostics are ignored.
pub fn parse_typescript_problems(output: &str, root_path: &Path) -> Vec<ProblemEntry> {
    let regex = typescript_line_regex();
    output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter_map(|line| {
            let caps = regex.captures(line)?;
            let file_path = resolve_problem_file_path(root_path, &caps[1]);
            let line_number: u32 = caps[2].parse().ok()?;
            let column: u32 = caps[3].parse().ok()?;
            let severity = severity_from_text(&caps[4]);
            let code = caps[5].to_string();
            let message = caps[6].to_string();

            Some(ProblemEntry {
                id: create_problem_id(
                    ProblemSource::Typescript,
                    &file_path,
                    line_number,
                    column,
                    &message,
                    Some(&code),
                ),
                file_path,
                line: line_number,
                column,
                message,
                source: ProblemSource::Typescript,
                severity,
                code: Some(code),
            })
        })
        .collect()
}

#[derive(Deserialize)]
struct EslintMessage {
    #[serde(default, rename = "ruleId")]
    rule_id: Option<String>,
    #[serde(default)]
    severity: f64,
    #[serde(default)]
    message: String,
    #[serde(default)]
    line: Option<f64>,
    #[serde(default)]
    column: Option<f64>,
}

#[derive(Deserialize)]
struct EslintEntry {
    #[serde(default, rename = "filePath")]
    file_path: String,
    #[serde(default)]
    messages: Vec<EslintMessage>,
}

fn position(value: Option<f64>) -> u32 {
    // Math.max(1, Number(value ?? 1))
    let number = value.unwrap_or(1.0);
    if number.is_nan() || number < 1.0 {
        1
    } else if number >= f64::from(u32::MAX) {
        u32::MAX
    } else {
        number as u32
    }
}

/// Parse `eslint --format json` output. Invalid JSON yields no problems.
pub fn parse_eslint_problems(output: &str, root_path: &Path) -> Vec<ProblemEntry> {
    let trimmed = output.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    let Ok(Value::Array(items)) = serde_json::from_str::<Value>(trimmed) else {
        return Vec::new();
    };

    let mut problems = Vec::new();
    for item in items {
        let Ok(entry) = serde_json::from_value::<EslintEntry>(item) else {
            continue;
        };
        let raw = Path::new(&entry.file_path);
        let file_path = if raw.is_absolute() {
            raw.to_path_buf()
        } else {
            resolve_problem_file_path(root_path, &entry.file_path)
        };

        for message in entry.messages.into_iter().filter(|m| m.severity > 0.0) {
            let line = position(message.line);
            let column = position(message.column);
            problems.push(ProblemEntry {
                id: create_problem_id(
                    ProblemSource::Eslint,
                    &file_path,
                    line,
                    column,
                    &message.message,
                    message.rule_id.as_deref(),
                ),
                file_path: file_path.clone(),
                line,
                column,
                severity: severity_from_number(message.severity),
                message: message.message,
                source: ProblemSource::Eslint,
                code: message.rule_id,
            });
        }
    }
    problems
}
