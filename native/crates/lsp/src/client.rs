//! One connection to one language server.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, OnceLock, Weak};
use std::time::{Duration, Instant};

use crossbeam_channel::{bounded, unbounded, Receiver, Sender, TryRecvError};
use lsp_types::notification::{
    DidChangeTextDocument, DidCloseTextDocument, DidOpenTextDocument, DidSaveTextDocument, Exit,
    Initialized, Notification,
};
use lsp_types::request::{
    Completion, DocumentSymbolRequest, GotoDefinition, HoverRequest, Initialize, References,
    Rename, Request, Shutdown,
};
use lsp_types::{
    CompletionParams, CompletionResponse, DidChangeTextDocumentParams, DidCloseTextDocumentParams,
    DidOpenTextDocumentParams, DidSaveTextDocumentParams, DocumentSymbolParams,
    DocumentSymbolResponse, GotoDefinitionParams, GotoDefinitionResponse, Hover, HoverParams,
    InitializeParams, InitializeResult, Location, Position, PublishDiagnosticsParams,
    ReferenceContext, ReferenceParams, RenameParams, ServerCapabilities, TextDocumentContentChangeEvent,
    TextDocumentIdentifier, TextDocumentItem, TextDocumentPositionParams, TextDocumentSyncCapability,
    TextDocumentSyncKind, Uri, VersionedTextDocumentIdentifier, WorkspaceEdit,
};
use parking_lot::Mutex;
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{json, Value};

use crate::error::LspError;
use crate::framing::read_message;
use crate::handler::{LogLevel, LspEventHandler, NoopHandler, ServerStatus};
use crate::registry::{resolve_executable, ServerConfig};
use crate::text::path_to_uri;

pub const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
pub const DEFAULT_INIT_TIMEOUT: Duration = Duration::from_secs(30);

const REQUEST_CANCELLED: i64 = -32800;
const CONTENT_MODIFIED: i64 = -32801;
const METHOD_NOT_FOUND: i64 = -32601;

type Reply = Result<Value, LspError>;

/// Message for the writer thread.
enum WriterMsg {
    Frame(Vec<u8>),
    /// Drop the write end (closes the server's stdin) and stop.
    Close,
}

struct Inner {
    name: String,
    /// Frames are queued here and written by a dedicated thread, so neither request callers nor
    /// the reader thread (which answers server requests) can block on a stalled server pipe.
    writer_tx: Sender<WriterMsg>,
    /// `workspace/workspaceFolders` answer.
    workspace_folders: Value,
    pending: Mutex<HashMap<i64, Sender<Reply>>>,
    next_id: AtomicI64,
    alive: AtomicBool,
    shutdown_requested: AtomicBool,
    capabilities: OnceLock<ServerCapabilities>,
    handler: Arc<dyn LspEventHandler>,
    request_timeout: Mutex<Duration>,
}

impl Inner {
    fn send(&self, msg: &Value) -> Result<(), LspError> {
        if !self.alive.load(Ordering::Acquire) {
            return Err(LspError::ServerExited);
        }
        let body = serde_json::to_vec(msg)?;
        let mut frame = Vec::with_capacity(body.len() + 32);
        frame.extend_from_slice(format!("Content-Length: {}\r\n\r\n", body.len()).as_bytes());
        frame.extend_from_slice(&body);
        self.writer_tx.send(WriterMsg::Frame(frame)).map_err(|_| LspError::ServerExited)
    }

    fn fail_pending(&self) {
        let pending: Vec<_> = self.pending.lock().drain().collect();
        for (_, tx) in pending {
            let _ = tx.send(Err(LspError::ServerExited));
        }
    }

    fn notify_raw(&self, method: &str, params: Value) -> Result<(), LspError> {
        self.send(&json!({"jsonrpc": "2.0", "method": method, "params": params}))
    }

