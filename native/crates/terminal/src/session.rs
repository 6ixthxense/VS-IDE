//! A single terminal session: PTY + child process + VT emulator state.
//!
//! Threads per session (all blocking, so an idle session costs no CPU):
//! * reader: PTY -> VT parser -> grid (takes the `term` lock only while parsing),
//! * writer: channel -> PTY (user input and query replies; nothing ever writes to the PTY while
//!   the `term` lock is held, so a child that does not read stdin cannot stall the UI),
//! * waiter: reaps the child and announces `Exited`.

use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};
use std::sync::Arc;
use std::time::Duration;

use alacritty_terminal::event::{Event, EventListener, WindowSize};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line, Point, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::{viewport_to_point, Config as TermConfig};
use alacritty_terminal::vte::ansi::{Color, NamedColor, Processor};
use alacritty_terminal::Term;
use crossbeam_channel::{bounded, unbounded, Sender};
use parking_lot::{Condvar, Mutex};
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};

use crate::error::TerminalError;
use crate::resolve::platform_argv;
use crate::shell::{cd_command, default_shell, ShellKind, ShellSpec};
use crate::snapshot::{
    Cell, CellFlags, CursorShape, CursorSnapshot, GridSnapshot, ModeFlags, Palette,
};

/// What happened in a session.
///
/// Event volume is bounded: `ContentChanged`, `Bell`, `TitleChanged` and `ClipboardStore` are
/// each coalesced, i.e. at most one of each is queued per session until the consumer calls
/// [`Session::snapshot`] / [`Session::acknowledge`] (clipboard: [`Session::take_clipboard`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalEventKind {
    /// New output is available.
    ContentChanged,
    /// Window title changed (OSC 0/2); read the latest value with [`Session::title`].
    TitleChanged,
    /// BEL received (one or more times).
    Bell,
    /// Child process exited. `code` is `None` if it could not be determined.
    Exited { code: Option<i32> },
    /// The program asked to store text in the clipboard (OSC 52); fetch it with
    /// [`Session::take_clipboard`] (last request wins).
    ClipboardStore,
}

/// An event tagged with the originating session id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalEvent {
    pub id: String,
    pub kind: TerminalEventKind,
}

/// Parameters for a new session.
#[derive(Debug, Clone)]
pub struct SessionConfig {
    /// Shell to run; `None` = [`default_shell`] for this OS.
    pub shell: Option<ShellSpec>,
    /// Run this argv instead of a shell (e.g. a task). Overrides `shell`. On Windows `argv[0]`
    /// is resolved through PATH/PATHEXT and `.cmd`/`.bat` files are run through `cmd.exe`.
    pub command: Option<Vec<String>>,
    /// Working directory. Falls back to `$HOME`/`%USERPROFILE%` if missing or nonexistent.
    pub cwd: Option<PathBuf>,
    /// Extra environment variables (the process environment is inherited).
    pub env: Vec<(String, String)>,
    pub cols: u16,
    pub rows: u16,
    /// Scrollback lines kept.
    pub scrollback: usize,
    pub palette: Palette,
}

impl Default for SessionConfig {
    fn default() -> Self {
        SessionConfig {
            shell: None,
            command: None,
            cwd: None,
            env: Vec::new(),
            cols: 80,
            rows: 24,
            scrollback: 10_000,
            palette: Palette::default(),
        }
    }
}

impl SessionConfig {
    /// Convenience: run `argv` directly (no shell), e.g. `["/bin/sh", "-c", "printf hello"]`.
    pub fn command<I, S>(argv: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        SessionConfig { command: Some(argv.into_iter().map(Into::into).collect()), ..Default::default() }
    }
}

/// Scroll request for the viewport.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollCmd {
    /// Positive = towards older output (up), negative = towards the live bottom.
    Lines(i32),
    PageUp,
    PageDown,
    Top,
    Bottom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionKind {
    Simple,
    Block,
    /// Word selection.
    Semantic,
    Lines,
}

/// Which half of a cell the pointer is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionSide {
    Left,
    Right,
}

struct Size {
    cols: usize,
    rows: usize,
}

impl Dimensions for Size {
    fn total_lines(&self) -> usize {
        self.rows
    }
    fn screen_lines(&self) -> usize {
        self.rows
    }
    fn columns(&self) -> usize {
        self.cols
    }
}

