use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProblemSeverity {
    Error,
    Warning,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProblemSource {
    Typescript,
    Eslint,
}

impl ProblemSource {
    pub fn as_str(self) -> &'static str {
        match self {
            ProblemSource::Typescript => "typescript",
            ProblemSource::Eslint => "eslint",
        }
    }
}

impl ProblemSeverity {
    pub fn as_str(self) -> &'static str {
        match self {
            ProblemSeverity::Error => "error",
            ProblemSeverity::Warning => "warning",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProblemEntry {
    pub id: String,
    pub file_path: PathBuf,
    pub line: u32,
    pub column: u32,
    pub message: String,
    pub source: ProblemSource,
    pub severity: ProblemSeverity,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProblemScanResult {
    pub root_path: PathBuf,
    /// ISO 8601 UTC timestamp.
    pub scanned_at: String,
    pub entries: Vec<ProblemEntry>,
    #[serde(default)]
    pub notes: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}
