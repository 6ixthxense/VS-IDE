//! Port of `src/test/main/tasksService.test.ts`, extended with real process tests.

use std::fs;
use std::path::Path;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use vsm_tasks::{
    discover_package_script_tasks, sanitize_task_request, taskkill_args, OutputStream,
    TaskDefinition, TaskError, TaskRunRequest, TaskRunStatus, TaskService, TaskSource, TaskUpdate,
};

fn workspace_with_package_json(scripts: &[(&str, &str)]) -> tempfile::TempDir {
    let root = tempfile::Builder::new()
        .prefix("vs-monitor-tasks-")
        .tempdir()
        .expect("tempdir");
    let scripts: serde_json::Map<String, serde_json::Value> = scripts
        .iter()
        .map(|(name, command)| ((*name).to_string(), serde_json::json!(command)))
        .collect();
    let package = serde_json::json!({ "name": "sample", "scripts": scripts });
    fs::write(
        root.path().join("package.json"),
        serde_json::to_string_pretty(&package).expect("json"),
    )
    .expect("write");
    root
}

#[test]
fn returns_npm_script_definitions_from_package_json() {
    let root = workspace_with_package_json(&[
        ("test", "vitest run"),
        ("build", "vite build"),
        ("dev", "vite"),
    ]);

    let tasks = discover_package_script_tasks(root.path());
    let npm = if cfg!(windows) { "npm.cmd" } else { "npm" };

    let expected = |name: &str, detail: &str| TaskDefinition {
        id: format!("npm:{name}"),
        label: name.to_string(),
        command: npm.to_string(),
        args: vec!["run".to_string(), name.to_string()],
        source: TaskSource::PackageScript,
        cwd: Some(root.path().to_path_buf()),
        detail: Some(detail.to_string()),
        shell: Some(cfg!(windows)),
    };
    assert_eq!(
        tasks,
        vec![
            expected("build", "vite build"),
            expected("dev", "vite"),
            expected("test", "vitest run"),
        ]
    );
}

#[test]
fn returns_an_empty_list_when_package_json_is_missing_or_invalid() {
    let empty = tempfile::tempdir().expect("tempdir");
    assert!(discover_package_script_tasks(empty.path()).is_empty());

    let invalid = tempfile::tempdir().expect("tempdir");
    fs::write(invalid.path().join("package.json"), "{ invalid json").expect("write");
    assert!(discover_package_script_tasks(invalid.path()).is_empty());

    let no_scripts = tempfile::tempdir().expect("tempdir");
    fs::write(no_scripts.path().join("package.json"), r#"{"name":"x"}"#).expect("write");
    assert!(discover_package_script_tasks(no_scripts.path()).is_empty());

    let not_object = tempfile::tempdir().expect("tempdir");
    fs::write(not_object.path().join("package.json"), "null").expect("write");
    assert!(discover_package_script_tasks(not_object.path()).is_empty());
}

#[test]
fn task_definitions_round_trip_through_service_listing() {
    let root = workspace_with_package_json(&[("lint", "eslint .")]);
    let (service, _rx) = TaskService::channel();
    let tasks = service.list_task_definitions(root.path());
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].command_line(), if cfg!(windows) { "npm.cmd run lint" } else { "npm run lint" });
}

#[test]
fn stop_uses_taskkill_with_process_tree_flags_on_windows() {
    // Port of the Windows-only "stops a running task by terminating its process tree"
    // tests: the helper process is `taskkill /pid <pid> /t /f`.
    assert_eq!(taskkill_args(4321), ["/pid", "4321", "/t", "/f"]);
    assert_eq!(taskkill_args(8765), ["/pid", "8765", "/t", "/f"]);
}

#[test]
fn stopping_an_unknown_run_reports_that_it_is_not_running() {
    let (service, _rx) = TaskService::channel();
    let error = service.stop_task("nope").unwrap_err();
    assert!(matches!(error, TaskError::NotRunning));
    assert_eq!(error.to_string(), "Task is no longer running");
}

