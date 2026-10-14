//! Port of `src/test/main/searchService.test.ts` plus coverage for the native additions
//! (streaming, cancellation, scoping, result cap, file skipping rules).

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Mutex;

use tempfile::TempDir;
use vsm_search::{
    build_replace_preview, replace_in_workspace, search_streaming, search_workspace, search_workspace_cancellable,
    spawn_search, SearchError, SearchOptions, SearchRequest,
};

fn fixture() -> TempDir {
    let root = tempfile::Builder::new()
        .prefix("vs-monitor-ide-search-")
        .tempdir()
        .expect("tempdir");
    let p = root.path();
    fs::create_dir(p.join("src")).expect("mkdir");
    fs::create_dir(p.join("docs")).expect("mkdir");
    fs::create_dir(p.join("dist")).expect("mkdir");
    fs::write(
        p.join("src").join("app.tsx"),
        [
            "const HelloWorld = \"Hello\";",
            "const helloWorld = \"hello\";",
            "console.log(helloWorld);",
        ]
        .join("\n"),
    )
    .expect("write");
    fs::write(p.join("docs").join("readme.md"), "hello from docs\n").expect("write");
    fs::write(p.join("dist").join("bundle.js"), "hello from dist\n").expect("write");
    root
}

fn request(root: &Path, query: &str, options: SearchOptions) -> SearchRequest {
    SearchRequest::new(root, query, options)
}

#[test]
fn filters_search_results_with_include_exclude_patterns_and_whole_word_matching() {
    let root = fixture();
    let results = search_workspace(&request(
        root.path(),
        "hello",
        SearchOptions {
            whole_word: true,
            case_sensitive: false,
            include_pattern: "src/*".into(),
            exclude_pattern: "*.md".into(),
            ..Default::default()
        },
    ))
    .expect("search");

    assert_eq!(results.len(), 2);
    assert!(results
        .iter()
        .all(|r| r.path.ends_with(Path::new("src").join("app.tsx"))));
    assert_eq!(results[0].line, 1);
    assert_eq!(results[0].text, "const HelloWorld = \"Hello\";");
    assert_eq!(results[1].line, 2);
    assert_eq!(results[0].name, "app.tsx");
}

#[test]
fn replaces_matches_using_regex_filters_while_skipping_excluded_paths() {
    let root = fixture();
    let cancel = AtomicBool::new(false);
    let count = replace_in_workspace(
        &request(
            root.path(),
            "hello(world)?",
            SearchOptions {
                use_regex: true,
                case_sensitive: false,
                exclude_pattern: "dist/*, docs/*".into(),
                ..Default::default()
            },
        ),
        "updated",
        &cancel,
    )
    .expect("replace");

    assert_eq!(count.changed_files, 1);
    assert!(count.failures.is_empty());
    let app = fs::read_to_string(root.path().join("src").join("app.tsx")).expect("read");
    assert!(app.contains("updated"));
    assert!(!app.to_lowercase().contains("hello"));
    let readme = fs::read_to_string(root.path().join("docs").join("readme.md")).expect("read");
    assert!(readme.contains("hello from docs"));
}

#[test]
fn replacement_uses_js_templates_like_the_preview() {
    let root = fixture();
    let count = replace_in_workspace(
        &request(root.path(), "hello", SearchOptions::default()).with_target(root.path().join("docs")),
        "$& $1",
        &AtomicBool::new(false),
    )
    .expect("replace");
    assert_eq!(count.changed_files, 1);
    // Same JS template semantics as the preview: `$&` is the match, `$1` has no group
    // and stays literal.
    let readme = fs::read_to_string(root.path().join("docs").join("readme.md")).expect("read");
    assert_eq!(readme, "hello $1 from docs\n");
}

#[test]
fn empty_query_returns_nothing_and_replace_rejects_it() {
    let root = fixture();
    let req = request(root.path(), "   ", SearchOptions::default());
    assert!(search_workspace(&req).expect("search").is_empty());
    assert!(replace_in_workspace(&req, "x", &AtomicBool::new(false)).is_err());
}

