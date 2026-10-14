//! Port of `renderer/utils/problems.ts`.

use std::cmp::Ordering;
use std::collections::HashMap;

use super::types::{ProblemEntry, ProblemScanResult, ProblemSeverity, ProblemSource};
use crate::locale::locale_compare;

fn problem_key(entry: &ProblemEntry) -> String {
    [
        entry.file_path.display().to_string(),
        entry.line.to_string(),
        entry.column.to_string(),
        entry.source.as_str().to_string(),
        entry.severity.as_str().to_string(),
        entry.code.clone().unwrap_or_default(),
        entry.message.clone(),
    ]
    .join(":")
}

fn compare_severity(left: ProblemSeverity, right: ProblemSeverity) -> Ordering {
    match (left, right) {
        (ProblemSeverity::Error, ProblemSeverity::Warning) => Ordering::Less,
        (ProblemSeverity::Warning, ProblemSeverity::Error) => Ordering::Greater,
        _ => Ordering::Equal,
    }
}

/// Merge scan results with live (language server) diagnostics. When live entries exist
/// they replace the scanned TypeScript entries. Duplicates (same key, later wins) are
/// removed and the result is sorted by severity, file, line, column and message.
pub fn merge_problem_entries(
    scan_entries: &[ProblemEntry],
    live_entries: &[ProblemEntry],
) -> Vec<ProblemEntry> {
    let base = scan_entries.iter().filter(|entry| {
        live_entries.is_empty() || entry.source != ProblemSource::Typescript
    });

    // JS Map semantics: first insertion fixes the position, later sets replace the value.
    let mut order: Vec<ProblemEntry> = Vec::new();
    let mut positions: HashMap<String, usize> = HashMap::new();
    for entry in base.chain(live_entries.iter()) {
        let key = problem_key(entry);
        match positions.get(&key) {
            Some(index) => order[*index] = entry.clone(),
            None => {
                positions.insert(key, order.len());
                order.push(entry.clone());
            }
        }
    }

    order.sort_by(|left, right| {
        compare_severity(left.severity, right.severity)
            .then_with(|| {
                locale_compare(
                    &left.file_path.to_string_lossy(),
                    &right.file_path.to_string_lossy(),
                )
            })
            .then_with(|| left.line.cmp(&right.line))
            .then_with(|| left.column.cmp(&right.column))
            .then_with(|| locale_compare(&left.message, &right.message))
    });
    order
}

pub fn count_problem_severity(entries: &[ProblemEntry], severity: ProblemSeverity) -> usize {
    entries.iter().filter(|entry| entry.severity == severity).count()
}

/// What the problems panel shows as "last scanned" (the TS code formats the scan time
/// with `toLocaleTimeString`; formatting for the user's locale is left to the UI).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProblemsTimestampLabel {
    /// ISO 8601 timestamp of the last scan.
    Scanned(String),
    Live,
    Never,
}

pub fn problems_timestamp_label(
    scan_result: Option<&ProblemScanResult>,
    live_entries: &[ProblemEntry],
) -> ProblemsTimestampLabel {
    match scan_result {
        Some(result) => ProblemsTimestampLabel::Scanned(result.scanned_at.clone()),
        None if !live_entries.is_empty() => ProblemsTimestampLabel::Live,
        None => ProblemsTimestampLabel::Never,
    }
}
