#![cfg(unix)]

use std::time::{Duration, Instant};

use crossbeam_channel::Receiver;
use vsm_terminal::{
    CellFlags, ScrollCmd, SelectionKind, SelectionSide, SessionConfig, TerminalError,
    TerminalEvent, TerminalEventKind, TerminalManager,
};

const T: Duration = Duration::from_secs(10);

fn sh(script: &str) -> SessionConfig {
    SessionConfig::command(["/bin/sh", "-c", script])
}

/// Wait until `pred(snapshot text)` holds, pumping events.
fn wait_text(mgr: &TerminalManager, id: &str, pred: impl Fn(&str) -> bool) -> String {
    let deadline = Instant::now() + T;
    let rx = mgr.events();
    loop {
        let text = mgr.snapshot(id).unwrap().to_text();
        if pred(&text) {
            return text;
        }
        assert!(Instant::now() < deadline, "timed out; screen was:\n{text}");
        let _ = rx.recv_timeout(Duration::from_millis(50));
    }
}

fn wait_exit(rx: &Receiver<TerminalEvent>, id: &str) -> Option<i32> {
    let deadline = Instant::now() + T;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        let ev = rx.recv_timeout(left).expect("timed out waiting for Exited");
        if ev.id == id {
            if let TerminalEventKind::Exited { code } = ev.kind {
                return code;
            }
        }
    }
}

#[test]
fn printf_hello_appears_in_grid_and_exits() {
    let mgr = TerminalManager::new();
    let rx = mgr.events();
    mgr.create("a", sh("printf hello")).unwrap();
    assert_eq!(wait_exit(&rx, "a"), Some(0));
    let snap = mgr.snapshot("a").unwrap();
    assert_eq!(snap.line_text(0), "hello");
    assert_eq!(snap.cell(0, 0).unwrap().ch, 'h');
    assert!(mgr.get("a").unwrap().is_exited());
    assert_eq!(mgr.get("a").unwrap().exit_code(), Some(0));
}

#[test]
fn nonzero_exit_code_reported() {
    let mgr = TerminalManager::new();
    let rx = mgr.events();
    mgr.create("a", sh("exit 7")).unwrap();
    assert_eq!(wait_exit(&rx, "a"), Some(7));
}

#[test]
fn colors_and_attributes_resolved() {
    let mgr = TerminalManager::new();
    let rx = mgr.events();
    mgr.create("a", sh(r"printf '\033[1;31mR\033[0m\033[38;2;1;2;3mT\033[0m\033[7mI\033[0m\033[4;42mU'"))
        .unwrap();
    wait_exit(&rx, "a");
    let snap = mgr.snapshot("a").unwrap();
    let r = snap.cell(0, 0).unwrap();
    assert_eq!(r.ch, 'R');
    assert!(r.flags.bold());
    assert_eq!(r.fg, snap_palette().ansi[1]);
    assert!(r.flags.contains(CellFlags::DEFAULT_BG));
    let t = snap.cell(0, 1).unwrap();
    assert_eq!((t.fg.r, t.fg.g, t.fg.b), (1, 2, 3));
    let i = snap.cell(0, 2).unwrap();
    assert_eq!(i.fg, snap.default_bg);
    assert_eq!(i.bg, snap.default_fg);
    assert!(i.flags.contains(CellFlags::INVERSE));
    let u = snap.cell(0, 3).unwrap();
    assert!(u.flags.underline());
    assert_eq!(u.bg, snap_palette().ansi[2]);
    assert!(!u.flags.contains(CellFlags::DEFAULT_BG));
}

fn snap_palette() -> vsm_terminal::Palette {
    vsm_terminal::Palette::default()
}

#[test]
fn cwd_and_env_are_applied() {
    let mgr = TerminalManager::new();
    let rx = mgr.events();
    let mut cfg = sh("printf '%s|%s' \"$(pwd)\" \"$VSM_TEST\"");
    cfg.cwd = Some("/tmp".into());
    cfg.env = vec![("VSM_TEST".into(), "yes".into())];
    mgr.create("a", cfg).unwrap();
    wait_exit(&rx, "a");
    assert_eq!(mgr.snapshot("a").unwrap().line_text(0), "/tmp|yes");
}

#[test]
fn missing_cwd_falls_back() {
    let mgr = TerminalManager::new();
    let rx = mgr.events();
    let mut cfg = sh("printf ok");
    cfg.cwd = Some("/definitely/not/here".into());
    mgr.create("a", cfg).unwrap();
    assert_eq!(wait_exit(&rx, "a"), Some(0));
}

