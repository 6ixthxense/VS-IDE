//! vsm-terminal — UI-agnostic terminal backend.
//!
//! Replaces `src/main/services/terminal.ts` + `src/main/ipc/terminal.ts` (node-pty + xterm).
//!
//! * PTY: [`portable-pty`](https://docs.rs/portable-pty) (ConPTY on Windows, openpty on Unix).
//! * VT parsing + grid/scrollback/selection: `alacritty_terminal` (Apache-2.0).
//! * One blocking reader thread per session (0 CPU while idle) which feeds the parser and
//!   coalesces "content changed" wakeups: at most one [`TerminalEventKind::ContentChanged`] is
//!   queued until the consumer calls [`Session::snapshot`] again.
//!
//! Typical use:
//!
//! ```no_run
//! use vsm_terminal::{SessionConfig, TerminalManager};
//! let mgr = TerminalManager::new();
//! let events = mgr.events();
//! mgr.create("default", SessionConfig::default()).unwrap();
//! mgr.write("default", b"echo hi\r").unwrap();
//! while let Ok(ev) = events.recv() {
//!     // redraw on ev.kind == ContentChanged, using mgr.snapshot("default")
//!     # break;
//! }
//! ```

mod error;
mod manager;
mod resolve;
mod session;
mod shell;
mod snapshot;
#[cfg(unix)]
mod unixio;

pub use error::TerminalError;
pub use manager::TerminalManager;
pub use session::{
    ScrollCmd, SelectionKind, SelectionSide, Session, SessionConfig, TerminalEvent,
    TerminalEventKind,
};
pub use shell::{default_shell, detect_shell, Os, ShellKind, ShellSpec};
pub use snapshot::{
    Cell, CellFlags, CursorShape, CursorSnapshot, GridSnapshot, ModeFlags, Palette, Rgb,
};
