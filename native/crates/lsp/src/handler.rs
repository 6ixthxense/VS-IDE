use std::sync::Arc;

use lsp_types::PublishDiagnosticsParams;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Error,
    Warning,
    Info,
    Log,
    /// Raw stderr line from the server process.
    Stderr,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerStatus {
    /// Initialize handshake completed.
    Started,
    /// Connection closed. `crashed` is true when we did not ask for the shutdown.
    Stopped { crashed: bool },
}

/// Sink for server-initiated events. All callbacks run on the client's reader thread (or the
/// stderr thread for [`LogLevel::Stderr`]); keep them short and never block on the same client.
pub trait LspEventHandler: Send + Sync {
    fn diagnostics(&self, _server: &str, _params: PublishDiagnosticsParams) {}
    fn log(&self, _server: &str, _level: LogLevel, _message: &str) {}
    fn status(&self, _server: &str, _status: ServerStatus) {}
}

/// Handler that ignores everything.
pub struct NoopHandler;
impl LspEventHandler for NoopHandler {}

struct DiagFn<F>(F);

impl<F> LspEventHandler for DiagFn<F>
where
    F: Fn(PublishDiagnosticsParams) + Send + Sync,
{
    fn diagnostics(&self, _server: &str, params: PublishDiagnosticsParams) {
        (self.0)(params)
    }
}

/// Handler that only forwards `publishDiagnostics` to a closure.
pub fn diagnostics_callback<F>(f: F) -> Arc<dyn LspEventHandler>
where
    F: Fn(PublishDiagnosticsParams) + Send + Sync + 'static,
{
    Arc::new(DiagFn(f))
}