type ColorFormatter = Arc<dyn Fn(alacritty_terminal::vte::ansi::Rgb) -> String + Send + Sync>;

struct Shared {
    dirty: AtomicBool,
    bell_pending: AtomicBool,
    title_pending: AtomicBool,
    clipboard_pending: AtomicBool,
    exited: AtomicBool,
    write_failed: AtomicBool,
    #[cfg(unix)]
    escalating: AtomicBool,
    title: Mutex<Option<String>>,
    clipboard: Mutex<Option<String>>,
    exit_code: Mutex<Option<i32>>,
    palette: Mutex<Palette>,
    /// OSC 4/10/11 color queries; answered by the reader after parsing (needs the term lock).
    color_requests: Mutex<Vec<(usize, ColorFormatter)>>,
    cols: AtomicU16,
    rows: AtomicU16,
    /// True once the child has been (or is about to be) reaped: its pid must not be signalled.
    reaped: Mutex<bool>,
    reaped_cv: Condvar,
}

#[derive(Clone)]
struct Listener {
    id: Arc<str>,
    tx: Sender<TerminalEvent>,
    write_tx: Sender<Vec<u8>>,
    shared: Arc<Shared>,
}

impl Listener {
    fn emit(&self, kind: TerminalEventKind) {
        let _ = self.tx.send(TerminalEvent { id: self.id.to_string(), kind });
    }
    /// Queue a reply for the writer thread. Never blocks and never touches the PTY directly.
    fn reply(&self, text: String) {
        let _ = self.write_tx.send(text.into_bytes());
    }
}

impl EventListener for Listener {
    fn send_event(&self, event: Event) {
        match event {
            Event::Title(t) => {
                *self.shared.title.lock() = Some(t);
                if !self.shared.title_pending.swap(true, Ordering::AcqRel) {
                    self.emit(TerminalEventKind::TitleChanged);
                }
            }
            Event::ResetTitle => {
                *self.shared.title.lock() = None;
                if !self.shared.title_pending.swap(true, Ordering::AcqRel) {
                    self.emit(TerminalEventKind::TitleChanged);
                }
            }
            Event::Bell => {
                if !self.shared.bell_pending.swap(true, Ordering::AcqRel) {
                    self.emit(TerminalEventKind::Bell);
                }
            }
            Event::ClipboardStore(_, text) => {
                *self.shared.clipboard.lock() = Some(text);
                if !self.shared.clipboard_pending.swap(true, Ordering::AcqRel) {
                    self.emit(TerminalEventKind::ClipboardStore);
                }
            }
            Event::PtyWrite(text) => self.reply(text),
            Event::ColorRequest(idx, fmt) => {
                let mut q = self.shared.color_requests.lock();
                if q.len() < 64 {
                    q.push((idx, fmt));
                }
            }
            Event::TextAreaSizeRequest(fmt) => {
                let text = fmt(WindowSize {
                    num_lines: self.shared.rows.load(Ordering::Relaxed),
                    num_cols: self.shared.cols.load(Ordering::Relaxed),
                    cell_width: 0,
                    cell_height: 0,
                });
                self.reply(text);
            }
            _ => {}
        }
    }
}

/// How long after SIGHUP a still-running process group gets SIGKILL.
#[cfg(unix)]
const KILL_ESCALATION: Duration = Duration::from_millis(1500);

/// A running terminal. All methods take `&self` and are thread-safe.
pub struct Session {
    id: String,
    term: Arc<Mutex<Term<Listener>>>,
    shared: Arc<Shared>,
    write_tx: Sender<Vec<u8>>,
    master: Arc<Mutex<Option<Box<dyn MasterPty + Send>>>>,
    #[cfg(not(unix))]
    killer: Mutex<Box<dyn portable_pty::ChildKiller + Send + Sync>>,
    #[cfg(unix)]
    wake: crate::unixio::WakeGuard,
    tx: Sender<TerminalEvent>,
    shell: ShellSpec,
    pid: Option<u32>,
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
}

#[cfg(unix)]
fn signal_group(pid: u32, sig: libc::c_int) {
    // SAFETY: plain kill(2); callers hold the `reaped` lock so the pid is not recycled. The
    // child is a session/group leader (setsid), so -pid addresses its whole process group.
    unsafe {
        libc::kill(-(pid as libc::pid_t), sig);
        libc::kill(pid as libc::pid_t, sig);
    }
}