    fn cancel(&self, id: i64) {
        let removed = self.pending.lock().remove(&id).is_some();
        if removed && self.alive.load(Ordering::Acquire) {
            let _ = self.notify_raw("$/cancelRequest", json!({ "id": id }));
        }
    }
}

/// Options for [`LspClient::connect`].
pub struct ConnectOptions {
    pub root: Option<PathBuf>,
    pub initialization_options: Option<Value>,
    pub init_timeout: Duration,
    pub handler: Arc<dyn LspEventHandler>,
}

impl Default for ConnectOptions {
    fn default() -> Self {
        ConnectOptions {
            root: None,
            initialization_options: None,
            init_timeout: DEFAULT_INIT_TIMEOUT,
            handler: Arc::new(NoopHandler),
        }
    }
}

/// A request in flight. Dropping it before the answer arrives cancels it
/// (`$/cancelRequest`), which is what an editor wants for stale completion/hover requests.
pub struct PendingRequest<T> {
    id: i64,
    method: &'static str,
    rx: Receiver<Reply>,
    inner: Arc<Inner>,
    finished: bool,
    _marker: PhantomData<fn() -> T>,
}

impl<T: DeserializeOwned> PendingRequest<T> {
    pub fn id(&self) -> i64 {
        self.id
    }

    fn decode(&mut self, reply: Reply) -> Result<T, LspError> {
        self.finished = true;
        let value = reply?;
        Ok(serde_json::from_value(value)?)
    }

    /// Block for the answer. On timeout the request is cancelled and [`LspError::Timeout`]
    /// returned.
    pub fn wait(mut self, timeout: Duration) -> Result<T, LspError> {
        match self.rx.recv_timeout(timeout) {
            Ok(reply) => self.decode(reply),
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                self.inner.cancel(self.id);
                self.finished = true;
                Err(LspError::Timeout { method: self.method.to_string(), after: timeout })
            }
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                self.finished = true;
                Err(LspError::ServerExited)
            }
        }
    }

    /// Non-blocking poll. `None` = not answered yet.
    pub fn try_recv(&mut self) -> Option<Result<T, LspError>> {
        match self.rx.try_recv() {
            Ok(reply) => Some(self.decode(reply)),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                self.finished = true;
                Some(Err(LspError::ServerExited))
            }
        }
    }

    /// Cancel explicitly (same as dropping).
    pub fn cancel(self) {}
}

impl<T> Drop for PendingRequest<T> {
    fn drop(&mut self) {
        if !self.finished {
            self.inner.cancel(self.id);
        }
    }
}

/// Connection to a running language server. Cheap to share behind an `Arc`.
pub struct LspClient {
    inner: Arc<Inner>,
    child: Mutex<Option<Proc>>,
}

impl LspClient {
    /// Spawn `config`'s server process and run the initialize handshake.
    pub fn spawn(
        config: &ServerConfig,
        root: &Path,
        handler: Arc<dyn LspEventHandler>,
        init_timeout: Duration,
    ) -> Result<LspClient, LspError> {
        let program = resolve_executable(&config.command).ok_or_else(|| LspError::ServerNotFound {
            server: config.id.clone(),
            command: config.command.clone(),
        })?;
        Self::spawn_program(config, &program, root, handler, init_timeout)
    }