#[test]
fn invalid_regex_is_an_error() {
    let root = fixture();
    let req = request(
        root.path(),
        "(",
        SearchOptions {
            use_regex: true,
            ..Default::default()
        },
    );
    assert!(matches!(
        search_workspace(&req),
        Err(SearchError::InvalidPattern(_))
    ));
}

#[test]
fn skips_ignored_directories_but_not_dotfiles() {
    let root = fixture();
    let p = root.path();
    for dir in ["node_modules", ".git", ".hidden", "dist-electron", "dist-renderer"] {
        fs::create_dir(p.join(dir)).expect("mkdir");
        fs::write(p.join(dir).join("x.txt"), "needle\n").expect("write");
    }
    fs::write(p.join(".env"), "needle=1\n").expect("write");
    fs::create_dir_all(p.join("src").join("nested")).expect("mkdir");
    fs::write(p.join("src").join("nested").join("deep.txt"), "a needle here").expect("write");

    let results = search_workspace(&request(p, "needle", SearchOptions::default())).expect("search");
    let mut names: Vec<String> = results.iter().map(|r| r.name.clone()).collect();
    names.sort();
    assert_eq!(names, vec![".env", "deep.txt"]);
}

#[test]
fn caps_results_in_sorted_file_order() {
    let root = tempfile::tempdir().expect("tempdir");
    for i in 0..30 {
        let body: String = (0..20).map(|n| format!("line {n} needle\n")).collect();
        fs::write(root.path().join(format!("f{i:02}.txt")), body).expect("write");
    }
    let results =
        search_workspace(&request(root.path(), "needle", SearchOptions::default())).expect("search");
    assert_eq!(results.len(), 200);
    // 20 matches per file, files in sorted order: the first 10 files, line order inside.
    for (index, result) in results.iter().enumerate() {
        assert_eq!(result.name, format!("f{:02}.txt", index / 20));
        assert_eq!(result.line, (index % 20 + 1) as u64);
    }

    let limited = search_workspace(
        &request(root.path(), "needle", SearchOptions::default()).with_max_results(5),
    )
    .expect("search");
    assert_eq!(limited.len(), 5);
}

#[test]
fn scopes_to_a_directory_or_a_single_file() {
    let root = fixture();
    let p = root.path();
    let in_docs = search_workspace(
        &request(p, "hello", SearchOptions::default()).with_target(p.join("docs")),
    )
    .expect("search");
    assert_eq!(in_docs.len(), 1);
    assert_eq!(in_docs[0].name, "readme.md");

    let single = search_workspace(
        &request(p, "hello", SearchOptions::default()).with_target(p.join("src").join("app.tsx")),
    )
    .expect("search");
    assert_eq!(single.len(), 3);
}

#[test]
fn scoping_respects_filters_and_skipped_directories() {
    let root = fixture();
    let p = root.path();
    // Target inside a skipped directory yields nothing, like the index-based TS lookup.
    let dist = search_workspace(
        &request(p, "hello", SearchOptions::default()).with_target(p.join("dist")),
    )
    .expect("search");
    assert!(dist.is_empty());

    // A single-file target is still subject to include/exclude globs.
    let excluded = search_workspace(
        &request(
            p,
            "hello",
            SearchOptions {
                exclude_pattern: "*.md".into(),
                ..Default::default()
            },
        )
        .with_target(p.join("docs").join("readme.md")),
    )
    .expect("search");
    assert!(excluded.is_empty());

    let outside = tempfile::tempdir().expect("tempdir");
    assert!(matches!(
        search_workspace(
            &request(p, "hello", SearchOptions::default()).with_target(outside.path())
        ),
        Err(SearchError::OutsideRoot { .. })
    ));
}

#[test]
fn matches_do_not_span_lines_and_handle_crlf() {
    let root = tempfile::tempdir().expect("tempdir");
    fs::write(root.path().join("a.txt"), "foo\r\nbar\r\n  baz  \r\n").expect("write");
    let regex = SearchOptions {
        use_regex: true,
        ..Default::default()
    };
    let hits = search_workspace(&request(root.path(), "^\\s*ba.\\s*$", regex.clone())).expect("search");
    assert_eq!(
        hits.iter().map(|h| (h.line, h.text.as_str())).collect::<Vec<_>>(),
        vec![(2, "bar"), (3, "baz")]
    );
    // A pattern that needs a newline can never match a single line.
    assert!(search_workspace(&request(root.path(), "foo\\nbar", regex)).is_err());
}

