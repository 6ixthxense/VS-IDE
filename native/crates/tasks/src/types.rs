use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Where a task definition came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TaskSource {
    PackageScript,
    Custom,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskRunStatus {
    Idle,
    Running,
    Success,
    Error,
    Cancelled,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskDefinition {
    pub id: String,
    pub label: String,
    pub command: String,
    pub args: Vec<String>,
    pub source: TaskSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// Run through the platform shell. Defaults to `true` on Windows, `false` elsewhere.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shell: Option<bool>,
}

impl TaskDefinition {
    /// A custom task with no extras.
    pub fn custom(id: &str, label: &str, command: &str, args: &[&str]) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            command: command.into(),
            args: args.iter().map(|a| (*a).to_string()).collect(),
            source: TaskSource::Custom,
            cwd: None,
            detail: None,
            shell: None,
        }
    }

    /// Port of `buildTaskCommand`: `command arg1 arg2 ...`.
    pub fn command_line(&self) -> String {
        if self.args.is_empty() {
            self.command.clone()
        } else {
            format!("{} {}", self.command, self.args.join(" "))
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskRunRequest {
    pub terminal_id: String,
    pub task: TaskDefinition,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskRun {
    pub run_id: String,
    pub task_id: String,
    pub label: String,
    pub command: String,
    pub source: TaskSource,
    pub terminal_id: String,
    pub cwd: PathBuf,
    pub status: TaskRunStatus,
    /// ISO 8601 UTC timestamp.
    pub started_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<String>,
    /// `None` while running or when the process was killed by a signal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputStream {
    Stdout,
    Stderr,
}

/// Everything a task run reports through the channel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TaskUpdate {
    /// The run was created (`TaskEvent` with status `running`).
    Started { run: TaskRun, message: String },
    /// One line of output, without its line terminator.
    Output {
        run_id: String,
        terminal_id: String,
        stream: OutputStream,
        line: String,
    },
    /// The run finished (`success`, `error` or `cancelled`).
    Finished { run: TaskRun, message: String },
}

impl TaskUpdate {
    pub fn terminal_id(&self) -> &str {
        match self {
            TaskUpdate::Started { run, .. } | TaskUpdate::Finished { run, .. } => &run.terminal_id,
            TaskUpdate::Output { terminal_id, .. } => terminal_id,
        }
    }

    /// The text the Electron app wrote into the terminal for this update (ANSI colors,
    /// `\r\n` line endings): `[Task]` header, plain stdout, red stderr, `[Done]`,
    /// `[Cancelled]` or `[Failed]` footer.
    pub fn to_terminal_text(&self) -> String {
        match self {
            TaskUpdate::Started { run, .. } => format!(
                "\r\n\x1b[35m[Task] {}\x1b[0m\r\n\x1b[90m{}\x1b[0m\r\n",
                run.label, run.command
            ),
            TaskUpdate::Output { stream, line, .. } => match stream {
                OutputStream::Stdout => format!("{line}\r\n"),
                OutputStream::Stderr => format!("\x1b[31m{line}\r\n\x1b[0m"),
            },
            TaskUpdate::Finished { run, message } => match run.status {
                TaskRunStatus::Success => format!("\r\n\x1b[32m[Done] {message}\x1b[0m\r\n"),
                TaskRunStatus::Cancelled => format!("\r\n\x1b[33m[Cancelled] {message}\x1b[0m\r\n"),
                _ => format!("\r\n\x1b[31m[Failed] {message}\x1b[0m\r\n"),
            },
        }
    }
}