    fn spawn_program(
        config: &ServerConfig,
        program: &Path,
        root: &Path,
        handler: Arc<dyn LspEventHandler>,
        init_timeout: Duration,
    ) -> Result<LspClient, LspError> {
        let mut cmd = Command::new(program);
        cmd.args(&config.args)
            .envs(config.env.iter().cloned())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if root.is_dir() {
            cmd.current_dir(root);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        #[cfg(unix)]
        {
            // Own process group, so the whole tree (tsserver & co.) can be killed with one signal.
            use std::os::unix::process::CommandExt;
            cmd.process_group(0);
        }
        let mut child = cmd.spawn().map_err(|e| LspError::Spawn {
            server: config.id.clone(),
            message: format!("{}: {e}", program.display()),
        })?;
        let stdin = child.stdin.take();
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let mut proc = Proc::new(child);
        let (Some(stdin), Some(stdout)) = (stdin, stdout) else {
            proc.kill_tree();
            return Err(LspError::Spawn { server: config.id.clone(), message: "no stdio pipes".into() });
        };
        if let Some(stderr) = stderr {
            let name = config.id.clone();
            let h = handler.clone();
            let _ = std::thread::Builder::new()
                .name(format!("vsm-lsp-err-{name}"))
                .spawn(move || drain_stderr(stderr, &name, h.as_ref()));
        }
        let opts = ConnectOptions {
            root: Some(root.to_path_buf()),
            initialization_options: config.initialization_options.clone(),
            init_timeout,
            handler,
        };
        match Self::connect_inner(&config.id, Box::new(stdout), Box::new(stdin), Some(proc), opts) {
            Ok(c) => Ok(c),
            Err((e, proc)) => {
                if let Some(mut p) = proc {
                    p.kill_tree();
                }
                Err(e)
            }
        }
    }

    /// Run the handshake over arbitrary streams (used for in-process servers and tests).
    pub fn connect(
        name: &str,
        reader: Box<dyn Read + Send>,
        writer: Box<dyn Write + Send>,
        opts: ConnectOptions,
    ) -> Result<LspClient, LspError> {
        Self::connect_inner(name, reader, writer, None, opts).map_err(|(e, _)| e)
    }

    #[allow(clippy::type_complexity, clippy::result_large_err)]
    fn connect_inner(
        name: &str,
        reader: Box<dyn Read + Send>,
        writer: Box<dyn Write + Send>,
        proc: Option<Proc>,
        opts: ConnectOptions,
    ) -> Result<LspClient, (LspError, Option<Proc>)> {
        let (root_uri, folders) = match workspace_folders(opts.root.as_deref()) {
            Ok(v) => v,
            Err(e) => return Err((e, proc)),
        };
        let (writer_tx, writer_rx) = unbounded::<WriterMsg>();
        let inner = Arc::new(Inner {
            name: name.to_string(),
            writer_tx,
            workspace_folders: folders.clone(),
            pending: Mutex::new(HashMap::new()),
            next_id: AtomicI64::new(1),
            alive: AtomicBool::new(true),
            shutdown_requested: AtomicBool::new(false),
            capabilities: OnceLock::new(),
            handler: opts.handler.clone(),
            request_timeout: Mutex::new(DEFAULT_REQUEST_TIMEOUT),
        });
        {
            let weak = Arc::downgrade(&inner);
            let spawned = std::thread::Builder::new()
                .name(format!("vsm-lsp-write-{name}"))
                .spawn(move || writer_loop(weak, writer, writer_rx));
            if let Err(e) = spawned {
                return Err((e.into(), proc));
            }
        }
        {
            let for_reader = inner.clone();
            let spawned = std::thread::Builder::new()
                .name(format!("vsm-lsp-read-{name}"))
                .spawn(move || reader_loop(for_reader, reader));
            if let Err(e) = spawned {
                inner.alive.store(false, Ordering::Release);
                let _ = inner.writer_tx.send(WriterMsg::Close);
                return Err((e.into(), proc));
            }
        }
        let client = LspClient { inner, child: Mutex::new(proc) };
        if let Err(e) = client.initialize(&opts, root_uri.as_ref(), &folders) {
            let proc = client.child.lock().take();
            client.inner.alive.store(false, Ordering::Release);
            return Err((e, proc));
        }
        Ok(client)
    }

    fn initialize(&self, opts: &ConnectOptions, root_uri: Option<&Uri>, folders: &Value) -> Result<(), LspError> {
        let params: InitializeParams = serde_json::from_value(json!({
            "processId": std::process::id(),
            "clientInfo": { "name": "VS-Monitor IDE", "version": env!("CARGO_PKG_VERSION") },
            "locale": "en",
            "rootUri": root_uri.map(|u| u.as_str()),
            "workspaceFolders": folders,
            "initializationOptions": opts.initialization_options,
            "capabilities": client_capabilities(),
        }))?;
        let result: InitializeResult = self.request::<Initialize>(params, opts.init_timeout)?;
        let _ = self.inner.capabilities.set(result.capabilities);
        // `ServerStatus::Started` is announced by `LspManager` (after it registered the server
        // and released its locks), not from here: a handler may re-enter the manager.
        self.notify::<Initialized>(lsp_types::InitializedParams {})?;
        Ok(())
    }

    pub fn name(&self) -> &str {
        &self.inner.name
    }

    pub fn is_alive(&self) -> bool {
        self.inner.alive.load(Ordering::Acquire)
    }

    pub fn capabilities(&self) -> &ServerCapabilities {
        static EMPTY: OnceLock<ServerCapabilities> = OnceLock::new();
        self.inner
            .capabilities
            .get()
            .unwrap_or_else(|| EMPTY.get_or_init(ServerCapabilities::default))
    }

    /// Default timeout used by the typed feature helpers (hover, completion, ...).
    pub fn set_request_timeout(&self, t: Duration) {
        *self.inner.request_timeout.lock() = t;
    }

    pub fn request_timeout(&self) -> Duration {
        *self.inner.request_timeout.lock()
    }

    /// How the server wants `didChange` delivered.
    pub fn sync_kind(&self) -> TextDocumentSyncKind {
        match &self.capabilities().text_document_sync {
            Some(TextDocumentSyncCapability::Kind(k)) => *k,
            Some(TextDocumentSyncCapability::Options(o)) => {
                o.change.unwrap_or(TextDocumentSyncKind::NONE)
            }
            None => TextDocumentSyncKind::NONE,
        }
    }

    fn wants_save(&self) -> bool {
        use lsp_types::TextDocumentSyncSaveOptions as S;
        matches!(&self.capabilities().text_document_sync,
            Some(TextDocumentSyncCapability::Options(o))
                if matches!(&o.save, Some(S::SaveOptions(_)) | Some(S::Supported(true))))
    }

    fn save_includes_text(&self) -> bool {
        use lsp_types::TextDocumentSyncSaveOptions as S;
        match &self.capabilities().text_document_sync {
            Some(TextDocumentSyncCapability::Options(o)) => match &o.save {
                Some(S::SaveOptions(s)) => s.include_text.unwrap_or(false),
                _ => false,
            },
            _ => false,
        }
    }

    // ---- generic request / notification ----

    pub fn notify<N>(&self, params: N::Params) -> Result<(), LspError>
    where
        N: Notification,
        N::Params: Serialize,
    {
        self.inner.notify_raw(N::METHOD, serde_json::to_value(params)?)
    }

    /// Send a request and return a handle to wait on / cancel.
    pub fn request_async<R>(&self, params: R::Params) -> Result<PendingRequest<R::Result>, LspError>
    where
        R: Request,
        R::Params: Serialize,
        R::Result: DeserializeOwned,
    {
        let id = self.inner.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = bounded(1);
        // Register before sending so a fast reply cannot be missed.
        self.inner.pending.lock().insert(id, tx);
        if !self.is_alive() {
            self.inner.pending.lock().remove(&id);
            return Err(LspError::ServerExited);
        }
        let msg = json!({
            "jsonrpc": "2.0", "id": id, "method": R::METHOD,
            "params": serde_json::to_value(params)?,
        });
        if let Err(e) = self.inner.send(&msg) {
            self.inner.pending.lock().remove(&id);
            return Err(e);
        }
        Ok(PendingRequest {
            id,
            method: R::METHOD,
            rx,
            inner: self.inner.clone(),
            finished: false,
            _marker: PhantomData,
        })
    }

    /// Send a request and wait up to `timeout` (cancelling it on timeout).
    pub fn request<R>(&self, params: R::Params, timeout: Duration) -> Result<R::Result, LspError>
    where
        R: Request,
        R::Params: Serialize,
        R::Result: DeserializeOwned,
    {
        self.request_async::<R>(params)?.wait(timeout)
    }

    // ---- document sync ----

    pub fn did_open(
        &self,
        uri: Uri,
        language_id: &str,
        version: i32,
        text: String,
    ) -> Result<(), LspError> {
        self.notify::<DidOpenTextDocument>(DidOpenTextDocumentParams {
            text_document: TextDocumentItem { uri, language_id: language_id.to_string(), version, text },
        })
    }

    /// Notify a change. Pass incremental `changes` (ranges relative to the previous version) and,
    /// when available, the full new text. The right form is chosen from the server's sync
    /// capability: servers that only support full sync get `full_text` (error
    /// [`LspError::NeedsFullText`] if it was not provided); servers that support incremental
    /// sync get `changes` (or `full_text` if `changes` is empty); servers with no change sync
    /// get nothing.
    pub fn did_change(
        &self,
        uri: Uri,
        version: i32,
        changes: Vec<TextDocumentContentChangeEvent>,
        full_text: Option<&str>,
    ) -> Result<(), LspError> {
        let full_event = |t: &str| TextDocumentContentChangeEvent { range: None, range_length: None, text: t.to_string() };
        let content_changes = match self.sync_kind() {
            TextDocumentSyncKind::NONE => return Ok(()),
            TextDocumentSyncKind::FULL => {
                vec![full_event(full_text.ok_or(LspError::NeedsFullText)?)]
            }
            _ => {
                if changes.is_empty() {
                    vec![full_event(full_text.ok_or(LspError::NeedsFullText)?)]
                } else {
                    changes
                }
            }
        };
        self.notify::<DidChangeTextDocument>(DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier { uri, version },
            content_changes,
        })
    }