#[test]
fn binary_and_non_utf8_files() {
    let root = tempfile::tempdir().expect("tempdir");
    fs::write(root.path().join("bin.dat"), b"needle\0\x01\x02 needle").expect("write");
    fs::write(root.path().join("latin1.txt"), b"caf\xe9 needle\n").expect("write");
    let hits = search_workspace(&request(root.path(), "needle", SearchOptions::default())).expect("search");
    let names: Vec<&str> = hits.iter().map(|h| h.name.as_str()).collect();
    assert!(names.contains(&"latin1.txt"));
    assert!(!names.contains(&"bin.dat"));
}

#[test]
fn streaming_delivers_all_matches_and_reports_a_summary() {
    let root = fixture();
    let collected = Mutex::new(Vec::new());
    let cancel = AtomicBool::new(false);
    let summary = search_streaming(
        &request(root.path(), "hello", SearchOptions::default()),
        &cancel,
        |m| collected.lock().expect("lock").push(m),
    )
    .expect("search");

    let mut paths: Vec<PathBuf> = collected
        .into_inner()
        .expect("lock")
        .into_iter()
        .map(|m| m.path)
        .collect();
    paths.sort();
    // app.tsx lines 1+2 (line 3 has "helloWorld" which also matches), docs/readme.md;
    // dist is skipped.
    assert_eq!(summary.matches, 4);
    assert_eq!(paths.len(), 4);
    assert!(!summary.truncated);
    assert!(!summary.cancelled);
}

#[test]
fn streaming_honours_the_result_cap() {
    let root = tempfile::tempdir().expect("tempdir");
    for i in 0..20 {
        fs::write(root.path().join(format!("f{i}.txt")), "needle\n".repeat(50)).expect("write");
    }
    let count = Mutex::new(0usize);
    let summary = search_streaming(
        &request(root.path(), "needle", SearchOptions::default()).with_max_results(37),
        &AtomicBool::new(false),
        |_| *count.lock().expect("lock") += 1,
    )
    .expect("search");
    assert_eq!(*count.lock().expect("lock"), 37);
    assert_eq!(summary.matches, 37);
    assert!(summary.truncated);
}

#[test]
fn cancellation_stops_the_search() {
    let root = fixture();
    let cancel = AtomicBool::new(true);
    let req = request(root.path(), "hello", SearchOptions::default());
    assert!(matches!(
        search_workspace_cancellable(&req, &cancel),
        Err(SearchError::Cancelled)
    ));
    let summary = search_streaming(&req, &cancel, |_| panic!("no matches expected")).expect("search");
    assert!(summary.cancelled);
    assert_eq!(summary.matches, 0);
    assert!(matches!(
        replace_in_workspace(&req, "x", &cancel),
        Err(SearchError::Cancelled)
    ));
}

#[test]
fn cancelling_mid_stream_is_prompt() {
    let root = tempfile::tempdir().expect("tempdir");
    for i in 0..50 {
        fs::write(root.path().join(format!("f{i}.txt")), "needle\n".repeat(1000)).expect("write");
    }
    let handle = spawn_search(
        request(root.path(), "needle", SearchOptions::default()).with_max_results(usize::MAX),
    );
    let first = handle.receiver.recv().expect("first match");
    assert_eq!(first.text, "needle");
    handle.cancel();
    let summary = handle.join().expect("join");
    assert!(summary.cancelled);
    assert!(summary.matches < 50_000);
}

#[test]
fn spawned_search_closes_the_channel_when_done() {
    let root = fixture();
    let handle = spawn_search(request(root.path(), "hello", SearchOptions::default()));
    let received: Vec<_> = handle.receiver.iter().collect();
    assert_eq!(received.len(), 4);
    let summary = handle.join().expect("join");
    assert_eq!(summary.matches, 4);
}

