//! vsm-tasks: task runner, task detection and problem matchers.
//!
//! * [`TaskService`]: spawn a task, stream its output lines and final status through a
//!   channel of [`TaskUpdate`]s, and stop it together with all of its child processes
//!   (`taskkill /T /F` on Windows, process group + `killpg` on Unix).
//! * [`discover_package_script_tasks`]: `package.json` script detection.
//! * [`problems`]: `tsc` / `eslint` output parsers, the scan service and the
//!   live/scan merge helpers.

mod discovery;
mod error;
mod kill;
mod paths;
pub mod problems;
mod runner;
mod timeutil;
mod types;

pub use discovery::discover_package_script_tasks;
pub use error::{Result, TaskError};
pub use kill::{taskkill_args, taskkill_outcome};
pub use paths::{is_within, normalize_lexical, resolve};
pub use runner::{sanitize_task_request, TaskService};
pub use timeutil::{iso_from_millis, now_iso};
pub use types::{
    OutputStream, TaskDefinition, TaskRun, TaskRunRequest, TaskRunStatus, TaskSource, TaskUpdate,
};
