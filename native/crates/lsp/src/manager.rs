//! Lazy per-language server lifecycle and document tracking.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use lsp_types::{
    CompletionResponse, DocumentSymbolResponse, GotoDefinitionResponse, Hover, Location, Position,
    PublishDiagnosticsParams, TextDocumentContentChangeEvent, Uri, WorkspaceEdit,
};
use parking_lot::{Mutex, RwLock};

use crate::client::{LspClient, DEFAULT_INIT_TIMEOUT};
use crate::error::LspError;
use crate::handler::{LspEventHandler, NoopHandler, ServerStatus};
use crate::registry::{LanguageRegistry, ServerConfig};
use crate::text::path_to_uri;

#[derive(Debug, Clone)]
pub struct ManagerOptions {
    pub init_timeout: Duration,
    pub request_timeout: Duration,
    pub shutdown_timeout: Duration,
}

impl Default for ManagerOptions {
    fn default() -> Self {
        ManagerOptions {
            init_timeout: DEFAULT_INIT_TIMEOUT,
            request_timeout: Duration::from_secs(10),
            shutdown_timeout: Duration::from_secs(3),
        }
    }
}

struct ServerEntry {
    client: Arc<LspClient>,
    open_docs: usize,
}

struct DocEntry {
    server_id: String,
}

#[derive(Default)]
struct State {
    servers: HashMap<String, ServerEntry>,
    docs: HashMap<Uri, DocEntry>,
}

/// Handler given to every client: forwards to the user's handler but drops diagnostics for
/// documents that are not (or no longer) open, so stale/late `publishDiagnostics` cannot
/// resurrect problems for a closed file.
struct Forward {
    inner: Arc<dyn LspEventHandler>,
    state: Arc<Mutex<State>>,
}

impl LspEventHandler for Forward {
    fn diagnostics(&self, server: &str, params: PublishDiagnosticsParams) {
        let open = self.state.lock().docs.contains_key(&params.uri);
        if open {
            self.inner.diagnostics(server, params);
        }
    }
    fn log(&self, server: &str, level: crate::handler::LogLevel, message: &str) {
        self.inner.log(server, level, message);
    }
    fn status(&self, server: &str, status: crate::handler::ServerStatus) {
        self.inner.status(server, status);
    }
}

/// Starts language servers on first use and stops each one when its last document is closed.
///
/// All methods block (server start-up can take seconds); call them from a background thread.
///
/// Event handler threading: `diagnostics`, `log` and `Stopped` run on a client's reader thread
/// (`log` with `Stderr` on its stderr thread); `Started` and the diagnostics-clearing calls made
/// by `close_document` / dead-server cleanup run on the thread that called the manager, with no
/// manager lock held. Handlers may therefore call back into the manager from `Started`, but
/// should hand work to another thread from reader-thread callbacks.
pub struct LspManager {
    registry: RwLock<LanguageRegistry>,
    root: RwLock<PathBuf>,
    handler: Arc<dyn LspEventHandler>,
    /// What clients get: `handler` behind the open-document filter.
    client_handler: Arc<dyn LspEventHandler>,
    options: ManagerOptions,
    state: Arc<Mutex<State>>,
    /// One start lock per server id: starting `typescript` never blocks `python`.
    start_locks: Mutex<HashMap<String, Arc<Mutex<()>>>>,
    /// One lock per document URI: open/close of the same document are strictly serialised.
    uri_locks: Mutex<HashMap<String, Arc<Mutex<()>>>>,
}

impl LspManager {
    pub fn new(registry: LanguageRegistry, root: impl Into<PathBuf>, handler: Arc<dyn LspEventHandler>) -> Self {
        Self::with_options(registry, root, handler, ManagerOptions::default())
    }

    pub fn with_options(
        registry: LanguageRegistry,
        root: impl Into<PathBuf>,
        handler: Arc<dyn LspEventHandler>,
        options: ManagerOptions,
    ) -> Self {
        let state = Arc::new(Mutex::new(State::default()));
        LspManager {
            registry: RwLock::new(registry),
            root: RwLock::new(root.into()),
            client_handler: Arc::new(Forward { inner: handler.clone(), state: state.clone() }),
            handler,
            options,
            state,
            start_locks: Mutex::new(HashMap::new()),
            uri_locks: Mutex::new(HashMap::new()),
        }
    }

    /// Manager with default registry and no event handler.
    pub fn with_defaults(root: impl Into<PathBuf>) -> Self {
        Self::new(LanguageRegistry::default(), root, Arc::new(NoopHandler))
    }

