//! Ports of `problemsService.test.ts` and `problemsUtils.test.ts`, plus scan tests.

use std::path::{Path, PathBuf};

use vsm_tasks::problems::{
    count_problem_severity, merge_problem_entries, parse_eslint_problems,
    parse_typescript_problems, problems_timestamp_label, ProblemEntry, ProblemScanResult,
    ProblemSeverity, ProblemSource, ProblemsService, ProblemsTimestampLabel,
};
use vsm_tasks::resolve;

fn root() -> PathBuf {
    // Equivalent of `path.resolve('C:/workspace/sample')`: absolute on every platform.
    std::env::temp_dir().join("workspace").join("sample")
}

fn resolved(root: &Path, relative: &str) -> PathBuf {
    resolve(root, Path::new(relative))
}

#[test]
fn parses_typescript_compiler_output_into_normalized_problem_entries() {
    let root = root();
    let output = [
        "src/app.ts(14,7): error TS2322: Type \"number\" is not assignable to type \"string\".",
        "src/util.ts(3,2): warning TS6133: \"unused\" is declared but its value is never read.",
        "This line should be ignored",
    ]
    .join("\n");

    let app = resolved(&root, "src/app.ts");
    let util = resolved(&root, "src/util.ts");
    assert_eq!(
        parse_typescript_problems(&output, &root),
        vec![
            ProblemEntry {
                id: format!("typescript:{}:14:7:TS2322", app.display()),
                file_path: app,
                line: 14,
                column: 7,
                message: "Type \"number\" is not assignable to type \"string\".".into(),
                source: ProblemSource::Typescript,
                severity: ProblemSeverity::Error,
                code: Some("TS2322".into()),
            },
            ProblemEntry {
                id: format!("typescript:{}:3:2:TS6133", util.display()),
                file_path: util,
                line: 3,
                column: 2,
                message: "\"unused\" is declared but its value is never read.".into(),
                source: ProblemSource::Typescript,
                severity: ProblemSeverity::Warning,
                code: Some("TS6133".into()),
            },
        ]
    );
}

#[test]
fn typescript_parser_handles_crlf_absolute_paths_and_uppercase_severity() {
    let root = root();
    let absolute = std::env::temp_dir().join("elsewhere").join("x.ts");
    let output = format!(
        "  {}(1,1): error TS1005: ';' expected.\r\nsrc/a b.ts(2,3): WARNING TS6133: odd casing\r\n",
        absolute.display()
    );
    let problems = parse_typescript_problems(&output, &root);
    assert_eq!(problems.len(), 2);
    assert_eq!(problems[0].file_path, absolute);
    assert_eq!(problems[1].file_path, resolved(&root, "src/a b.ts"));
    // Parity with the TS service: only the exact lowercase "warning" is a warning.
    assert_eq!(problems[1].severity, ProblemSeverity::Error);
    assert!(parse_typescript_problems("", &root).is_empty());
}

#[test]
fn parses_eslint_json_output_into_normalized_problem_entries() {
    let root = root();
    let output = serde_json::json!([
        {
            "filePath": "src/app.ts",
            "messages": [
                { "ruleId": "no-console", "severity": 1, "message": "Unexpected console statement.", "line": 4, "column": 3 },
                { "ruleId": "@typescript-eslint/no-unused-vars", "severity": 2, "message": "'value' is assigned a value but never used.", "line": 8, "column": 11 },
                { "ruleId": null, "severity": 0, "message": "ignored" }
            ]
        }
    ])
    .to_string();

    let app = resolved(&root, "src/app.ts");
    assert_eq!(
        parse_eslint_problems(&output, &root),
        vec![
            ProblemEntry {
                id: format!("eslint:{}:4:3:no-console", app.display()),
                file_path: app.clone(),
                line: 4,
                column: 3,
                message: "Unexpected console statement.".into(),
                source: ProblemSource::Eslint,
                severity: ProblemSeverity::Warning,
                code: Some("no-console".into()),
            },
            ProblemEntry {
                id: format!("eslint:{}:8:11:@typescript-eslint/no-unused-vars", app.display()),
                file_path: app,
                line: 8,
                column: 11,
                message: "'value' is assigned a value but never used.".into(),
                source: ProblemSource::Eslint,
                severity: ProblemSeverity::Error,
                code: Some("@typescript-eslint/no-unused-vars".into()),
            },
        ]
    );
}

