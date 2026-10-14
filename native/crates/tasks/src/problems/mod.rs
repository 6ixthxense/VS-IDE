//! Problem matchers and the problems scan service.

mod merge;
mod parse;
mod service;
mod types;

pub use merge::{
    count_problem_severity, merge_problem_entries, problems_timestamp_label, ProblemsTimestampLabel,
};
pub use parse::{create_problem_id, parse_eslint_problems, parse_typescript_problems};
pub use service::{resolve_workspace_binary, ProblemsService};
pub use types::{ProblemEntry, ProblemScanResult, ProblemSeverity, ProblemSource};