    /// Edit the registry (takes effect for servers started afterwards).
    pub fn registry_mut(&self) -> parking_lot::RwLockWriteGuard<'_, LanguageRegistry> {
        self.registry.write()
    }

    pub fn registry(&self) -> parking_lot::RwLockReadGuard<'_, LanguageRegistry> {
        self.registry.read()
    }

    /// Change the workspace root used for servers started afterwards.
    pub fn set_root(&self, root: impl Into<PathBuf>) {
        *self.root.write() = root.into();
    }

    pub fn language_id_for_path(&self, path: &Path) -> Option<String> {
        self.registry.read().language_id_for_path(path).map(str::to_string)
    }

    /// Ids of servers currently running.
    pub fn running_servers(&self) -> Vec<String> {
        let mut v: Vec<String> = self.state.lock().servers.keys().cloned().collect();
        v.sort();
        v
    }

    pub fn client(&self, server_id: &str) -> Option<Arc<LspClient>> {
        self.state.lock().servers.get(server_id).map(|e| e.client.clone())
    }

    pub fn is_open(&self, uri: &Uri) -> bool {
        self.state.lock().docs.contains_key(uri)
    }

    fn uri_lock(&self, uri: &Uri) -> Arc<Mutex<()>> {
        let mut map = self.uri_locks.lock();
        if map.len() > 64 {
            // Drop locks nobody holds or waits on (strong count 1 == only the map).
            map.retain(|_, l| Arc::strong_count(l) > 1);
        }
        map.entry(uri.as_str().to_string()).or_default().clone()
    }

    fn start_lock(&self, server_id: &str) -> Arc<Mutex<()>> {
        self.start_locks.lock().entry(server_id.to_string()).or_default().clone()
    }

    /// Drop servers whose process died, together with their document bookkeeping, so the next
    /// `open_document` starts a fresh server. Diagnostics of the dropped documents are cleared
    /// through the handler (after all locks are released). The UI is told via
    /// [`ServerStatus::Stopped`](crate::ServerStatus) and should re-open its documents.
    fn reap_dead(&self) {
        let mut cleared: Vec<(String, Uri)> = Vec::new();
        {
            let mut st = self.state.lock();
            let dead: Vec<String> =
                st.servers.iter().filter(|(_, e)| !e.client.is_alive()).map(|(k, _)| k.clone()).collect();
            for id in dead {
                st.servers.remove(&id);
                let gone: Vec<Uri> =
                    st.docs.iter().filter(|(_, d)| d.server_id == id).map(|(u, _)| u.clone()).collect();
                for u in gone {
                    st.docs.remove(&u);
                    cleared.push((id.clone(), u));
                }
            }
        }
        for (server, uri) in cleared {
            self.handler.diagnostics(&server, PublishDiagnosticsParams { uri, diagnostics: Vec::new(), version: None });
        }
    }

    fn ensure_server(&self, cfg: &ServerConfig) -> Result<Arc<LspClient>, LspError> {
        let alive = |st: &State| st.servers.get(&cfg.id).filter(|e| e.client.is_alive()).map(|e| e.client.clone());
        if let Some(c) = alive(&self.state.lock()) {
            return Ok(c);
        }
        let lock = self.start_lock(&cfg.id);
        let guard = lock.lock();
        if let Some(c) = alive(&self.state.lock()) {
            return Ok(c);
        }
        let root = self.root.read().clone();
        let client = LspClient::spawn(cfg, &root, self.client_handler.clone(), self.options.init_timeout)?;
        client.set_request_timeout(self.options.request_timeout);
        let client = Arc::new(client);
        self.state
            .lock()
            .servers
            .insert(cfg.id.clone(), ServerEntry { client: client.clone(), open_docs: 0 });
        // Announce only after the entry exists and every lock is released, so a handler that
        // calls back into the manager cannot deadlock.
        drop(guard);
        self.handler.status(&cfg.id, ServerStatus::Started);
        Ok(client)
    }

    /// Remove a document's bookkeeping. Returns the document's server id, its client, and the
    /// client again if it just lost its last document (caller must shut it down).
    #[allow(clippy::type_complexity)]
    fn release_doc(&self, uri: &Uri) -> Option<(String, Option<Arc<LspClient>>, Option<Arc<LspClient>>)> {
        let mut st = self.state.lock();
        let doc = st.docs.remove(uri)?;
        let mut stop = None;
        let mut client = None;
        if let Some(e) = st.servers.get_mut(&doc.server_id) {
            e.open_docs = e.open_docs.saturating_sub(1);
            client = Some(e.client.clone());
            if e.open_docs == 0 {
                stop = st.servers.remove(&doc.server_id).map(|e| e.client);
            }
        }
        Some((doc.server_id, client, stop))
    }

    /// Open (or re-sync) a document, starting the language's server if needed.
    pub fn open_document(&self, uri: Uri, language_id: &str, version: i32, text: String) -> Result<(), LspError> {
        let lock = self.uri_lock(&uri);
        let _held = lock.lock();
        let cfg = self
            .registry
            .read()
            .for_language(language_id)
            .cloned()
            .ok_or_else(|| LspError::NoServerForLanguage(language_id.to_string()))?;

        for _ in 0..3 {
            self.reap_dead();
            if self.state.lock().docs.contains_key(&uri) {
                // Already open: treat as a full-content update.
                return self.change_document(&uri, version, Vec::new(), Some(&text));
            }
            let client = self.ensure_server(&cfg)?;
            // Reserve the document slot *before* telling the server: the entry then cannot be
            // stopped by a concurrent close of another document, and a concurrent open of the
            // same URI is excluded by the per-URI lock, so `open_docs` is never double counted.
            let reserved = {
                let mut st = self.state.lock();
                let ok = st
                    .servers
                    .get_mut(&cfg.id)
                    .filter(|e| Arc::ptr_eq(&e.client, &client) && client.is_alive())
                    .map(|e| e.open_docs += 1)
                    .is_some();
                if ok {
                    st.docs.insert(uri.clone(), DocEntry { server_id: cfg.id.clone() });
                }
                ok
            };
            if !reserved {
                continue; // server was stopped/crashed between lookup and reservation: retry
            }
            return match client.did_open(uri.clone(), language_id, version, text) {
                Ok(()) => Ok(()),
                Err(e) => {
                    // Roll the reservation back (and stop the server if it was the only user).
                    if let Some((_, _, Some(stop))) = self.release_doc(&uri) {
                        let _ = stop.shutdown(self.options.shutdown_timeout);
                    }
                    Err(e)
                }
            };
        }
        Err(LspError::ServerExited)
    }

    /// Convenience: open by path, deriving the language id from the extension.
    pub fn open_path(&self, path: &Path, version: i32, text: String) -> Result<Uri, LspError> {
        let lang = self
            .language_id_for_path(path)
            .ok_or_else(|| LspError::NoServerForLanguage(path.display().to_string()))?;
        let uri = path_to_uri(path)?;
        self.open_document(uri.clone(), &lang, version, text)?;
        Ok(uri)
    }

    fn client_for(&self, uri: &Uri) -> Result<Arc<LspClient>, LspError> {
        let st = self.state.lock();
        let doc = st.docs.get(uri).ok_or_else(|| LspError::DocumentNotOpen(uri.as_str().to_string()))?;
        st.servers
            .get(&doc.server_id)
            .map(|e| e.client.clone())
            .ok_or(LspError::ServerExited)
    }

    /// Send a change. See [`LspClient::did_change`] for how `changes`/`full_text` are used.
    pub fn change_document(
        &self,
        uri: &Uri,
        version: i32,
        changes: Vec<TextDocumentContentChangeEvent>,
        full_text: Option<&str>,
    ) -> Result<(), LspError> {
        self.client_for(uri)?.did_change(uri.clone(), version, changes, full_text)
    }

    pub fn save_document(&self, uri: &Uri, text: Option<&str>) -> Result<(), LspError> {
        self.client_for(uri)?.did_save(uri.clone(), text)
    }

    /// Close a document. When it was the last one for its server, the server is shut down.
    /// Diagnostics for the document are cleared through the event handler.
    pub fn close_document(&self, uri: &Uri) -> Result<(), LspError> {
        let lock = self.uri_lock(uri);
        let _held = lock.lock();
        let Some((server_id, client, stop)) = self.release_doc(uri) else {
            return Err(LspError::DocumentNotOpen(uri.as_str().to_string()));
        };
        let result = match &client {
            Some(c) => c.did_close(uri.clone()),
            None => Ok(()),
        };
        self.handler.diagnostics(
            &server_id,
            PublishDiagnosticsParams { uri: uri.clone(), diagnostics: Vec::new(), version: None },
        );
        if let Some(c) = stop {
            let _ = c.shutdown(self.options.shutdown_timeout);
        }
        match result {
            Err(e) if e.is_disconnected() => Ok(()),
            other => other,
        }
    }

    pub fn hover(&self, uri: &Uri, pos: Position) -> Result<Option<Hover>, LspError> {
        self.client_for(uri)?.hover(uri.clone(), pos)
    }

    pub fn completion(&self, uri: &Uri, pos: Position) -> Result<Option<CompletionResponse>, LspError> {
        self.client_for(uri)?.completion(uri.clone(), pos)
    }

    pub fn definition(&self, uri: &Uri, pos: Position) -> Result<Option<GotoDefinitionResponse>, LspError> {
        self.client_for(uri)?.definition(uri.clone(), pos)
    }

    pub fn references(
        &self,
        uri: &Uri,
        pos: Position,
        include_declaration: bool,
    ) -> Result<Option<Vec<Location>>, LspError> {
        self.client_for(uri)?.references(uri.clone(), pos, include_declaration)
    }

    pub fn rename(&self, uri: &Uri, pos: Position, new_name: &str) -> Result<Option<WorkspaceEdit>, LspError> {
        self.client_for(uri)?.rename(uri.clone(), pos, new_name)
    }

    pub fn document_symbols(&self, uri: &Uri) -> Result<Option<DocumentSymbolResponse>, LspError> {
        self.client_for(uri)?.document_symbols(uri.clone())
    }

    /// Shut every server down (call on workspace change / app exit).
    pub fn shutdown_all(&self) {
        let clients: Vec<Arc<LspClient>> = {
            let mut st = self.state.lock();
            st.docs.clear();
            st.servers.drain().map(|(_, e)| e.client).collect()
        };
        for c in clients {
            let _ = c.shutdown(self.options.shutdown_timeout);
        }
    }
}

impl Drop for LspManager {
    fn drop(&mut self) {
        self.shutdown_all();
    }
}
