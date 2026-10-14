//! Runs in its own process so thread counting is not disturbed by other tests.
#![cfg(target_os = "linux")]

use std::time::{Duration, Instant};

use vsm_terminal::{SessionConfig, TerminalEventKind, TerminalManager};

fn term_threads() -> usize {
    std::fs::read_dir("/proc/self/task")
        .unwrap()
        .filter_map(|e| std::fs::read_to_string(e.ok()?.path().join("comm")).ok())
        .filter(|c| c.starts_with("vsm-term-"))
        .count()
}

fn wait_for(cond: impl Fn() -> bool) -> bool {
    let end = Instant::now() + Duration::from_secs(5);
    while Instant::now() < end {
        if cond() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    cond()
}

// One test fn so the two scenarios never overlap (thread counts are process-global).
#[test]
fn threads_are_released() {
    dropping_the_manager_releases_threads();
    close_releases_threads_even_if_a_background_job_keeps_the_pty_open();
}

fn close_releases_threads_even_if_a_background_job_keeps_the_pty_open() {
    if std::process::Command::new("setsid").arg("--version").output().is_err() {
        eprintln!("setsid not available, skipping");
        return;
    }
    let mgr = TerminalManager::new();
    let rx = mgr.events();
    // `setsid sleep` leaves our process group and keeps the slave side open (inherits stdout),
    // so the reader never sees EOF after the shell exits.
    mgr.create("bg", SessionConfig::command(["/bin/sh", "-c", "setsid sleep 6 & sleep 0.2; exit 0"]))
        .unwrap();
    loop {
        let ev = rx.recv_timeout(Duration::from_secs(10)).unwrap();
        if matches!(ev.kind, TerminalEventKind::Exited { .. }) {
            break;
        }
    }
    assert!(wait_for(|| term_threads() > 0), "reader thread should still be blocked on the open PTY");
    mgr.close("bg");
    assert!(wait_for(|| term_threads() == 0), "threads leaked: {}", term_threads());
}

fn dropping_the_manager_releases_threads() {
    {
        let mgr = TerminalManager::new();
        mgr.create("a", SessionConfig::command(["/bin/sh"])).unwrap();
        mgr.create("b", SessionConfig::command(["/bin/sh"])).unwrap();
        assert!(wait_for(|| term_threads() >= 6), "expected 3 threads per session");
    }
    assert!(wait_for(|| term_threads() == 0), "threads leaked: {}", term_threads());
}