#[test]
fn sanitizing_requires_cwd_inside_the_workspace() {
    let root = tempfile::tempdir().expect("tempdir");
    let mut task = TaskDefinition::custom("t", "t", "echo", &["x"]);

    // No cwd: defaults to the root.
    let request = TaskRunRequest { terminal_id: "default".into(), task: task.clone() };
    let sanitized = sanitize_task_request(root.path(), &request).expect("sanitize");
    assert_eq!(sanitized.task.cwd.as_deref(), Some(root.path()));

    // Inside the workspace (absolute and relative).
    task.cwd = Some(root.path().join("sub").join(".."));
    let request = TaskRunRequest { terminal_id: "default".into(), task: task.clone() };
    let sanitized = sanitize_task_request(root.path(), &request).expect("sanitize");
    assert_eq!(sanitized.task.cwd.as_deref(), Some(root.path()));

    task.cwd = Some("pkg/app".into());
    let request = TaskRunRequest { terminal_id: "default".into(), task: task.clone() };
    let sanitized = sanitize_task_request(root.path(), &request).expect("sanitize");
    assert_eq!(sanitized.task.cwd, Some(root.path().join("pkg").join("app")));

    // Escaping the workspace is rejected.
    task.cwd = Some(root.path().join("..").join("elsewhere"));
    let request = TaskRunRequest { terminal_id: "default".into(), task };
    assert!(matches!(
        sanitize_task_request(root.path(), &request),
        Err(TaskError::OutsideWorkspace(_))
    ));
}

#[test]
fn relative_roots_and_cwds_are_rejected() {
    let task = TaskDefinition::custom("t", "t", "echo", &["x"]);
    let request = TaskRunRequest { terminal_id: "default".into(), task: task.clone() };
    assert!(matches!(
        sanitize_task_request(Path::new("relative"), &request),
        Err(TaskError::NotAbsolute(_))
    ));

    let (service, _rx) = TaskService::channel();
    assert!(matches!(
        service.run_task(Path::new("relative"), &request),
        Err(TaskError::NotAbsolute(_))
    ));
    let mut with_cwd = task;
    with_cwd.cwd = Some("also/relative".into());
    let request = TaskRunRequest { terminal_id: "default".into(), task: with_cwd };
    let root = tempfile::tempdir().expect("tempdir");
    assert!(matches!(
        service.run_task(root.path(), &request),
        Err(TaskError::NotAbsolute(_))
    ));
}

#[test]
fn terminal_text_matches_the_electron_output() {
    use vsm_tasks::TaskRun;
    let run = TaskRun {
        run_id: "r".into(),
        task_id: "t".into(),
        label: "build".into(),
        command: "npm run build".into(),
        source: TaskSource::PackageScript,
        terminal_id: "default".into(),
        cwd: "/w".into(),
        status: TaskRunStatus::Running,
        started_at: "2024-01-01T00:00:00.000Z".into(),
        finished_at: None,
        exit_code: None,
        detail: None,
    };
    let started = TaskUpdate::Started { run: run.clone(), message: "build started.".into() };
    assert_eq!(
        started.to_terminal_text(),
        "\r\n\x1b[35m[Task] build\x1b[0m\r\n\x1b[90mnpm run build\x1b[0m\r\n"
    );
    let stdout = TaskUpdate::Output {
        run_id: "r".into(),
        terminal_id: "default".into(),
        stream: OutputStream::Stdout,
        line: "hi".into(),
    };
    assert_eq!(stdout.to_terminal_text(), "hi\r\n");
    let stderr = TaskUpdate::Output {
        run_id: "r".into(),
        terminal_id: "default".into(),
        stream: OutputStream::Stderr,
        line: "bad".into(),
    };
    assert_eq!(stderr.to_terminal_text(), "\x1b[31mbad\r\n\x1b[0m");
    let mut finished = run;
    finished.status = TaskRunStatus::Success;
    assert_eq!(
        TaskUpdate::Finished { run: finished.clone(), message: "ok".into() }.to_terminal_text(),
        "\r\n\x1b[32m[Done] ok\x1b[0m\r\n"
    );
    finished.status = TaskRunStatus::Cancelled;
    assert_eq!(
        TaskUpdate::Finished { run: finished.clone(), message: "no".into() }.to_terminal_text(),
        "\r\n\x1b[33m[Cancelled] no\x1b[0m\r\n"
    );
    finished.status = TaskRunStatus::Error;
    assert_eq!(
        TaskUpdate::Finished { run: finished, message: "bad".into() }.to_terminal_text(),
        "\r\n\x1b[31m[Failed] bad\x1b[0m\r\n"
    );
}