#[test]
fn eslint_parser_is_lenient() {
    let root = root();
    assert!(parse_eslint_problems("", &root).is_empty());
    assert!(parse_eslint_problems("   \n", &root).is_empty());
    assert!(parse_eslint_problems("not json", &root).is_empty());
    assert!(parse_eslint_problems("{\"a\":1}", &root).is_empty());

    // Missing line/column default to 1, ids fall back to the message without a rule.
    let absolute = std::env::temp_dir().join("abs.js");
    let output = serde_json::json!([{
        "filePath": absolute,
        "messages": [{ "ruleId": null, "severity": 2, "message": "Parsing error", "line": 0 }]
    }])
    .to_string();
    let problems = parse_eslint_problems(&output, &root);
    assert_eq!(problems.len(), 1);
    assert_eq!(problems[0].file_path, absolute);
    assert_eq!((problems[0].line, problems[0].column), (1, 1));
    assert_eq!(problems[0].code, None);
    assert_eq!(problems[0].id, format!("eslint:{}:1:1:Parsing error", absolute.display()));
}

// --- renderer utils -------------------------------------------------------

fn entry(id: &str) -> ProblemEntry {
    ProblemEntry {
        id: id.into(),
        file_path: "C:/repo/src/index.ts".into(),
        line: 1,
        column: 1,
        message: "Example problem".into(),
        source: ProblemSource::Typescript,
        severity: ProblemSeverity::Error,
        code: None,
    }
}

#[test]
fn replaces_scanned_typescript_entries_with_live_typescript_diagnostics() {
    let scan = vec![
        ProblemEntry { message: "Old ts issue".into(), ..entry("scan-ts") },
        ProblemEntry {
            source: ProblemSource::Eslint,
            message: "Lint issue".into(),
            ..entry("scan-eslint")
        },
    ];
    let live = vec![ProblemEntry { message: "Live ts issue".into(), ..entry("live-ts") }];

    assert_eq!(
        merge_problem_entries(&scan, &live),
        vec![scan[1].clone(), live[0].clone()]
    );
}

#[test]
fn keeps_scanned_entries_intact_when_no_live_diagnostics_exist() {
    let scan = vec![
        ProblemEntry { message: "TS issue".into(), ..entry("scan-ts") },
        ProblemEntry {
            source: ProblemSource::Eslint,
            message: "Lint issue".into(),
            severity: ProblemSeverity::Warning,
            ..entry("scan-eslint")
        },
    ];
    assert_eq!(merge_problem_entries(&scan, &[]), scan);
}

#[test]
fn merge_dedupes_and_sorts() {
    let a = ProblemEntry { file_path: "b.ts".into(), ..entry("a") };
    let b = ProblemEntry { file_path: "a.ts".into(), line: 9, ..entry("b") };
    let c = ProblemEntry { file_path: "a.ts".into(), line: 2, severity: ProblemSeverity::Warning, ..entry("c") };
    let merged = merge_problem_entries(&[a.clone(), b.clone(), c.clone(), a.clone()], &[]);
    // Errors first, then by file and line; the duplicate of `a` is gone.
    assert_eq!(merged, vec![b, a, c]);
}

#[test]
fn counts_severities_from_a_merged_set() {
    let entries = vec![
        entry("problem"),
        ProblemEntry { severity: ProblemSeverity::Warning, line: 2, ..entry("warning") },
    ];
    assert_eq!(count_problem_severity(&entries, ProblemSeverity::Error), 1);
    assert_eq!(count_problem_severity(&entries, ProblemSeverity::Warning), 1);
}

#[test]
fn timestamp_label_reflects_scan_or_live_state() {
    assert_eq!(
        problems_timestamp_label(None, &[entry("x")]),
        ProblemsTimestampLabel::Live
    );
    assert_eq!(problems_timestamp_label(None, &[]), ProblemsTimestampLabel::Never);
    let scan = ProblemScanResult {
        root_path: "/w".into(),
        scanned_at: "2024-01-01T00:00:00.000Z".into(),
        entries: vec![],
        notes: vec![],
        error: None,
    };
    assert_eq!(
        problems_timestamp_label(Some(&scan), &[]),
        ProblemsTimestampLabel::Scanned("2024-01-01T00:00:00.000Z".into())
    );
}

// --- scan service ---------------------------------------------------------