    /// `didSave`; text is only sent when the server asked for it.
    pub fn did_save(&self, uri: Uri, text: Option<&str>) -> Result<(), LspError> {
        if !self.wants_save() {
            return Ok(());
        }
        let text = if self.save_includes_text() { text.map(str::to_string) } else { None };
        self.notify::<DidSaveTextDocument>(DidSaveTextDocumentParams {
            text_document: TextDocumentIdentifier { uri },
            text,
        })
    }

    pub fn did_close(&self, uri: Uri) -> Result<(), LspError> {
        self.notify::<DidCloseTextDocument>(DidCloseTextDocumentParams {
            text_document: TextDocumentIdentifier { uri },
        })
    }

    // ---- language features (blocking, default timeout) ----

    pub fn hover(&self, uri: Uri, position: Position) -> Result<Option<Hover>, LspError> {
        self.request::<HoverRequest>(
            HoverParams {
                text_document_position_params: tdpp(uri, position),
                work_done_progress_params: Default::default(),
            },
            self.request_timeout(),
        )
    }

    pub fn completion_async(
        &self,
        uri: Uri,
        position: Position,
    ) -> Result<PendingRequest<Option<CompletionResponse>>, LspError> {
        self.request_async::<Completion>(CompletionParams {
            text_document_position: tdpp(uri, position),
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
            context: None,
        })
    }