/// Block until `pid` has exited without reaping it (so the pid cannot be recycled yet).
#[cfg(any(target_os = "linux", target_os = "android", target_os = "macos"))]
fn wait_exit_noreap(pid: u32) {
    loop {
        // SAFETY: siginfo_t is plain data; waitid only writes to it.
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        let r = unsafe {
            libc::waitid(libc::P_PID, pid as libc::id_t, &mut info, libc::WEXITED | libc::WNOWAIT)
        };
        if r == 0 {
            return;
        }
        if std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted {
            return;
        }
    }
}

#[cfg(all(unix, not(any(target_os = "linux", target_os = "android", target_os = "macos"))))]
fn wait_exit_noreap(_pid: u32) {}

impl Session {
    /// Spawn a session. Events are sent to `tx`.
    pub fn spawn(
        id: impl Into<String>,
        config: SessionConfig,
        tx: Sender<TerminalEvent>,
    ) -> Result<Arc<Session>, TerminalError> {
        let id: String = id.into();
        let cols = config.cols.max(1);
        let rows = config.rows.max(1);

        let explicit = config.command.as_ref().filter(|a| !a.is_empty());
        let (shell, mut cmd) = if let Some(argv) = explicit {
            let argv = platform_argv(argv).map_err(TerminalError::InvalidArgument)?;
            let shell = ShellSpec::new(argv[0].clone(), argv[1..].to_vec());
            (shell, CommandBuilder::from_argv(argv.iter().map(OsString::from).collect()))
        } else {
            let shell = config.shell.clone().unwrap_or_else(default_shell);
            let mut c = CommandBuilder::new(&shell.program);
            c.args(&shell.args);
            (shell, c)
        };
        // Same fallback as the Electron service: missing cwd -> home directory.
        let cwd = config.cwd.clone().filter(|p| p.is_dir()).or_else(home_dir);
        if let Some(cwd) = cwd {
            cmd.cwd(cwd);
        }
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        for (k, v) in &config.env {
            cmd.env(k, v);
        }

        let pair = native_pty_system()
            .openpty(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })
            .map_err(|e| TerminalError::Pty(e.to_string()))?;
        let mut child = pair.slave.spawn_command(cmd).map_err(|e| TerminalError::Spawn {
            program: shell.program.display().to_string(),
            message: e.to_string(),
        })?;
        // Must drop our slave handle or the reader never sees EOF on Unix.
        drop(pair.slave);
        let pid = child.process_id();
        #[cfg(not(unix))]
        let killer = child.clone_killer();

        #[cfg(unix)]
        let (mut reader, mut writer, wake) = crate::unixio::open(&*pair.master)?;
        #[cfg(not(unix))]
        let (mut reader, mut writer) = {
            let r = pair.master.try_clone_reader().map_err(|e| TerminalError::Pty(e.to_string()))?;
            let w = pair.master.take_writer().map_err(|e| TerminalError::Pty(e.to_string()))?;
            (r, w)
        };
        let master: Arc<Mutex<Option<Box<dyn MasterPty + Send>>>> =
            Arc::new(Mutex::new(Some(pair.master)));

        let shared = Arc::new(Shared {
            dirty: AtomicBool::new(false),
            bell_pending: AtomicBool::new(false),
            title_pending: AtomicBool::new(false),
            clipboard_pending: AtomicBool::new(false),
            exited: AtomicBool::new(false),
            write_failed: AtomicBool::new(false),
            #[cfg(unix)]
            escalating: AtomicBool::new(false),
            title: Mutex::new(None),
            clipboard: Mutex::new(None),
            exit_code: Mutex::new(None),
            palette: Mutex::new(config.palette.clone()),
            color_requests: Mutex::new(Vec::new()),
            cols: AtomicU16::new(cols),
            rows: AtomicU16::new(rows),
            reaped: Mutex::new(false),
            reaped_cv: Condvar::new(),
        });
        let (write_tx, write_rx) = unbounded::<Vec<u8>>();
        let listener = Listener {
            id: Arc::from(id.as_str()),
            tx: tx.clone(),
            write_tx: write_tx.clone(),
            shared: shared.clone(),
        };
        let term_cfg = TermConfig { scrolling_history: config.scrollback, ..TermConfig::default() };
        let term = Arc::new(Mutex::new(Term::new(
            term_cfg,
            &Size { cols: cols as usize, rows: rows as usize },
            listener.clone(),
        )));