#[test]
fn spawn_failure_is_an_error() {
    let mgr = TerminalManager::new();
    let err = mgr.create("bad", SessionConfig::command(["/definitely/not/a/program"])).err();
    assert!(matches!(err, Some(TerminalError::Spawn { .. })), "{err:?}");
    assert!(mgr.get("bad").is_none());
}

#[test]
fn interactive_shell_multiple_sessions_and_idempotent_create() {
    let mgr = TerminalManager::new();
    mgr.create("one", SessionConfig::command(["/bin/sh"])).unwrap();
    mgr.create("two", SessionConfig::command(["/bin/sh"])).unwrap();
    let again = mgr.create("one", SessionConfig::command(["/bin/sh"])).unwrap();
    assert_eq!(again.id(), "one");
    assert_eq!(mgr.ids(), vec!["one".to_string(), "two".to_string()]);

    mgr.write("one", b"echo AAA$((1+1))\r").unwrap();
    mgr.write("two", b"echo BBB$((2+2))\r").unwrap();
    wait_text(&mgr, "one", |t| t.contains("AAA2"));
    wait_text(&mgr, "two", |t| t.contains("BBB4"));
    assert!(!mgr.snapshot("one").unwrap().to_text().contains("BBB4"));

    mgr.close("one");
    assert!(matches!(mgr.write("one", b"x"), Err(TerminalError::NotFound(_))));
    mgr.close("two");
    assert!(mgr.ids().is_empty());
}

#[test]
fn resize_updates_pty_and_grid() {
    let mgr = TerminalManager::new();
    mgr.create("a", SessionConfig::command(["/bin/sh"])).unwrap();
    mgr.write("a", b"stty size\r").unwrap();
    wait_text(&mgr, "a", |t| t.contains("24 80"));

    mgr.resize("a", 100, 30).unwrap();
    let snap = mgr.snapshot("a").unwrap();
    assert_eq!((snap.cols, snap.rows), (100, 30));
    assert_eq!(snap.cells.len(), 100 * 30);
    mgr.write("a", b"clear; stty size\r").unwrap();
    wait_text(&mgr, "a", |t| t.contains("30 100"));
    assert_eq!(mgr.get("a").unwrap().size(), (100, 30));

    assert!(matches!(mgr.resize("a", 0, 10), Err(TerminalError::InvalidArgument(_))));
    assert!(matches!(mgr.resize("nope", 10, 10), Err(TerminalError::NotFound(_))));
}

#[test]
fn kill_terminates_and_emits_exit() {
    let mgr = TerminalManager::new();
    let rx = mgr.events();
    mgr.create("a", sh("sleep 60")).unwrap();
    assert!(!mgr.get("a").unwrap().is_exited());
    mgr.kill("a").unwrap();
    let start = Instant::now();
    wait_exit(&rx, "a");
    assert!(start.elapsed() < Duration::from_secs(5));
    assert!(mgr.get("a").unwrap().is_exited());
    assert!(matches!(mgr.write("a", b"x"), Err(TerminalError::Exited(_))));
    // killing twice is fine
    mgr.kill("a").unwrap();
    mgr.close("a");
}

#[test]
fn close_kills_running_process() {
    let mgr = TerminalManager::new();
    let rx = mgr.events();
    mgr.create("a", sh("sleep 60")).unwrap();
    mgr.close("a");
    wait_exit(&rx, "a");
}

#[test]
fn title_and_bell_events() {
    let mgr = TerminalManager::new();
    let rx = mgr.events();
    mgr.create("a", sh(r"printf '\033]0;my title\007\007'")).unwrap();
    let deadline = Instant::now() + T;
    let (mut title, mut bell) = (false, false);
    while !(title && bell) {
        let left = deadline.saturating_duration_since(Instant::now());
        let ev = rx.recv_timeout(left).expect("timed out");
        match ev.kind {
            TerminalEventKind::TitleChanged => title = true,
            TerminalEventKind::Bell => bell = true,
            _ => {}
        }
    }
    assert_eq!(mgr.get("a").unwrap().title().as_deref(), Some("my title"));
    assert_eq!(mgr.snapshot("a").unwrap().title.as_deref(), Some("my title"));
}