    pub fn completion(&self, uri: Uri, position: Position) -> Result<Option<CompletionResponse>, LspError> {
        self.completion_async(uri, position)?.wait(self.request_timeout())
    }

    pub fn definition(&self, uri: Uri, position: Position) -> Result<Option<GotoDefinitionResponse>, LspError> {
        self.request::<GotoDefinition>(
            GotoDefinitionParams {
                text_document_position_params: tdpp(uri, position),
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            },
            self.request_timeout(),
        )
    }

    pub fn references(
        &self,
        uri: Uri,
        position: Position,
        include_declaration: bool,
    ) -> Result<Option<Vec<Location>>, LspError> {
        self.request::<References>(
            ReferenceParams {
                text_document_position: tdpp(uri, position),
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
                context: ReferenceContext { include_declaration },
            },
            self.request_timeout(),
        )
    }

    pub fn rename(
        &self,
        uri: Uri,
        position: Position,
        new_name: &str,
    ) -> Result<Option<WorkspaceEdit>, LspError> {
        self.request::<Rename>(
            RenameParams {
                text_document_position: tdpp(uri, position),
                new_name: new_name.to_string(),
                work_done_progress_params: Default::default(),
            },
            self.request_timeout(),
        )
    }

    pub fn document_symbols(&self, uri: Uri) -> Result<Option<DocumentSymbolResponse>, LspError> {
        self.request::<DocumentSymbolRequest>(
            DocumentSymbolParams {
                text_document: TextDocumentIdentifier { uri },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            },
            self.request_timeout(),
        )
    }