#[test]
fn replace_preview_equals_the_applied_result() {
    let root = tempfile::tempdir().expect("tempdir");
    let line = "key=value and other=thing";
    fs::write(root.path().join("a.txt"), line).expect("write");
    let options = SearchOptions {
        use_regex: true,
        ..Default::default()
    };
    let preview = build_replace_preview(line, "(\\w+)=(\\w+)", "$2=$1 [$&]", &options);
    let report = replace_in_workspace(
        &request(root.path(), "(\\w+)=(\\w+)", options),
        "$2=$1 [$&]",
        &AtomicBool::new(false),
    )
    .expect("replace");
    assert_eq!(report.changed_files, 1);
    assert_eq!(fs::read_to_string(root.path().join("a.txt")).expect("read"), preview);
}

#[test]
fn replace_skips_files_with_nul_bytes_that_search_hides() {
    let root = tempfile::tempdir().expect("tempdir");
    fs::write(root.path().join("bin.dat"), b"needle\0needle").expect("write");
    fs::write(root.path().join("a.txt"), "needle").expect("write");
    let report = replace_in_workspace(
        &request(root.path(), "needle", SearchOptions::default()),
        "x",
        &AtomicBool::new(false),
    )
    .expect("replace");
    assert_eq!(report.changed_files, 1);
    assert_eq!(fs::read(root.path().join("bin.dat")).expect("read"), b"needle\0needle");
}

#[test]
fn nul_late_in_a_big_file_drops_the_whole_file() {
    let root = tempfile::tempdir().expect("tempdir");
    let mut body = "needle line\n".repeat(200_000); // ~2.4 MB, well beyond one read buffer
    body.push('\0');
    fs::write(root.path().join("big.bin"), body).expect("write");
    fs::write(root.path().join("ok.txt"), "needle\n").expect("write");

    let hits = search_workspace(&request(root.path(), "needle", SearchOptions::default()))
        .expect("search");
    assert_eq!(hits.iter().map(|h| h.name.as_str()).collect::<Vec<_>>(), vec!["ok.txt"]);

    let streamed = Mutex::new(Vec::new());
    let summary = search_streaming(
        &request(root.path(), "needle", SearchOptions::default()),
        &AtomicBool::new(false),
        |m| streamed.lock().expect("lock").push(m.name),
    )
    .expect("search");
    assert_eq!(streamed.into_inner().expect("lock"), vec!["ok.txt"]);
    assert_eq!(summary.matches, 1);
}

#[test]
fn truncated_is_only_set_when_a_match_was_actually_refused() {
    let root = tempfile::tempdir().expect("tempdir");
    for name in ["a.txt", "b.txt", "c.txt"] {
        fs::write(root.path().join(name), "needle\n").expect("write");
    }
    let run = |cap: usize| {
        search_streaming(
            &request(root.path(), "needle", SearchOptions::default()).with_max_results(cap),
            &AtomicBool::new(false),
            |_| {},
        )
        .expect("search")
    };
    let exact = run(3);
    assert_eq!((exact.matches, exact.truncated), (3, false));
    let over = run(2);
    assert_eq!((over.matches, over.truncated), (2, true));
    let roomy = run(10);
    assert_eq!((roomy.matches, roomy.truncated), (3, false));
}

#[test]
fn relative_roots_are_rejected() {
    let req = request(Path::new("relative/root"), "x", SearchOptions::default());
    assert!(matches!(
        search_workspace(&req),
        Err(SearchError::RootNotAbsolute(_))
    ));
    assert!(matches!(
        search_streaming(&req, &AtomicBool::new(false), |_| {}),
        Err(SearchError::RootNotAbsolute(_))
    ));
    assert!(matches!(
        replace_in_workspace(&req, "y", &AtomicBool::new(false)),
        Err(SearchError::RootNotAbsolute(_))
    ));
}

#[test]
fn replace_reports_cancellation_and_leaves_no_temp_files() {
    let root = fixture();
    let report = replace_in_workspace(
        &request(root.path(), "hello", SearchOptions::default()),
        "bye",
        &AtomicBool::new(false),
    )
    .expect("replace");
    assert_eq!(report.changed_files, 2); // src/app.tsx and docs/readme.md (dist is skipped)
    assert!(report.failures.is_empty() && !report.cancelled);
    for dir in ["src", "docs"] {
        let entries: Vec<_> = fs::read_dir(root.path().join(dir)).expect("dir").collect();
        assert_eq!(entries.len(), 1, "temp file left in {dir}");
    }
}
