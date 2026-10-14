//! vsm-lsp — generic Language Server Protocol client over stdio.
//!
//! Replaces Monaco's built-in TypeScript worker with real language servers
//! (see docs/native-rewrite-plan.md, phase 3).
//!
//! Layers:
//! * [`framing`]: `Content-Length` JSON-RPC framing.
//! * [`LspClient`]: one connection to one server (requests with timeouts and cancellation,
//!   notifications, `publishDiagnostics` callback, document sync, typed feature helpers).
//! * [`LanguageRegistry`]: which server command handles which language (configurable).
//! * [`LspManager`]: lazy start per server, document tracking, and automatic stop when the last
//!   document of a server is closed.
//!
//! Threading: each client has exactly one blocking reader thread (plus one stderr drain thread
//! for spawned servers). Nothing polls, so an idle client costs no CPU. Request methods block
//! the calling thread (with a timeout); call them from a background executor, never from the
//! UI thread. Event-handler callbacks run on the reader thread and must not issue blocking
//! requests themselves.

mod client;
mod error;
pub mod framing;
mod handler;
#[cfg(windows)]
mod job;
mod manager;
mod registry;
pub mod text;

pub use client::{ConnectOptions, LspClient, PendingRequest, DEFAULT_INIT_TIMEOUT, DEFAULT_REQUEST_TIMEOUT};
pub use error::LspError;
pub use handler::{diagnostics_callback, LogLevel, LspEventHandler, NoopHandler, ServerStatus};
pub use manager::{LspManager, ManagerOptions};
pub use registry::{resolve_executable, LanguageRegistry, ServerConfig};

// Re-export the protocol types so consumers don't need a direct `lsp-types` dependency.
pub use lsp_types;