// ---------------------------------------------------------------------------
// Real process tests.
// ---------------------------------------------------------------------------

fn collect_until_finished(rx: &Receiver<TaskUpdate>, run_id: &str) -> Vec<TaskUpdate> {
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut updates = Vec::new();
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let update = rx
            .recv_timeout(remaining)
            .unwrap_or_else(|_| panic!("timed out waiting for run {run_id}; got {updates:?}"));
        let is_end = matches!(&update, TaskUpdate::Finished { run, .. } if run.run_id == run_id);
        updates.push(update);
        if is_end {
            return updates;
        }
    }
}

fn request(terminal: &str, task: TaskDefinition) -> TaskRunRequest {
    TaskRunRequest { terminal_id: terminal.into(), task }
}

fn shell_task(id: &str, script: &str) -> TaskDefinition {
    let mut task = TaskDefinition::custom(id, id, script, &[]);
    task.shell = Some(true);
    task
}

#[cfg(unix)]
mod unix {
    use super::*;

    fn process_is_alive(pid: i32) -> bool {
        // A zombie (killed but not yet reaped by a lazy init) counts as dead.
        if let Ok(stat) = fs::read_to_string(format!("/proc/{pid}/stat")) {
            let state = stat.rsplit(')').next().unwrap_or("").trim().chars().next();
            return state != Some('Z');
        }
        // SAFETY: signal 0 only probes for existence.
        unsafe { libc::kill(pid, 0) == 0 }
    }