        // Writer thread: the only place that writes to the PTY (may block if the child does not
        // read; that only ever stalls this thread).
        {
            let shared = shared.clone();
            std::thread::Builder::new().name(format!("vsm-term-write-{id}")).spawn(move || {
                while let Ok(buf) = write_rx.recv() {
                    if writer.write_all(&buf).and_then(|_| writer.flush()).is_err() {
                        shared.write_failed.store(true, Ordering::Release);
                        break;
                    }
                }
            })?;
        }

        // Reader thread: blocks until data (0 CPU when idle), parses, coalesces wakeups.
        let (done_tx, done_rx) = bounded::<()>(1);
        {
            let term = term.clone();
            let listener = listener.clone();
            std::thread::Builder::new().name(format!("vsm-term-read-{id}")).spawn(move || {
                let mut parser: Processor = Processor::new();
                let mut buf = vec![0u8; 64 * 1024];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => {
                            let mut replies: Vec<String> = Vec::new();
                            {
                                let mut t = term.lock();
                                parser.advance(&mut *t, &buf[..n]);
                                let reqs = std::mem::take(&mut *listener.shared.color_requests.lock());
                                if !reqs.is_empty() {
                                    let palette = listener.shared.palette.lock();
                                    for (idx, fmt) in reqs {
                                        // Terminal overrides (OSC 4/10/11) win over the theme.
                                        let rgb = t.colors()[idx]
                                            .unwrap_or_else(|| {
                                                let p = palette.indexed(idx);
                                                alacritty_terminal::vte::ansi::Rgb { r: p.r, g: p.g, b: p.b }
                                            });
                                        replies.push(fmt(rgb));
                                    }
                                }
                            }
                            for r in replies {
                                listener.reply(r);
                            }
                            if !listener.shared.dirty.swap(true, Ordering::AcqRel) {
                                listener.emit(TerminalEventKind::ContentChanged);
                            }
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                        Err(_) => break,
                    }
                }
                let _ = done_tx.send(());
            })?;
        }

        // Waiter thread: reaps the child and announces `Exited` once output is drained.
        {
            let listener = listener.clone();
            let master = master.clone();
            std::thread::Builder::new().name(format!("vsm-term-wait-{id}")).spawn(move || {
                #[cfg(unix)]
                if let Some(pid) = pid {
                    // Wait without reaping, flag the pid as dead under the lock that guards all
                    // signalling, and only then reap: kill() can never hit a recycled pid.
                    wait_exit_noreap(pid);
                    *listener.shared.reaped.lock() = true;
                    listener.shared.reaped_cv.notify_all();
                }
                let code = match child.wait() {
                    Ok(status) => Some(status.exit_code() as i32),
                    Err(_) => None,
                };
                *listener.shared.reaped.lock() = true;
                listener.shared.reaped_cv.notify_all();
                *listener.shared.exit_code.lock() = code;
                // ConPTY only signals EOF once the pseudoconsole is closed.
                #[cfg(windows)]
                {
                    master.lock().take();
                }
                #[cfg(not(windows))]
                let _ = &master;
                // Give the reader a moment to drain remaining output; a background process
                // holding the slave open must not prevent us from reporting the exit.
                let _ = done_rx.recv_timeout(Duration::from_millis(500));
                listener.shared.exited.store(true, Ordering::Release);
                listener.shared.dirty.store(true, Ordering::Release);
                listener.emit(TerminalEventKind::ContentChanged);
                listener.emit(TerminalEventKind::Exited { code });
            })?;
        }

        Ok(Arc::new(Session {
            id,
            term,
            shared,
            write_tx,
            master,
            #[cfg(not(unix))]
            killer: Mutex::new(killer),
            #[cfg(unix)]
            wake,
            tx,
            shell,
            pid,
        }))
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn shell(&self) -> &ShellSpec {
        &self.shell
    }

    pub fn pid(&self) -> Option<u32> {
        self.pid
    }

    pub fn title(&self) -> Option<String> {
        self.shared.title.lock().clone()
    }

    /// Latest OSC 52 clipboard-store request (re-arms the `ClipboardStore` event).
    pub fn take_clipboard(&self) -> Option<String> {
        self.shared.clipboard_pending.store(false, Ordering::Release);
        self.shared.clipboard.lock().take()
    }

    pub fn is_exited(&self) -> bool {
        self.shared.exited.load(Ordering::Acquire)
    }

    /// Exit code once the child has exited.
    pub fn exit_code(&self) -> Option<i32> {
        *self.shared.exit_code.lock()
    }

    /// Current size as (cols, rows).
    pub fn size(&self) -> (u16, u16) {
        (self.shared.cols.load(Ordering::Relaxed), self.shared.rows.load(Ordering::Relaxed))
    }

    /// Replace the color palette (takes effect on the next snapshot).
    pub fn set_palette(&self, palette: Palette) {
        *self.shared.palette.lock() = palette;
        self.mark_dirty();
    }

    /// Queue raw bytes (keyboard input) for the child. Never blocks: the actual PTY write
    /// happens on the session's writer thread, in order with emulator replies.
    pub fn write(&self, data: &[u8]) -> Result<(), TerminalError> {
        if self.is_exited() {
            return Err(TerminalError::Exited(self.id.clone()));
        }
        if self.shared.write_failed.load(Ordering::Acquire) {
            return Err(std::io::Error::from(std::io::ErrorKind::BrokenPipe).into());
        }
        self.write_tx.send(data.to_vec()).map_err(|_| TerminalError::Exited(self.id.clone()))
    }

    /// Paste text, wrapping it in bracketed-paste markers when the app enabled that mode.
    pub fn paste(&self, text: &str) -> Result<(), TerminalError> {
        let bracketed = self
            .term
            .lock()
            .mode()
            .contains(alacritty_terminal::term::TermMode::BRACKETED_PASTE);
        if bracketed {
            let sanitized = text.replace('\x1b', "");
            self.write(format!("\x1b[200~{sanitized}\x1b[201~").as_bytes())
        } else {
            self.write(text.replace("\r\n", "\r").replace('\n', "\r").as_bytes())
        }
    }

    /// `cd` the running shell to `path` (port of `setWorkingDirectory`).
    pub fn change_directory(&self, path: &str) -> Result<(), TerminalError> {
        let kind: ShellKind = self.shell.kind;
        self.write(cd_command(kind, path).as_bytes())
    }

    pub fn resize(&self, cols: u16, rows: u16) -> Result<(), TerminalError> {
        if cols == 0 || rows == 0 {
            return Err(TerminalError::InvalidArgument(format!("size {cols}x{rows}")));
        }
        if let Some(m) = self.master.lock().as_ref() {
            m.resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })
                .map_err(|e| TerminalError::Pty(e.to_string()))?;
        }
        self.term.lock().resize(Size { cols: cols as usize, rows: rows as usize });
        self.shared.cols.store(cols, Ordering::Relaxed);
        self.shared.rows.store(rows, Ordering::Relaxed);
        self.mark_dirty();
        Ok(())
    }

    /// Terminate the child: SIGHUP to its process group, then SIGKILL to the group if it is
    /// still alive 1.5 s later (Unix); TerminateProcess (Windows). Never signals a reaped pid.
    pub fn kill(&self) -> Result<(), TerminalError> {
        #[cfg(unix)]
        {
            let Some(pid) = self.pid else { return Ok(()) };
            {
                let reaped = self.shared.reaped.lock();
                if *reaped {
                    return Ok(());
                }
                signal_group(pid, libc::SIGHUP);
            }
            if !self.shared.escalating.swap(true, Ordering::AcqRel) {
                let shared = self.shared.clone();
                let _ = std::thread::Builder::new()
                    .name(format!("vsm-term-kill-{}", self.id))
                    .spawn(move || {
                        let deadline = std::time::Instant::now() + KILL_ESCALATION;
                        let mut reaped = shared.reaped.lock();
                        while !*reaped {
                            if shared.reaped_cv.wait_until(&mut reaped, deadline).timed_out() {
                                break;
                            }
                        }
                        if !*reaped {
                            signal_group(pid, libc::SIGKILL);
                        }
                    });
            }
            Ok(())
        }
        #[cfg(not(unix))]
        {
            if self.is_exited() {
                return Ok(());
            }
            match self.killer.lock().kill() {
                Ok(()) => Ok(()),
                Err(_) if self.is_exited() => Ok(()),
                Err(e) => Err(e.into()),
            }
        }
    }

    /// Kill the process and release the session's threads and buffers, even if a background
    /// process keeps the PTY open. The session stays readable (last screen) but is dead.
    pub fn close(&self) {
        let _ = self.kill();
        #[cfg(unix)]
        self.wake.fire();
        #[cfg(not(unix))]
        {
            self.master.lock().take();
        }
    }

    fn mark_dirty(&self) {
        if !self.shared.dirty.swap(true, Ordering::AcqRel) {
            let _ = self.tx.send(TerminalEvent {
                id: self.id.clone(),
                kind: TerminalEventKind::ContentChanged,
            });
        }
    }

    /// Re-arm the coalesced `ContentChanged`, `Bell` and `TitleChanged` events without taking a
    /// snapshot.
    pub fn acknowledge(&self) {
        self.shared.dirty.store(false, Ordering::Release);
        self.shared.bell_pending.store(false, Ordering::Release);
        self.shared.title_pending.store(false, Ordering::Release);
    }

    /// Scroll the viewport through the scrollback.
    pub fn scroll(&self, cmd: ScrollCmd) {
        let scroll = match cmd {
            ScrollCmd::Lines(n) => Scroll::Delta(n),
            ScrollCmd::PageUp => Scroll::PageUp,
            ScrollCmd::PageDown => Scroll::PageDown,
            ScrollCmd::Top => Scroll::Top,
            ScrollCmd::Bottom => Scroll::Bottom,
        };
        let changed = {
            let mut t = self.term.lock();
            let before = t.grid().display_offset();
            t.scroll_display(scroll);
            before != t.grid().display_offset()
        };
        if changed {
            self.mark_dirty();
        }
    }

    // ---- selection (viewport coordinates: row 0 = top visible row) ----

    fn viewport_point(term: &Term<Listener>, row: usize, col: usize) -> Point {
        let row = row.min(term.screen_lines().saturating_sub(1));
        let col = col.min(term.columns().saturating_sub(1));
        viewport_to_point(term.grid().display_offset(), Point::new(row, Column(col)))
    }

    pub fn selection_start(&self, row: usize, col: usize, side: SelectionSide, kind: SelectionKind) {
        {
            let mut t = self.term.lock();
            let p = Self::viewport_point(&t, row, col);
            let ty = match kind {
                SelectionKind::Simple => SelectionType::Simple,
                SelectionKind::Block => SelectionType::Block,
                SelectionKind::Semantic => SelectionType::Semantic,
                SelectionKind::Lines => SelectionType::Lines,
            };
            t.selection = Some(Selection::new(ty, p, side_of(side)));
        }
        self.mark_dirty();
    }

    pub fn selection_update(&self, row: usize, col: usize, side: SelectionSide) {
        {
            let mut t = self.term.lock();
            let p = Self::viewport_point(&t, row, col);
            if let Some(sel) = t.selection.as_mut() {
                sel.update(p, side_of(side));
            }
        }
        self.mark_dirty();
    }

    pub fn selection_clear(&self) {
        let had = self.term.lock().selection.take().is_some();
        if had {
            self.mark_dirty();
        }
    }

    /// Select the entire buffer (scrollback included).
    pub fn select_all(&self) {
        {
            let mut t = self.term.lock();
            let top = t.topmost_line();
            let bottom = t.bottommost_line();
            let last = t.last_column();
            let mut sel = Selection::new(SelectionType::Simple, Point::new(top, Column(0)), Side::Left);
            sel.update(Point::new(bottom, last), Side::Right);
            t.selection = Some(sel);
        }
        self.mark_dirty();
    }

    pub fn has_selection(&self) -> bool {
        self.term.lock().selection.as_ref().is_some_and(|s| !s.is_empty())
    }

    /// Text of the current selection.
    pub fn selection_text(&self) -> Option<String> {
        self.term.lock().selection_to_string()
    }

    /// Entire buffer (scrollback + screen) as text, trailing blank lines removed.
    pub fn full_text(&self) -> String {
        let t = self.term.lock();
        let s = t.bounds_to_string(
            Point::new(t.topmost_line(), Column(0)),
            Point::new(t.bottommost_line(), t.last_column()),
        );
        s.trim_end().to_string()
    }

    /// Drop all scrollback history.
    pub fn clear_history(&self) {
        self.term.lock().grid_mut().clear_history();
        self.mark_dirty();
    }

    /// Capture the visible grid. Also re-arms the coalesced events (see [`Session::acknowledge`]).
    pub fn snapshot(&self) -> GridSnapshot {
        self.acknowledge();
        let term = self.term.lock();
        let palette = self.shared.palette.lock().clone();
        let grid = term.grid();
        let rows = grid.screen_lines();
        let cols = grid.columns();
        let offset = grid.display_offset();
        let history_size = grid.history_size();
        let colors = term.colors();
        let overrides = |i: usize| colors[i];
        let content = term.renderable_content();
        let selection = content.selection;

        let mut cells = Vec::with_capacity(rows * cols);
        for r in 0..rows {
            let line = Line(r as i32 - offset as i32);
            let row = &grid[line];
            for c in 0..cols {
                let cell = &row[Column(c)];
                let f = cell.flags;
                let mut fg = palette.resolve(cell.fg, &overrides);
                let mut bg = palette.resolve(cell.bg, &overrides);
                let inverse = f.contains(Flags::INVERSE);
                let default_bg = matches!(cell.bg, Color::Named(NamedColor::Background)) && !inverse;
                if f.contains(Flags::DIM) {
                    fg = fg.dim();
                }
                if inverse {
                    std::mem::swap(&mut fg, &mut bg);
                }
                let mut bits = 0u16;
                let mut set = |cond: bool, bit: u16| {
                    if cond {
                        bits |= bit;
                    }
                };
                set(f.contains(Flags::BOLD), CellFlags::BOLD);
                set(f.contains(Flags::ITALIC), CellFlags::ITALIC);
                set(f.contains(Flags::UNDERLINE), CellFlags::UNDERLINE);
                set(f.contains(Flags::DOUBLE_UNDERLINE), CellFlags::DOUBLE_UNDERLINE);
                set(f.contains(Flags::UNDERCURL), CellFlags::UNDERCURL);
                set(f.contains(Flags::DOTTED_UNDERLINE), CellFlags::DOTTED_UNDERLINE);
                set(f.contains(Flags::DASHED_UNDERLINE), CellFlags::DASHED_UNDERLINE);
                set(f.contains(Flags::STRIKEOUT), CellFlags::STRIKEOUT);
                set(f.contains(Flags::DIM), CellFlags::DIM);
                set(f.contains(Flags::HIDDEN), CellFlags::HIDDEN);
                set(f.contains(Flags::WIDE_CHAR), CellFlags::WIDE);
                set(
                    f.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER),
                    CellFlags::WIDE_SPACER,
                );
                set(inverse, CellFlags::INVERSE);
                set(default_bg, CellFlags::DEFAULT_BG);
                set(
                    selection.is_some_and(|s| s.contains(Point::new(line, Column(c)))),
                    CellFlags::SELECTED,
                );
                cells.push(Cell {
                    ch: cell.c,
                    zerowidth: cell.zerowidth().map(|z| z.to_vec().into_boxed_slice()),
                    fg,
                    bg,
                    flags: CellFlags(bits),
                });
            }
        }

        let cur = content.cursor;
        let cur_row = cur.point.line.0 + offset as i32;
        let shape: CursorShape = cur.shape.into();
        let cursor = CursorSnapshot {
            row: cur_row.max(0) as usize,
            col: cur.point.column.0,
            shape,
            visible: shape != CursorShape::Hidden && cur_row >= 0 && (cur_row as usize) < rows,
        };

        use alacritty_terminal::term::TermMode as M;
        let m = content.mode;
        let mode = ModeFlags {
            alt_screen: m.contains(M::ALT_SCREEN),
            app_cursor: m.contains(M::APP_CURSOR),
            app_keypad: m.contains(M::APP_KEYPAD),
            bracketed_paste: m.contains(M::BRACKETED_PASTE),
            mouse_reporting: m.intersects(M::MOUSE_MODE),
            mouse_drag: m.contains(M::MOUSE_DRAG),
            mouse_motion: m.contains(M::MOUSE_MOTION),
            sgr_mouse: m.contains(M::SGR_MOUSE),
            focus_reporting: m.contains(M::FOCUS_IN_OUT),
            alternate_scroll: m.contains(M::ALTERNATE_SCROLL),
        };

        GridSnapshot {
            cols,
            rows,
            cells,
            cursor,
            display_offset: offset,
            history_size,
            title: self.shared.title.lock().clone(),
            mode,
            default_fg: palette.foreground,
            default_bg: palette.background,
            cursor_color: palette.cursor,
        }
    }
}

fn side_of(s: SelectionSide) -> Side {
    match s {
        SelectionSide::Left => Side::Left,
        SelectionSide::Right => Side::Right,
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.close();
    }
}