    // ---- lifecycle ----

    /// Graceful stop: `shutdown` request, `exit` notification, close the server's stdin, then
    /// wait for the process (killing its whole process tree if it lingers). Safe to call more
    /// than once.
    pub fn shutdown(&self, timeout: Duration) -> Result<(), LspError> {
        if self.inner.shutdown_requested.swap(true, Ordering::AcqRel) {
            return Ok(());
        }
        let mut result = Ok(());
        if self.is_alive() {
            if let Err(e) = self.request::<Shutdown>((), timeout) {
                result = Err(e);
            }
            let _ = self.inner.notify_raw(Exit::METHOD, Value::Null);
        }
        // Queued after `exit`: the writer thread drops the pipe, so the server sees EOF.
        let _ = self.inner.writer_tx.send(WriterMsg::Close);
        self.reap_child(Duration::from_secs(2));
        self.inner.alive.store(false, Ordering::Release);
        self.inner.fail_pending();
        result
    }

    fn reap_child(&self, grace: Duration) {
        let Some(mut proc) = self.child.lock().take() else { return };
        let deadline = Instant::now() + grace;
        loop {
            if proc.has_exited() {
                break;
            }
            if Instant::now() >= deadline {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        proc.kill_tree();
    }
}

impl Drop for LspClient {
    fn drop(&mut self) {
        if self.inner.alive.load(Ordering::Acquire) && !self.inner.shutdown_requested.load(Ordering::Acquire) {
            let _ = self.inner.notify_raw(Exit::METHOD, Value::Null);
        }
        self.inner.shutdown_requested.store(true, Ordering::Release);
        let _ = self.inner.writer_tx.send(WriterMsg::Close);
        if let Some(mut proc) = self.child.lock().take() {
            proc.kill_tree();
        }
    }
}

/// A spawned server process, killed as a tree.
struct Proc {
    child: Child,
    #[cfg(windows)]
    job: Option<crate::job::Job>,
}

impl Proc {
    fn new(child: Child) -> Proc {
        #[cfg(windows)]
        let job = crate::job::Job::new().and_then(|j| j.assign(&child).map(|_| j)).ok();
        Proc {
            child,
            #[cfg(windows)]
            job,
        }
    }

    /// True once the leader exited. On Linux/macOS this does not reap, so the pid (== process
    /// group id) stays valid for the group kill in [`Proc::kill_tree`].
    fn has_exited(&mut self) -> bool {
        #[cfg(any(target_os = "linux", target_os = "android", target_os = "macos"))]
        {
            // SAFETY: siginfo_t is plain data; waitid only writes to it.
            let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
            let r = unsafe {
                libc::waitid(
                    libc::P_PID,
                    self.child.id() as libc::id_t,
                    &mut info,
                    libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
                )
            };
            if r == 0 {
                // si_pid is 0 when nothing has exited yet.
                // SAFETY: reading si_pid after a successful waitid.
                return unsafe { info.si_pid() } != 0;
            }
            matches!(self.child.try_wait(), Ok(Some(_)) | Err(_))
        }
        #[cfg(not(any(target_os = "linux", target_os = "android", target_os = "macos")))]
        {
            matches!(self.child.try_wait(), Ok(Some(_)) | Err(_))
        }
    }

    /// Kill the server and everything it spawned (tsserver, node, ...), then reap it.
    fn kill_tree(&mut self) {
        #[cfg(unix)]
        {
            // The child leads its own process group (spawned with process_group(0)); it has not
            // been reaped yet, so its pid cannot have been recycled.
            // SAFETY: plain kill(2).
            unsafe {
                libc::kill(-(self.child.id() as libc::pid_t), libc::SIGKILL);
            }
        }
        #[cfg(windows)]
        if let Some(job) = &self.job {
            job.terminate();
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn writer_loop(inner: Weak<Inner>, mut writer: Box<dyn Write + Send>, rx: Receiver<WriterMsg>) {
    while let Ok(msg) = rx.recv() {
        match msg {
            WriterMsg::Frame(bytes) => {
                if writer.write_all(&bytes).and_then(|_| writer.flush()).is_err() {
                    if let Some(inner) = inner.upgrade() {
                        inner.alive.store(false, Ordering::Release);
                        inner.fail_pending();
                    }
                    break;
                }
            }
            WriterMsg::Close => break,
        }
    }
    // `writer` drops here: the server's stdin is closed.
}

fn tdpp(uri: Uri, position: Position) -> TextDocumentPositionParams {
    TextDocumentPositionParams { text_document: TextDocumentIdentifier { uri }, position }
}

fn drain_stderr(stderr: impl Read, name: &str, handler: &dyn LspEventHandler) {
    let mut r = BufReader::new(stderr);
    let mut buf = Vec::new();
    loop {
        buf.clear();
        match r.read_until(b'\n', &mut buf) {
            Ok(0) | Err(_) => break,
            Ok(_) => {
                let line = String::from_utf8_lossy(&buf);
                let line = line.trim_end();
                if !line.is_empty() {
                    handler.log(name, LogLevel::Stderr, line);
                }
            }
        }
    }
}

fn reader_loop(inner: Arc<Inner>, reader: Box<dyn Read + Send>) {
    let mut reader = BufReader::with_capacity(64 * 1024, reader);
    loop {
        match read_message(&mut reader) {
            Ok(Some(msg)) => dispatch(&inner, msg),
            Ok(None) => break,
            Err(LspError::Json(_)) => continue, // skip a malformed body, framing is still intact
            Err(_) => break,
        }
    }
    inner.alive.store(false, Ordering::Release);
    inner.fail_pending();
    let _ = inner.writer_tx.send(WriterMsg::Close);
    let crashed = !inner.shutdown_requested.load(Ordering::Acquire);
    inner.handler.status(&inner.name, ServerStatus::Stopped { crashed });
}

fn dispatch(inner: &Arc<Inner>, msg: Value) {
    let method = msg.get("method").and_then(Value::as_str).map(str::to_string);
    let id = msg.get("id").cloned();
    match (method, id) {
        (Some(method), Some(id)) => {
            let params = msg.get("params").cloned().unwrap_or(Value::Null);
            handle_server_request(inner, id, &method, params);
        }
        (Some(method), None) => {
            let params = msg.get("params").cloned().unwrap_or(Value::Null);
            handle_notification(inner, &method, params);
        }
        (None, Some(id)) => {
            let Some(id) = id.as_i64() else { return };
            let Some(tx) = inner.pending.lock().remove(&id) else { return };
            let reply = if let Some(err) = msg.get("error") {
                let code = err.get("code").and_then(Value::as_i64).unwrap_or(0);
                if code == REQUEST_CANCELLED || code == CONTENT_MODIFIED {
                    Err(LspError::Cancelled)
                } else {
                    Err(LspError::ServerError {
                        code,
                        message: err.get("message").and_then(Value::as_str).unwrap_or("").to_string(),
                        data: err.get("data").cloned(),
                    })
                }
            } else {
                Ok(msg.get("result").cloned().unwrap_or(Value::Null))
            };
            let _ = tx.send(reply);
        }
        (None, None) => {}
    }
}

fn handle_notification(inner: &Inner, method: &str, params: Value) {
    match method {
        "textDocument/publishDiagnostics" => {
            if let Ok(p) = serde_json::from_value::<PublishDiagnosticsParams>(params) {
                inner.handler.diagnostics(&inner.name, p);
            }
        }
        "window/logMessage" | "window/showMessage" => {
            let level = match params.get("type").and_then(Value::as_i64) {
                Some(1) => LogLevel::Error,
                Some(2) => LogLevel::Warning,
                Some(3) => LogLevel::Info,
                _ => LogLevel::Log,
            };
            if let Some(m) = params.get("message").and_then(Value::as_str) {
                inner.handler.log(&inner.name, level, m);
            }
        }
        _ => {}
    }
}

fn handle_server_request(inner: &Inner, id: Value, method: &str, params: Value) {
    let result: Result<Value, (i64, String)> = match method {
        "workspace/configuration" => {
            let n = params.get("items").and_then(Value::as_array).map_or(0, Vec::len);
            Ok(Value::Array(vec![Value::Null; n]))
        }
        "workspace/workspaceFolders" => Ok(inner.workspace_folders.clone()),
        "client/registerCapability"
        | "client/unregisterCapability"
        | "window/workDoneProgress/create"
        | "window/showMessageRequest"
        | "window/showDocument" => Ok(if method == "window/showDocument" {
            json!({ "success": false })
        } else {
            Value::Null
        }),
        "workspace/applyEdit" => Ok(json!({
            "applied": false,
            "failureReason": "workspace edits from the server are not supported",
        })),
        _ => Err((METHOD_NOT_FOUND, format!("method not supported: {method}"))),
    };
    let msg = match result {
        Ok(v) => json!({"jsonrpc": "2.0", "id": id, "result": v}),
        Err((code, message)) => {
            json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
        }
    };
    let _ = inner.send(&msg);
}

fn client_capabilities() -> Value {
    json!({
        "general": { "positionEncodings": ["utf-16"] },
        "workspace": {
            "workspaceFolders": true,
            "configuration": true,
            "applyEdit": false,
        },
        "textDocument": {
            "synchronization": { "didSave": true, "dynamicRegistration": false, "willSave": false },
            "publishDiagnostics": { "relatedInformation": true, "versionSupport": true },
            "completion": {
                "completionItem": {
                    "snippetSupport": false,
                    "documentationFormat": ["markdown", "plaintext"],
                    "deprecatedSupport": true,
                },
                "contextSupport": true,
            },
            "hover": { "contentFormat": ["markdown", "plaintext"] },
            "definition": { "linkSupport": false },
            "references": {},
            "rename": { "prepareSupport": false },
            "documentSymbol": { "hierarchicalDocumentSymbolSupport": true },
        },
        "window": { "workDoneProgress": true },
    })
}

/// Root uri plus the `workspaceFolders` JSON (null when there is no root).
fn workspace_folders(root: Option<&Path>) -> Result<(Option<Uri>, Value), LspError> {
    let Some(p) = root else { return Ok((None, Value::Null)) };
    let uri = path_to_uri(p)?;
    let folders = json!([{
        "uri": uri.as_str(),
        "name": p.file_name().and_then(|n| n.to_str()).unwrap_or("workspace"),
    }]);
    Ok((Some(uri), folders))
}