#[test]
fn scan_without_local_binaries_adds_notes_and_caches_the_result() {
    let workspace = tempfile::tempdir().expect("tempdir");
    let service = ProblemsService::new();

    let before = service.get_last_result(workspace.path());
    assert!(before.entries.is_empty() && before.notes.is_empty());

    let result = service.scan(workspace.path());
    assert!(result.entries.is_empty());
    assert_eq!(
        result.notes,
        vec![
            "Skipped TypeScript scan because this workspace has no local \"tsc\" binary.",
            "Skipped ESLint scan because this workspace has no local \"eslint\" binary.",
        ]
    );
    assert!(result.error.is_none());
    assert_eq!(service.get_last_result(workspace.path()), result);
}

#[cfg(unix)]
mod unix_scan {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    fn install_binary(root: &Path, name: &str, script: &str) {
        let bin = root.join("node_modules").join(".bin");
        fs::create_dir_all(&bin).expect("mkdir");
        let path = bin.join(name);
        fs::write(&path, format!("#!/bin/sh\n{script}\n")).expect("write");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("chmod");
    }

    /// A freshly written script can hit ETXTBSY ("Text file busy") when another test
    /// thread forks while the write handle is still open; retry that harmless race.
    fn scan_with_retry(root: &Path) -> ProblemScanResult {
        for _ in 0..20 {
            let result = ProblemsService::new().scan(root);
            if !result.error.as_deref().is_some_and(|e| e.contains("busy")) {
                return result;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        ProblemsService::new().scan(root)
    }

    #[test]
    fn scan_runs_local_tools_merges_dedupes_and_sorts() {
        let workspace = tempfile::tempdir().expect("tempdir");
        let root = workspace.path();
        install_binary(
            root,
            "tsc",
            r#"echo "src/b.ts(5,1): error TS2322: bad type."
echo "src/a.ts(1,1): warning TS6133: unused." 1>&2
echo "src/b.ts(5,1): error TS2322: bad type."
exit 2"#,
        );
        install_binary(
            root,
            "eslint",
            r#"echo '[{"filePath":"src/a.ts","messages":[{"ruleId":"no-console","severity":1,"message":"console","line":1,"column":1}]}]'
exit 1"#,
        );

        let result = scan_with_retry(root);

        assert!(result.error.is_none(), "{:?}", result.error);
        assert!(result.notes.is_empty(), "{:?}", result.notes);
        let summary: Vec<(String, u32, &str, &str)> = result
            .entries
            .iter()
            .map(|e| (
                e.file_path.file_name().expect("name").to_string_lossy().into_owned(),
                e.line,
                e.source.as_str(),
                e.severity.as_str(),
            ))
            .collect();
        // Sorted by file, line, column, errors before warnings, then source;
        // the duplicate tsc line is removed.
        assert_eq!(
            summary,
            vec![
                ("a.ts".to_string(), 1, "eslint", "warning"),
                ("a.ts".to_string(), 1, "typescript", "warning"),
                ("b.ts".to_string(), 5, "typescript", "error"),
            ]
        );
    }

    #[test]
    fn scan_notes_tools_that_fail_without_output() {
        let workspace = tempfile::tempdir().expect("tempdir");
        let root = workspace.path();
        install_binary(root, "tsc", "echo 'error TS5083: Cannot read file tsconfig.json.'\nexit 2");
        install_binary(root, "eslint", "echo 'Oops! Something went wrong!' 1>&2\nexit 2");

        let result = scan_with_retry(root);
        assert!(result.entries.is_empty());
        assert_eq!(result.notes.len(), 2);
        assert!(result.notes[0].starts_with("TypeScript scan could not finish cleanly"));
        assert!(result.notes[1].starts_with("ESLint scan could not finish cleanly"));
    }

    #[test]
    fn scan_reports_spawn_failures_in_the_error_field() {
        let workspace = tempfile::tempdir().expect("tempdir");
        let root = workspace.path();
        // Exists but is not executable.
        let bin = root.join("node_modules").join(".bin");
        fs::create_dir_all(&bin).expect("mkdir");
        fs::write(bin.join("tsc"), "not executable").expect("write");

        let result = ProblemsService::new().scan(root);
        assert!(result.error.is_some());
        assert!(result.entries.is_empty());
    }
}

#[test]
fn scan_rejects_relative_roots() {
    let result = ProblemsService::new().scan(Path::new("relative/root"));
    assert!(result.entries.is_empty());
    assert!(result.error.expect("error").contains("absolute"));
}