#[test]
fn scrollback_scroll_and_selection() {
    let mgr = TerminalManager::new();
    let rx = mgr.events();
    let mut cfg = sh("i=1; while [ $i -le 100 ]; do echo line$i; i=$((i+1)); done");
    cfg.rows = 10;
    mgr.create("a", cfg).unwrap();
    wait_exit(&rx, "a");

    let snap = mgr.snapshot("a").unwrap();
    assert_eq!(snap.display_offset, 0);
    assert!(snap.history_size >= 90);
    assert_eq!(snap.line_text(8), "line100");

    mgr.scroll("a", ScrollCmd::Top).unwrap();
    let top = mgr.snapshot("a").unwrap();
    assert_eq!(top.display_offset, top.history_size);
    assert_eq!(top.line_text(0), "line1");

    mgr.scroll("a", ScrollCmd::Lines(-3)).unwrap();
    assert_eq!(mgr.snapshot("a").unwrap().line_text(0), "line4");
    mgr.scroll("a", ScrollCmd::PageDown).unwrap();
    mgr.scroll("a", ScrollCmd::Bottom).unwrap();
    assert_eq!(mgr.snapshot("a").unwrap().display_offset, 0);

    // select "line9x" rows 0..1 in the live view
    let s = mgr.get("a").unwrap();
    let live = s.snapshot();
    let first = live.line_text(0);
    s.selection_start(0, 0, SelectionSide::Left, SelectionKind::Simple);
    s.selection_update(1, 100, SelectionSide::Right);
    assert!(s.has_selection());
    let text = s.selection_text().unwrap();
    assert!(text.starts_with(&first), "{text:?}");
    assert!(text.contains('\n'));
    assert!(s.snapshot().cell(0, 0).unwrap().flags.selected());
    s.selection_clear();
    assert!(s.selection_text().is_none());
    assert!(!s.snapshot().cell(0, 0).unwrap().flags.selected());

    s.select_all();
    let all = s.selection_text().unwrap();
    assert!(all.contains("line1\n") && all.contains("line100"));
    assert!(s.full_text().starts_with("line1\n"));
}

#[test]
fn wakeups_are_coalesced_and_idle_is_silent() {
    let mgr = TerminalManager::new();
    let rx = mgr.events();
    mgr.create("a", SessionConfig::command(["/bin/sh"])).unwrap();
    // let the prompt/startup settle, then drain
    std::thread::sleep(Duration::from_millis(300));
    mgr.snapshot("a").unwrap();
    while rx.try_recv().is_ok() {}
    mgr.snapshot("a").unwrap();

    // idle: nothing happens
    assert!(rx.recv_timeout(Duration::from_millis(500)).is_err(), "idle session produced events");

    // burst of output without snapshotting: at most one ContentChanged
    mgr.write("a", b"seq 1 20000\r").unwrap();
    std::thread::sleep(Duration::from_millis(1500));
    let mut changed = 0;
    while let Ok(ev) = rx.try_recv() {
        if ev.kind == TerminalEventKind::ContentChanged {
            changed += 1;
        }
    }
    assert_eq!(changed, 1, "expected exactly one coalesced wakeup");

    // after a snapshot the next output wakes us again
    mgr.snapshot("a").unwrap();
    mgr.write("a", b"echo again\r").unwrap();
    let ev = rx.recv_timeout(T).unwrap();
    assert_eq!(ev.kind, TerminalEventKind::ContentChanged);
}

#[test]
fn change_directory_and_paste() {
    let mgr = TerminalManager::new();
    mgr.create("a", SessionConfig::command(["/bin/sh"])).unwrap();
    mgr.set_working_directory("a", "/usr").unwrap();
    mgr.write("a", b"pwd\r").unwrap();
    wait_text(&mgr, "a", |t| t.lines().any(|l| l.trim_start_matches(['#', '$', ' ']) == "/usr"));
    mgr.paste("a", "echo pasted\n").unwrap();
    wait_text(&mgr, "a", |t| t.lines().any(|l| l.trim_start_matches(['#', '$', ' ']) == "pasted"));
}

#[test]
fn alt_screen_modes_and_cursor_position_report() {
    let mgr = TerminalManager::new();
    let rx = mgr.events();
    mgr.create("a", sh(r"printf '\033[?1049h\033[?2004h'; sleep 0.3")).unwrap();
    wait_exit(&rx, "a");
    let snap = mgr.snapshot("a").unwrap();
    assert!(snap.mode.alt_screen && snap.mode.bracketed_paste);

    // The emulator answers ESC[6n by writing the report to the PTY; the tty echoes it back.
    mgr.create("b", sh(r"printf 'ab\033[6n'; sleep 0.5")).unwrap();
    wait_text(&mgr, "b", |t| t.contains("^[[1;3R"));
}