    fn wait_for_file(path: &Path) -> String {
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if let Ok(text) = fs::read_to_string(path) {
                if text.ends_with('\n') {
                    return text.trim().to_string();
                }
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("{} never appeared", path.display());
    }

    #[test]
    fn streams_stdout_and_stderr_lines_and_reports_success() {
        let root = tempfile::tempdir().expect("tempdir");
        let (service, rx) = TaskService::channel();
        let task = shell_task("echo", "echo one; echo two; echo problem 1>&2; printf partial");

        let run = service.run_task(root.path(), &request("term-1", task)).expect("run");
        assert_eq!(run.status, TaskRunStatus::Running);
        assert_eq!(run.cwd, root.path());
        assert_eq!(run.command, "echo one; echo two; echo problem 1>&2; printf partial");
        assert!(run.run_id.starts_with("task-run-"));

        let updates = collect_until_finished(&rx, &run.run_id);
        assert!(matches!(&updates[0], TaskUpdate::Started { message, .. } if message == "echo started."));

        let lines = |wanted: OutputStream| -> Vec<String> {
            updates
                .iter()
                .filter_map(|u| match u {
                    TaskUpdate::Output { stream, line, run_id, terminal_id } if *stream == wanted => {
                        assert_eq!(run_id, &run.run_id);
                        assert_eq!(terminal_id, "term-1");
                        Some(line.clone())
                    }
                    _ => None,
                })
                .collect()
        };
        assert_eq!(lines(OutputStream::Stdout), vec!["one", "two", "partial"]);
        assert_eq!(lines(OutputStream::Stderr), vec!["problem"]);

        let Some(TaskUpdate::Finished { run: finished, message }) = updates.last() else {
            panic!("no finish event");
        };
        assert_eq!(finished.status, TaskRunStatus::Success);
        assert_eq!(finished.exit_code, Some(0));
        assert!(finished.finished_at.is_some());
        assert_eq!(message, "echo finished successfully.");
        assert!(service.active_run_ids().is_empty());
    }

    #[test]
    fn non_zero_exit_codes_are_errors() {
        let root = tempfile::tempdir().expect("tempdir");
        let (service, rx) = TaskService::channel();
        let run = service
            .run_task(root.path(), &request("t", shell_task("fail", "exit 3")))
            .expect("run");
        let updates = collect_until_finished(&rx, &run.run_id);
        let Some(TaskUpdate::Finished { run: finished, message }) = updates.last() else {
            panic!("no finish event");
        };
        assert_eq!(finished.status, TaskRunStatus::Error);
        assert_eq!(finished.exit_code, Some(3));
        assert_eq!(message, "fail exited with code 3.");
    }

    #[test]
    fn runs_non_shell_commands_in_the_task_cwd() {
        let root = tempfile::tempdir().expect("tempdir");
        let sub = root.path().join("sub");
        fs::create_dir(&sub).expect("mkdir");
        let (service, rx) = TaskService::channel();

        let mut task = TaskDefinition::custom("pwd", "pwd", "pwd", &[]);
        task.cwd = Some(sub.clone());
        let run = service.run_task(root.path(), &request("t", task)).expect("run");
        assert_eq!(run.cwd, sub);
        let updates = collect_until_finished(&rx, &run.run_id);
        let printed = updates.iter().find_map(|u| match u {
            TaskUpdate::Output { line, .. } => Some(line.clone()),
            _ => None,
        });
        let printed = fs::canonicalize(printed.expect("pwd output")).expect("canonicalize");
        assert_eq!(printed, fs::canonicalize(&sub).expect("canonicalize"));
    }

    #[test]
    fn spawn_failures_are_reported_as_failed_runs() {
        let root = tempfile::tempdir().expect("tempdir");
        let (service, rx) = TaskService::channel();
        let task = TaskDefinition::custom("missing", "missing", "definitely-not-a-real-binary-xyz", &[]);
        let run = service.run_task(root.path(), &request("t", task)).expect("run");
        let updates = collect_until_finished(&rx, &run.run_id);
        assert_eq!(updates.len(), 2);
        let Some(TaskUpdate::Finished { run: finished, .. }) = updates.last() else {
            panic!("no finish event");
        };
        assert_eq!(finished.status, TaskRunStatus::Error);
        assert_eq!(finished.exit_code, None);
        assert!(service.active_run_ids().is_empty());
    }

    #[test]
    fn stops_a_running_task_by_terminating_its_whole_process_tree() {
        let root = tempfile::tempdir().expect("tempdir");
        let pid_file = root.path().join("child.pid");
        let (service, rx) = TaskService::channel();

        // The shell starts a background sleep (a grandchild) and waits for it.
        let script = format!("sleep 300 & echo $! > {}.tmp && mv {}.tmp {}; wait", pid_file.display(), pid_file.display(), pid_file.display());
        let run = service
            .run_task(root.path(), &request("t", shell_task("tree", &script)))
            .expect("run");
        let grandchild: i32 = wait_for_file(&pid_file).parse().expect("pid");
        assert!(process_is_alive(grandchild));
        assert_eq!(service.active_run_ids(), vec![run.run_id.clone()]);

        let started = Instant::now();
        service.stop_task(&run.run_id).expect("stop");
        assert!(started.elapsed() < Duration::from_secs(10));

        let updates = collect_until_finished(&rx, &run.run_id);
        let Some(TaskUpdate::Finished { run: finished, message }) = updates.last() else {
            panic!("no finish event");
        };
        assert_eq!(finished.status, TaskRunStatus::Cancelled);
        assert_eq!(message, "tree was cancelled.");
        assert!(!process_is_alive(grandchild), "grandchild survived stop_task");
        assert!(service.active_run_ids().is_empty());
        // Stopping again: the run is gone.
        assert!(matches!(service.stop_task(&run.run_id), Err(TaskError::NotRunning)));
    }

    #[test]
    fn escalates_to_sigkill_for_processes_that_ignore_sigterm() {
        let root = tempfile::tempdir().expect("tempdir");
        let ready = root.path().join("ready");
        let (service, rx) = TaskService::channel();
        let script = format!(
            "trap '' TERM; (trap '' TERM; while true; do sleep 1; done) & echo ok > {r}.tmp && mv {r}.tmp {r}; while true; do sleep 1; done",
            r = ready.display()
        );
        let run = service
            .run_task(root.path(), &request("t", shell_task("stubborn", &script)))
            .expect("run");
        wait_for_file(&ready);

        service.stop_task(&run.run_id).expect("stop");
        let updates = collect_until_finished(&rx, &run.run_id);
        let Some(TaskUpdate::Finished { run: finished, .. }) = updates.last() else {
            panic!("no finish event");
        };
        assert_eq!(finished.status, TaskRunStatus::Cancelled);
        assert_eq!(finished.exit_code, None, "killed by signal");
    }

    #[test]
    fn stop_right_after_launch_works() {
        // Port of "waits briefly for a pid when stop is pressed immediately after launch".
        let root = tempfile::tempdir().expect("tempdir");
        let (service, rx) = TaskService::channel();
        let run = service
            .run_task(root.path(), &request("t", shell_task("slow", "sleep 300")))
            .expect("run");
        service.stop_task(&run.run_id).expect("stop");
        let updates = collect_until_finished(&rx, &run.run_id);
        let Some(TaskUpdate::Finished { run: finished, .. }) = updates.last() else {
            panic!("no finish event");
        };
        assert_eq!(finished.status, TaskRunStatus::Cancelled);
    }

    #[test]
    fn starting_a_new_run_on_a_terminal_stops_the_previous_one() {
        let root = tempfile::tempdir().expect("tempdir");
        let (service, rx) = TaskService::channel();
        let first = service
            .run_task(root.path(), &request("shared", shell_task("first", "sleep 300")))
            .expect("run");
        let second = service
            .run_task(root.path(), &request("shared", shell_task("second", "echo done")))
            .expect("run");

        let mut finished = std::collections::HashMap::new();
        let deadline = Instant::now() + Duration::from_secs(30);
        while finished.len() < 2 {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let update = rx.recv_timeout(remaining).expect("update");
            if let TaskUpdate::Finished { run, .. } = update {
                finished.insert(run.run_id.clone(), run.status);
            }
        }
        assert_eq!(finished[&first.run_id], TaskRunStatus::Cancelled);
        assert_eq!(finished[&second.run_id], TaskRunStatus::Success);
    }

    #[test]
    fn rerunning_on_a_terminal_kills_the_previous_tree_before_spawning() {
        let root = tempfile::tempdir().expect("tempdir");
        let pid_file = root.path().join("child.pid");
        let (service, rx) = TaskService::channel();
        let script = format!("sleep 300 & echo $! > {0}.tmp && mv {0}.tmp {0}; wait", pid_file.display());
        let first = service
            .run_task(root.path(), &request("shared", shell_task("first", &script)))
            .expect("run");
        let grandchild: i32 = wait_for_file(&pid_file).parse().expect("pid");
        assert!(process_is_alive(grandchild));

        let second = service
            .run_task(root.path(), &request("shared", shell_task("second", "echo done")))
            .expect("run");
        // The old tree is already gone when run_task returns (no overlap).
        assert!(!process_is_alive(grandchild), "previous tree still running");
        collect_until_finished(&rx, &second.run_id);
        assert_ne!(first.run_id, second.run_id);
    }

    #[test]
    fn stop_task_also_kills_background_members_after_the_leader_exited() {
        let root = tempfile::tempdir().expect("tempdir");
        let pid_file = root.path().join("bg.pid");
        let (service, rx) = TaskService::channel();
        // The shell exits right away; the background sleep keeps the pipes (and the group) alive.
        let script = format!("sleep 300 & echo $! > {0}.tmp && mv {0}.tmp {0}", pid_file.display());
        let run = service
            .run_task(root.path(), &request("t", shell_task("bg", &script)))
            .expect("run");
        let background: i32 = wait_for_file(&pid_file).parse().expect("pid");
        assert!(process_is_alive(background));

        // The run is still registered while the pipes drain.
        service.stop_task(&run.run_id).expect("stop");
        assert!(!process_is_alive(background), "background member survived stop_task");
        let updates = collect_until_finished(&rx, &run.run_id);
        let Some(TaskUpdate::Finished { run: finished, .. }) = updates.last() else {
            panic!("no finish event");
        };
        assert_eq!(finished.status, TaskRunStatus::Cancelled);
    }

    #[test]
    fn nothing_is_sent_after_finished_even_if_a_background_process_keeps_writing() {
        let root = tempfile::tempdir().expect("tempdir");
        let (service, rx) = TaskService::channel();
        // Normal exit, but a detached grandchild holds the pipe and prints after the
        // 1 s drain timeout.
        let run = service
            .run_task(
                root.path(),
                &request("t", shell_task("late", "(sleep 1.6; echo late) & echo now")),
            )
            .expect("run");
        let updates = collect_until_finished(&rx, &run.run_id);
        let Some(TaskUpdate::Finished { run: finished, .. }) = updates.last() else {
            panic!("no finish event");
        };
        assert_eq!(finished.status, TaskRunStatus::Success);
        assert!(updates.iter().any(|u| matches!(u, TaskUpdate::Output { line, .. } if line == "now")));

        std::thread::sleep(Duration::from_millis(1500));
        assert!(rx.try_recv().is_err(), "update delivered after Finished");
    }

    #[test]
    fn carriage_return_progress_is_streamed_as_separate_lines() {
        let root = tempfile::tempdir().expect("tempdir");
        let (service, rx) = TaskService::channel();
        let run = service
            .run_task(root.path(), &request("t", shell_task("progress", "printf '10%%\\r50%%\\r100%%\\n'")))
            .expect("run");
        let updates = collect_until_finished(&rx, &run.run_id);
        let lines: Vec<&str> = updates
            .iter()
            .filter_map(|u| match u {
                TaskUpdate::Output { line, .. } => Some(line.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(lines, ["10%", "50%", "100%"]);
    }

    #[test]
    fn huge_unterminated_output_is_chunked_and_backpressured() {
        let root = tempfile::tempdir().expect("tempdir");
        // Tiny channel: the readers must block instead of buffering everything.
        let (service, rx) = TaskService::channel_with_capacity(2);
        let run = service
            .run_task(
                root.path(),
                &request("t", shell_task("flood", "head -c 1000000 /dev/zero | tr '\\0' 'x'")),
            )
            .expect("run");
        let mut total = 0usize;
        let mut longest = 0usize;
        loop {
            match rx.recv_timeout(Duration::from_secs(30)).expect("update") {
                TaskUpdate::Output { line, .. } => {
                    total += line.len();
                    longest = longest.max(line.len());
                }
                TaskUpdate::Finished { run: finished, .. } => {
                    assert_eq!(finished.run_id, run.run_id);
                    assert_eq!(finished.status, TaskRunStatus::Success);
                    break;
                }
                TaskUpdate::Started { .. } => {}
            }
        }
        assert_eq!(total, 1_000_000);
        assert!(longest <= 64 * 1024);
    }

    #[test]
    fn stop_all_stops_every_active_run() {
        let root = tempfile::tempdir().expect("tempdir");
        let (service, rx) = TaskService::channel();
        let a = service.run_task(root.path(), &request("a", shell_task("a", "sleep 300"))).expect("run");
        let b = service.run_task(root.path(), &request("b", shell_task("b", "sleep 300"))).expect("run");
        service.stop_all().expect("stop all");
        let mut pending = vec![a.run_id, b.run_id];
        while !pending.is_empty() {
            if let TaskUpdate::Finished { run, .. } = rx.recv_timeout(Duration::from_secs(30)).expect("update") {
                assert_eq!(run.status, TaskRunStatus::Cancelled);
                pending.retain(|id| *id != run.run_id);
            }
        }
    }
}

#[cfg(windows)]
mod windows {
    use super::*;

    #[test]
    fn runs_and_stops_a_task_through_the_shell() {
        let root = tempfile::tempdir().expect("tempdir");
        let (service, rx) = TaskService::channel();
        let run = service
            .run_task(root.path(), &request("t", shell_task("echo", "echo hello")))
            .expect("run");
        let updates = collect_until_finished(&rx, &run.run_id);
        assert!(updates.iter().any(|u| matches!(u, TaskUpdate::Output { line, .. } if line.contains("hello"))));

        let run = service
            .run_task(root.path(), &request("t", shell_task("wait", "ping -n 60 127.0.0.1")))
            .expect("run");
        service.stop_task(&run.run_id).expect("stop");
        let updates = collect_until_finished(&rx, &run.run_id);
        let Some(TaskUpdate::Finished { run: finished, .. }) = updates.last() else {
            panic!("no finish event");
        };
        assert_eq!(finished.status, TaskRunStatus::Cancelled);
    }
}