#[test]
fn unread_stdin_and_huge_paste_never_block_the_ui_thread() {
    // The child sleeps without reading stdin, so the PTY input buffer fills up and a blocking
    // write would hang. Writes are queued to a writer thread; snapshot/resize must stay fast.
    let mgr = TerminalManager::new();
    let rx = mgr.events();
    mgr.create("a", sh("sleep 3")).unwrap();
    let big = "x".repeat(300 * 1024);
    let t0 = Instant::now();
    mgr.paste("a", &big).unwrap();
    mgr.write("a", big.as_bytes()).unwrap();
    assert!(t0.elapsed() < Duration::from_millis(500), "write blocked: {:?}", t0.elapsed());
    std::thread::sleep(Duration::from_millis(200));
    let t1 = Instant::now();
    for _ in 0..20 {
        mgr.snapshot("a").unwrap();
    }
    mgr.resize("a", 90, 30).unwrap();
    assert!(t1.elapsed() < Duration::from_millis(500), "snapshot blocked: {:?}", t1.elapsed());
    mgr.kill("a").unwrap();
    wait_exit(&rx, "a");
    mgr.close("a");
}

#[test]
fn kill_escalates_to_sigkill_for_processes_ignoring_sighup() {
    let mgr = TerminalManager::new();
    let rx = mgr.events();
    mgr.create("a", sh("trap '' HUP; while :; do sleep 1; done")).unwrap();
    std::thread::sleep(Duration::from_millis(300));
    let start = Instant::now();
    mgr.kill("a").unwrap();
    wait_exit(&rx, "a");
    let took = start.elapsed();
    assert!(took >= Duration::from_millis(1200), "SIGHUP alone should not have worked ({took:?})");
    assert!(took < Duration::from_secs(5));
}

#[test]
fn kill_after_exit_does_not_signal_and_succeeds() {
    let mgr = TerminalManager::new();
    let rx = mgr.events();
    mgr.create("a", sh("exit 0")).unwrap();
    wait_exit(&rx, "a");
    mgr.kill("a").unwrap();
    mgr.kill("a").unwrap();
}

#[test]
fn bell_and_title_events_are_coalesced() {
    let mgr = TerminalManager::new();
    let rx = mgr.events();
    // 500 bells and 500 title changes without the consumer ever calling snapshot().
    mgr.create(
        "a",
        sh("i=0; while [ $i -lt 500 ]; do printf '\\007\\033]0;t%s\\007' $i; i=$((i+1)); done"),
    )
    .unwrap();
    let mut bells = 0;
    let mut titles = 0;
    loop {
        let ev = rx.recv_timeout(T).expect("timed out");
        match ev.kind {
            TerminalEventKind::Bell => bells += 1,
            TerminalEventKind::TitleChanged => titles += 1,
            TerminalEventKind::Exited { .. } => break,
            _ => {}
        }
    }
    assert_eq!((bells, titles), (1, 1));
    // last value wins
    assert_eq!(mgr.get("a").unwrap().title().as_deref(), Some("t499"));
}

#[test]
fn pending_cwd_is_applied_at_create() {
    let mgr = TerminalManager::new();
    let rx = mgr.events();
    mgr.set_working_directory("later", "/usr").unwrap();
    mgr.create("later", sh("pwd")).unwrap();
    wait_exit(&rx, "later");
    assert_eq!(mgr.snapshot("later").unwrap().line_text(0), "/usr");
    // consumed once
    mgr.close("later");
    mgr.create("later", sh("pwd")).unwrap();
    wait_exit(&rx, "later");
    assert_ne!(mgr.snapshot("later").unwrap().line_text(0), "/usr");
    // close() forgets a pending path too
    mgr.set_working_directory("gone", "/usr").unwrap();
    mgr.close("gone");
}

#[test]
fn color_query_reports_terminal_override_first() {
    let mgr = TerminalManager::new();
    // Set palette entry 1 via OSC 4, then query it; the tty echoes the reply on screen.
    mgr.create(
        "a",
        sh(r"printf '\033]4;1;rgb:12/34/56\033\\'; printf '\033]4;1;?\033\\'; sleep 0.5"),
    )
    .unwrap();
    wait_text(&mgr, "a", |t| t.contains("1212/3434/5656"));
}
