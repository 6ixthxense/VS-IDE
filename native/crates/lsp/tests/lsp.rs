mod support {
    pub mod fake_server;
}

use std::io::pipe;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossbeam_channel::{unbounded, Receiver};
use lsp_types::notification::Notification;
use lsp_types::request::Request;
use lsp_types::{
    DocumentSymbolResponse, GotoDefinitionResponse, HoverContents, Position, PublishDiagnosticsParams,
    Range, TextDocumentContentChangeEvent, Uri,
};
use parking_lot::Mutex;
use serde_json::Value;
use support::fake_server::{self, FakeOptions};
use vsm_lsp::text::path_to_uri;
use vsm_lsp::{
    ConnectOptions, LanguageRegistry, LogLevel, LspClient, LspError, LspEventHandler, LspManager,
    ServerConfig, ServerStatus,
};

const T: Duration = Duration::from_secs(10);

// ---- custom test-only messages ----
enum Hang {}
impl Request for Hang {
    type Params = ();
    type Result = Value;
    const METHOD: &'static str = "test/hang";
}
enum State {}
impl Request for State {
    type Params = ();
    type Result = Value;
    const METHOD: &'static str = "test/state";
}
enum Crash {}
impl Notification for Crash {
    type Params = ();
    const METHOD: &'static str = "test/crash";
}

#[derive(Debug, PartialEq)]
enum Ev {
    Diag(PublishDiagnosticsParams),
    Status(ServerStatus),
}

struct Collect(Mutex<crossbeam_channel::Sender<Ev>>);
impl LspEventHandler for Collect {
    fn diagnostics(&self, _s: &str, p: PublishDiagnosticsParams) {
        let _ = self.0.lock().send(Ev::Diag(p));
    }
    fn status(&self, _s: &str, st: ServerStatus) {
        let _ = self.0.lock().send(Ev::Status(st));
    }
    fn log(&self, _s: &str, _l: LogLevel, _m: &str) {}
}

fn collector() -> (Arc<dyn LspEventHandler>, Receiver<Ev>) {
    let (tx, rx) = unbounded();
    (Arc::new(Collect(Mutex::new(tx))), rx)
}

fn next_diag(rx: &Receiver<Ev>) -> PublishDiagnosticsParams {
    loop {
        match rx.recv_timeout(T).expect("timed out waiting for diagnostics") {
            Ev::Diag(d) => return d,
            Ev::Status(_) => {}
        }
    }
}

fn wait_stopped(rx: &Receiver<Ev>) -> bool {
    let deadline = Instant::now() + T;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        if let Ev::Status(ServerStatus::Stopped { crashed }) =
            rx.recv_timeout(left).expect("timed out waiting for Stopped")
        {
            return crashed;
        }
    }
}

fn uri(s: &str) -> Uri {
    s.parse().unwrap()
}

/// Connect to the fake server running on a thread of this process over pipes.
fn in_process(
    opts: FakeOptions,
    handler: Arc<dyn LspEventHandler>,
) -> (LspClient, std::thread::JoinHandle<i32>) {
    let (srv_in_r, srv_in_w) = pipe().unwrap();
    let (srv_out_r, srv_out_w) = pipe().unwrap();
    let t = std::thread::spawn(move || fake_server::run(srv_in_r, srv_out_w, opts));
    let client = LspClient::connect(
        "fake",
        Box::new(srv_out_r),
        Box::new(srv_in_w),
        ConnectOptions { root: Some(std::env::temp_dir()), handler, ..Default::default() },
    )
    .unwrap();
    (client, t)
}

fn state(c: &LspClient) -> Value {
    c.request::<State>((), T).unwrap()
}

#[test]
#[allow(clippy::mutable_key_type)]
fn initialize_and_language_features() {
    let (h, _rx) = collector();
    let (c, server) = in_process(FakeOptions::default(), h);
    assert!(c.is_alive());
    assert!(c.capabilities().hover_provider.is_some());
    assert_eq!(c.sync_kind(), lsp_types::TextDocumentSyncKind::INCREMENTAL);

    let u = uri("file:///w/a.ts");
    c.did_open(u.clone(), "typescript", 1, "let x = 1\n".into()).unwrap();

    let hover = c.hover(u.clone(), Position::new(3, 4)).unwrap().unwrap();
    let HoverContents::Markup(m) = hover.contents else { panic!("expected markup") };
    assert_eq!(m.value, "hover 3:4 file:///w/a.ts");

    let Some(GotoDefinitionResponse::Scalar(loc)) = c.definition(u.clone(), Position::new(0, 0)).unwrap() else {
        panic!("expected scalar location")
    };
    assert_eq!(loc.uri, u);

    let refs = c.references(u.clone(), Position::new(0, 0), true).unwrap().unwrap();
    assert_eq!(refs.len(), 2);

    let edit = c.rename(u.clone(), Position::new(0, 4), "renamed").unwrap().unwrap();
    let changes = edit.changes.unwrap();
    assert_eq!(changes.values().next().unwrap()[0].new_text, "renamed");
    assert_eq!(changes.len(), 1);

    let Some(DocumentSymbolResponse::Nested(syms)) = c.document_symbols(u.clone()).unwrap() else {
        panic!("expected nested symbols")
    };
    assert_eq!(syms[0].name, "main");

    let comp = c.completion(u.clone(), Position::new(0, 0)).unwrap().unwrap();
    let lsp_types::CompletionResponse::List(list) = comp else { panic!("expected list") };
    assert_eq!(list.items.iter().map(|i| i.label.as_str()).collect::<Vec<_>>(), ["foo", "bar"]);

    // unknown request -> JSON-RPC error surfaced as ServerError
    enum Bogus {}
    impl Request for Bogus {
        type Params = ();
        type Result = Value;
        const METHOD: &'static str = "bogus/method";
    }
    match c.request::<Bogus>((), T) {
        Err(LspError::ServerError { code, .. }) => assert_eq!(code, -32601),
        other => panic!("{other:?}"),
    }

    c.shutdown(T).unwrap();
    assert!(!c.is_alive());
    assert_eq!(server.join().unwrap(), 0, "server should see shutdown then exit");
}

#[test]
fn server_requests_are_answered() {
    let (h, _rx) = collector();
    let (c, _s) = in_process(FakeOptions::default(), h);
    // The server asked for workspace/configuration (2 items) and registerCapability on `initialized`.
    // The replies are asynchronous relative to our own requests, so poll briefly.
    let deadline = Instant::now() + T;
    let st = loop {
        let st = state(&c);
        if !st["configReply"].is_null() && st.get("registerReply").is_some() {
            break st;
        }
        assert!(Instant::now() < deadline, "server never got a configuration reply");
        std::thread::sleep(Duration::from_millis(10));
    };
    assert_eq!(st["configReply"], serde_json::json!([null, null]));
    c.shutdown(T).unwrap();
}

#[test]
fn diagnostics_and_incremental_sync() {
    let (h, rx) = collector();
    let (c, _s) = in_process(FakeOptions::default(), h);
    let u = uri("file:///w/a.ts");

    c.did_open(u.clone(), "typescript", 1, "ok\nERROR here\n".into()).unwrap();
    let d = next_diag(&rx);
    assert_eq!(d.uri, u);
    assert_eq!(d.version, Some(1));
    assert_eq!(d.diagnostics.len(), 1);
    assert_eq!(d.diagnostics[0].range.start.line, 1);
    assert_eq!(d.diagnostics[0].message, "found ERROR");

    // Incremental: delete "ERROR" (line 1, cols 0..5) -> no more errors.
    let change = TextDocumentContentChangeEvent {
        range: Some(Range::new(Position::new(1, 0), Position::new(1, 5))),
        range_length: None,
        text: String::new(),
    };
    c.did_change(u.clone(), 2, vec![change], Some("ok\n here\n")).unwrap();
    let d = next_diag(&rx);
    assert_eq!(d.version, Some(2));
    assert!(d.diagnostics.is_empty());

    // Insert ERROR on line 0 incrementally.
    let change = TextDocumentContentChangeEvent {
        range: Some(Range::new(Position::new(0, 2), Position::new(0, 2))),
        range_length: None,
        text: " ERROR".into(),
    };
    c.did_change(u.clone(), 3, vec![change], None).unwrap();
    assert_eq!(next_diag(&rx).diagnostics.len(), 1);

    // Empty changes + full text -> falls back to a full replacement.
    c.did_change(u.clone(), 4, vec![], Some("clean\n")).unwrap();
    assert!(next_diag(&rx).diagnostics.is_empty());
    assert!(matches!(c.did_change(u.clone(), 5, vec![], None), Err(LspError::NeedsFullText)));

    c.did_save(u.clone(), Some("clean\n")).unwrap();
    c.did_close(u.clone()).unwrap();

    let st = state(&c);
    assert_eq!(st["saves"], 1);
    assert_eq!(st["openDocs"], 0);
    let changes = st["changes"].as_array().unwrap();
    assert_eq!(changes.len(), 3);
    assert_eq!(changes[0]["incremental"], true);
    assert_eq!(changes[2]["incremental"], false);
    c.shutdown(T).unwrap();
}

#[test]
fn full_sync_servers_get_full_text() {
    let (h, rx) = collector();
    let (c, _s) = in_process(FakeOptions { full_sync: true, ..Default::default() }, h);
    assert_eq!(c.sync_kind(), lsp_types::TextDocumentSyncKind::FULL);
    let u = uri("file:///w/b.py");
    c.did_open(u.clone(), "python", 1, "x\n".into()).unwrap();
    next_diag(&rx);
    let incremental = TextDocumentContentChangeEvent {
        range: Some(Range::new(Position::new(0, 0), Position::new(0, 1))),
        range_length: None,
        text: "ERROR".into(),
    };
    assert!(matches!(
        c.did_change(u.clone(), 2, vec![incremental.clone()], None),
        Err(LspError::NeedsFullText)
    ));
    c.did_change(u.clone(), 2, vec![incremental], Some("ERROR\n")).unwrap();
    assert_eq!(next_diag(&rx).diagnostics.len(), 1);
    let st = state(&c);
    assert_eq!(st["changes"][0]["incremental"], false);
    c.shutdown(T).unwrap();
}

#[test]
fn timeout_cancels_request() {
    let (h, _rx) = collector();
    let (c, _s) = in_process(FakeOptions::default(), h);
    let started = Instant::now();
    let err = c.request::<Hang>((), Duration::from_millis(150)).unwrap_err();
    assert!(matches!(err, LspError::Timeout { .. }), "{err:?}");
    assert!(started.elapsed() < Duration::from_secs(3));
    // The server must have received $/cancelRequest for it, and the client still works.
    let st = state(&c);
    assert_eq!(st["cancelled"].as_array().unwrap().len(), 1);
    c.shutdown(T).unwrap();
}

#[test]
fn dropping_or_cancelling_pending_request_sends_cancel() {
    let (h, _rx) = collector();
    let (c, _s) = in_process(FakeOptions::default(), h);
    let p1 = c.request_async::<Hang>(()).unwrap();
    let id1 = p1.id();
    drop(p1);
    let p2 = c.request_async::<Hang>(()).unwrap();
    let id2 = p2.id();
    p2.cancel();
    let st = state(&c);
    let cancelled: Vec<i64> = st["cancelled"].as_array().unwrap().iter().filter_map(Value::as_i64).collect();
    assert_eq!(cancelled, vec![id1, id2]);

    // A request that completes is not cancelled afterwards.
    let u = uri("file:///w/a.ts");
    let mut pending = c.completion_async(u, Position::new(0, 0)).unwrap();
    let deadline = Instant::now() + T;
    let got = loop {
        if let Some(r) = pending.try_recv() {
            break r;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    };
    assert!(got.unwrap().is_some());
    drop(pending);
    assert_eq!(state(&c)["cancelled"].as_array().unwrap().len(), 2);
    c.shutdown(T).unwrap();
}

#[test]
fn server_crash_fails_pending_and_future_requests() {
    let (h, rx) = collector();
    let (c, server) = in_process(FakeOptions::default(), h);
    let pending = c.request_async::<Hang>(()).unwrap();
    c.notify::<Crash>(()).unwrap();
    let started = Instant::now();
    assert!(matches!(pending.wait(T), Err(LspError::ServerExited)));
    assert!(started.elapsed() < Duration::from_secs(3));
    assert!(wait_stopped(&rx), "unrequested stop must be reported as a crash");
    assert!(!c.is_alive());
    assert!(matches!(c.hover(uri("file:///a.ts"), Position::new(0, 0)), Err(LspError::ServerExited)));
    assert_eq!(server.join().unwrap(), 99);
}

#[test]
fn requested_shutdown_is_not_a_crash() {
    let (h, rx) = collector();
    let (c, _s) = in_process(FakeOptions::default(), h);
    c.shutdown(T).unwrap();
    assert!(!wait_stopped(&rx));
}

// ---- subprocess tests (real stdio, real child process) ----

fn fake_server_exe() -> String {
    env!("CARGO_BIN_EXE_vsm-lsp-fake-server").to_string()
}

fn fake_config(id: &str, langs: &[&str]) -> ServerConfig {
    ServerConfig::new(id, &fake_server_exe(), &[], langs)
}

#[test]
fn spawned_subprocess_roundtrip() {
    let (h, rx) = collector();
    let cfg = fake_config("fake", &["typescript"]);
    let c = LspClient::spawn(&cfg, &std::env::temp_dir(), h, T).unwrap();
    let u = uri("file:///w/a.ts");
    c.did_open(u.clone(), "typescript", 1, "ERROR\n".into()).unwrap();
    assert_eq!(next_diag(&rx).diagnostics.len(), 1);
    assert!(c.hover(u, Position::new(0, 1)).unwrap().is_some());
    c.shutdown(T).unwrap();
    assert!(!wait_stopped(&rx));
}

#[test]
fn spawn_errors() {
    let (h, _rx) = collector();
    let cfg = ServerConfig::new("nope", "definitely-not-an-lsp-server-xyz", &[], &["x"]);
    let err = LspClient::spawn(&cfg, &std::env::temp_dir(), h.clone(), T).err().unwrap();
    assert!(matches!(err, LspError::ServerNotFound { .. }), "{err:?}");

    // A program that exits immediately never answers initialize -> ServerExited, not a hang.
    if cfg!(unix) {
        let cfg = ServerConfig::new("true", "/bin/sh", &["-c", "exit 0"], &["x"]);
        let started = Instant::now();
        let err = LspClient::spawn(&cfg, &std::env::temp_dir(), h, T).err().unwrap();
        assert!(matches!(err, LspError::ServerExited), "{err:?}");
        assert!(started.elapsed() < Duration::from_secs(5));
    }
}

fn manager(h: Arc<dyn LspEventHandler>) -> LspManager {
    let mut reg = LanguageRegistry::empty();
    reg.register(fake_config("fake-ts", &["typescript", "typescriptreact"]));
    reg.register(fake_config("fake-py", &["python"]));
    reg.register(ServerConfig::new("missing", "definitely-not-an-lsp-server-xyz", &[], &["cobol"]));
    reg.set_extension("ts", "typescript");
    reg.set_extension("py", "python");
    LspManager::new(reg, std::env::temp_dir(), h)
}

#[test]
fn manager_starts_lazily_and_stops_when_last_document_closes() {
    let (h, rx) = collector();
    let m = manager(h);
    assert!(m.running_servers().is_empty(), "nothing starts until a document is opened");

    let a = uri("file:///w/a.ts");
    let b = uri("file:///w/b.tsx");
    let p = uri("file:///w/c.py");

    m.open_document(a.clone(), "typescript", 1, "ERROR\n".into()).unwrap();
    assert_eq!(m.running_servers(), vec!["fake-ts"]);
    assert_eq!(next_diag(&rx).diagnostics.len(), 1);
    let ts_client = m.client("fake-ts").unwrap();

    // Same server serves tsx; python is a separate, lazily started server.
    m.open_document(b.clone(), "typescriptreact", 1, "x\n".into()).unwrap();
    next_diag(&rx);
    assert_eq!(m.running_servers(), vec!["fake-ts"]);
    m.open_document(p.clone(), "python", 1, "y\n".into()).unwrap();
    next_diag(&rx);
    assert_eq!(m.running_servers(), vec!["fake-py", "fake-ts"]);

    // Features route to the right server.
    assert!(m.hover(&a, Position::new(0, 0)).unwrap().is_some());
    assert!(m.definition(&p, Position::new(0, 0)).unwrap().is_some());
    assert_eq!(m.references(&a, Position::new(0, 0), false).unwrap().unwrap().len(), 2);
    assert!(m.rename(&a, Position::new(0, 0), "n").unwrap().is_some());
    assert!(m.document_symbols(&a).unwrap().is_some());
    assert!(m.completion(&a, Position::new(0, 0)).unwrap().is_some());
    m.change_document(&a, 2, vec![], Some("clean\n")).unwrap();
    assert!(next_diag(&rx).diagnostics.is_empty());
    m.save_document(&a, None).unwrap();

    // Closing one of two ts documents keeps the server; closing the last stops it.
    m.close_document(&a).unwrap();
    let cleared = next_diag(&rx);
    assert_eq!(cleared.uri, a);
    assert!(cleared.diagnostics.is_empty());
    assert_eq!(m.running_servers(), vec!["fake-py", "fake-ts"]);
    assert!(ts_client.is_alive());

    m.close_document(&b).unwrap();
    assert_eq!(m.running_servers(), vec!["fake-py"]);
    assert!(!ts_client.is_alive());

    assert!(matches!(m.hover(&b, Position::new(0, 0)), Err(LspError::DocumentNotOpen(_))));
    assert!(matches!(m.close_document(&b), Err(LspError::DocumentNotOpen(_))));

    // Re-opening restarts it.
    m.open_document(b.clone(), "typescriptreact", 1, "x\n".into()).unwrap();
    assert_eq!(m.running_servers(), vec!["fake-py", "fake-ts"]);

    m.shutdown_all();
    assert!(m.running_servers().is_empty());
}

#[test]
fn manager_errors_and_open_path() {
    let (h, _rx) = collector();
    let m = manager(h);
    assert!(matches!(
        m.open_document(uri("file:///a.go"), "go", 1, String::new()),
        Err(LspError::NoServerForLanguage(_))
    ));
    assert!(matches!(
        m.open_document(uri("file:///a.cob"), "cobol", 1, String::new()),
        Err(LspError::ServerNotFound { .. })
    ));
    assert!(m.running_servers().is_empty(), "failed start must not leave state behind");

    let path: PathBuf = std::env::temp_dir().join("vsm-lsp-test").join("x.ts");
    let u = m.open_path(&path, 1, "let a\n".into()).unwrap();
    assert_eq!(u, path_to_uri(&path).unwrap());
    assert!(m.is_open(&u));
    assert!(matches!(
        m.open_path(&path.with_extension("unknownext"), 1, String::new()),
        Err(LspError::NoServerForLanguage(_))
    ));
}

#[test]
fn manager_recovers_after_server_crash() {
    let (h, rx) = collector();
    let m = manager(h);
    let a = uri("file:///w/a.ts");
    m.open_document(a.clone(), "typescript", 1, "x\n".into()).unwrap();
    next_diag(&rx);
    m.client("fake-ts").unwrap().notify::<Crash>(()).unwrap();
    assert!(wait_stopped(&rx));
    assert!(matches!(m.hover(&a, Position::new(0, 0)), Err(e) if e.is_disconnected()));

    // Opening a document again replaces the dead server with a fresh one.
    m.open_document(a.clone(), "typescript", 1, "x\n".into()).unwrap();
    assert!(m.client("fake-ts").unwrap().is_alive());
    assert!(m.hover(&a, Position::new(0, 0)).unwrap().is_some());
}

// ---------------------------------------------------------------------------------------
// Regression tests for the review fixes
// ---------------------------------------------------------------------------------------

use std::io::BufReader;
use vsm_lsp::framing::{read_message, write_message};

#[test]
fn wants_save_is_false_for_supported_false() {
    let (h, _rx) = collector();
    let (c, _s) = in_process(FakeOptions { no_save: true, ..Default::default() }, h);
    let u = uri("file:///w/a.ts");
    c.did_open(u.clone(), "typescript", 1, "x\n".into()).unwrap();
    c.did_save(u, Some("x\n")).unwrap();
    assert_eq!(state(&c)["saves"], 0, "server said save:false, so no didSave may be sent");
    c.shutdown(T).unwrap();
}

#[test]
fn workspace_folders_request_gets_the_real_root() {
    let (h, _rx) = collector();
    let (c, _s) = in_process(FakeOptions::default(), h);
    let deadline = Instant::now() + T;
    let st = loop {
        let st = state(&c);
        if !st["foldersReply"].is_null() {
            break st;
        }
        assert!(Instant::now() < deadline, "no workspaceFolders reply");
        std::thread::sleep(Duration::from_millis(10));
    };
    let root = path_to_uri(&std::env::temp_dir()).unwrap();
    assert_eq!(st["foldersReply"][0]["uri"], root.as_str());
    assert!(st["foldersReply"][0]["name"].is_string());
    c.shutdown(T).unwrap();
}

#[test]
fn stalled_server_stdin_never_blocks_callers_or_the_reader_thread() {
    let (srv_in_r, srv_in_w) = pipe().unwrap();
    let (srv_out_r, mut srv_out_w) = pipe().unwrap();
    let (go_tx, go_rx) = crossbeam_channel::bounded::<()>(0);
    let (end_tx, end_rx) = crossbeam_channel::bounded::<()>(0);
    let server = std::thread::spawn(move || {
        let mut r = BufReader::new(srv_in_r);
        let init = read_message(&mut r).unwrap().unwrap();
        write_message(
            &mut srv_out_w,
            &serde_json::json!({"jsonrpc":"2.0","id":init["id"],"result":{"capabilities":{"textDocumentSync":1}}}),
        )
        .unwrap();
        let _initialized = read_message(&mut r).unwrap().unwrap();
        // From here on the server never reads stdin again; its pipe will fill up.
        go_rx.recv().unwrap();
        for i in 0..5 {
            write_message(
                &mut srv_out_w,
                &serde_json::json!({"jsonrpc":"2.0","id":100+i,"method":"workspace/configuration","params":{"items":[{}]}}),
            )
            .unwrap();
            write_message(
                &mut srv_out_w,
                &serde_json::json!({"jsonrpc":"2.0","method":"textDocument/publishDiagnostics",
                    "params":{"uri":"file:///w/a.ts","diagnostics":[]}}),
            )
            .unwrap();
        }
        let _ = end_rx.recv();
        drop(r);
    });
    let (h, rx) = collector();
    let c = LspClient::connect(
        "stall",
        Box::new(srv_out_r),
        Box::new(srv_in_w),
        ConnectOptions { handler: h, ..Default::default() },
    )
    .unwrap();

    // 40 MB of notifications into a pipe nobody drains: callers must not block.
    let t0 = Instant::now();
    let big = "a".repeat(1 << 20);
    for _ in 0..40 {
        c.did_open(uri("file:///w/a.ts"), "x", 1, big.clone()).unwrap();
    }
    assert!(t0.elapsed() < Duration::from_secs(5), "did_open blocked: {:?}", t0.elapsed());
    // Timeouts still fire on time while the pipe is stuck.
    let t1 = Instant::now();
    assert!(matches!(c.request::<Hang>((), Duration::from_millis(200)), Err(LspError::Timeout { .. })));
    assert!(t1.elapsed() < Duration::from_secs(2));

    // The reader thread must still deliver server messages although its replies cannot be written.
    go_tx.send(()).unwrap();
    for _ in 0..5 {
        assert!(rx.recv_timeout(T).is_ok(), "reader thread is stuck behind a blocked write");
    }
    end_tx.send(()).unwrap();
    server.join().unwrap();
    drop(c); // writer thread gets EPIPE and exits
}

#[test]
fn shutdown_closes_servers_stdin() {
    let (srv_in_r, srv_in_w) = pipe().unwrap();
    let (srv_out_r, mut srv_out_w) = pipe().unwrap();
    let server = std::thread::spawn(move || {
        let mut r = BufReader::new(srv_in_r);
        let init = read_message(&mut r).unwrap().unwrap();
        write_message(
            &mut srv_out_w,
            &serde_json::json!({"jsonrpc":"2.0","id":init["id"],"result":{"capabilities":{}}}),
        )
        .unwrap();
        let mut methods = Vec::new();
        // read until EOF: only possible if the client really closed its write end
        while let Some(m) = read_message(&mut r).unwrap() {
            let method = m["method"].as_str().unwrap_or("").to_string();
            if method == "shutdown" {
                write_message(&mut srv_out_w, &serde_json::json!({"jsonrpc":"2.0","id":m["id"],"result":null})).unwrap();
            }
            methods.push(method);
        }
        methods
    });
    let (h, _rx) = collector();
    let c = LspClient::connect(
        "eof",
        Box::new(srv_out_r),
        Box::new(srv_in_w),
        ConnectOptions { handler: h, ..Default::default() },
    )
    .unwrap();
    c.shutdown(T).unwrap();
    assert_eq!(server.join().unwrap(), ["initialized", "shutdown", "exit"]);
}

#[cfg(unix)]
fn pid_is_dead(pid: u32) -> bool {
    match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
        Err(_) => true,
        Ok(s) => s.rsplit(')').next().map(|r| r.trim_start().starts_with('Z')).unwrap_or(true),
    }
}

#[cfg(unix)]
fn wait_dead(pid: u32) -> bool {
    let end = Instant::now() + Duration::from_secs(5);
    while Instant::now() < end {
        if pid_is_dead(pid) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    pid_is_dead(pid)
}

#[cfg(unix)]
fn server_with_grandchild(tag: &str) -> (ServerConfig, PathBuf) {
    let pidfile = std::env::temp_dir().join(format!("vsm-lsp-gc-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_file(&pidfile);
    let script = format!("sleep 60 & echo $! > '{}'; exec '{}'", pidfile.display(), fake_server_exe());
    (ServerConfig::new("gc", "/bin/sh", &["-c", &script], &["typescript"]), pidfile)
}

#[cfg(unix)]
fn read_pid(pidfile: &std::path::Path) -> u32 {
    let end = Instant::now() + T;
    loop {
        if let Ok(s) = std::fs::read_to_string(pidfile) {
            if let Ok(p) = s.trim().parse() {
                return p;
            }
        }
        assert!(Instant::now() < end, "pidfile never appeared");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(unix)]
#[test]
fn shutdown_kills_the_whole_process_tree() {
    let (h, _rx) = collector();
    let (cfg, pidfile) = server_with_grandchild("shutdown");
    let c = LspClient::spawn(&cfg, &std::env::temp_dir(), h, T).unwrap();
    let gc = read_pid(&pidfile);
    assert!(!pid_is_dead(gc));
    c.shutdown(T).unwrap();
    assert!(wait_dead(gc), "grandchild {gc} survived shutdown");
    let _ = std::fs::remove_file(pidfile);
}

#[cfg(unix)]
#[test]
fn dropping_the_client_kills_the_whole_process_tree() {
    let (h, _rx) = collector();
    let (cfg, pidfile) = server_with_grandchild("drop");
    let c = LspClient::spawn(&cfg, &std::env::temp_dir(), h, T).unwrap();
    let gc = read_pid(&pidfile);
    drop(c);
    assert!(wait_dead(gc), "grandchild {gc} survived drop");
    let _ = std::fs::remove_file(pidfile);
}

#[cfg(unix)]
#[test]
fn unresponsive_server_is_force_killed_with_its_children() {
    // Server ignores everything (never answers shutdown) -> timeout path -> kill_tree.
    let (h, _rx) = collector();
    let pidfile = std::env::temp_dir().join(format!("vsm-lsp-gc-{}-hang", std::process::id()));
    let _ = std::fs::remove_file(&pidfile);
    // Handshake is done by the fake server; then `test/hang`-style: we simply use a tiny
    // shutdown timeout so `shutdown` request times out *if* the server is slow. Here the fake
    // server answers, so instead block it: SIGSTOP the group leader after the handshake.
    let script = format!("sleep 60 & echo $! > '{}'; exec '{}'", pidfile.display(), fake_server_exe());
    let cfg = ServerConfig::new("gc", "/bin/sh", &["-c", &script], &["typescript"]);
    let c = LspClient::spawn(&cfg, &std::env::temp_dir(), h, T).unwrap();
    let gc = read_pid(&pidfile);
    let status = std::process::Command::new("kill").args(["-STOP", &format!("-{}", pid_of_group_leader(gc))]).status().unwrap();
    assert!(status.success());
    let t0 = Instant::now();
    let _ = c.shutdown(Duration::from_millis(200));
    assert!(t0.elapsed() < Duration::from_secs(6));
    assert!(wait_dead(gc), "grandchild {gc} survived forced kill");
    let _ = std::fs::remove_file(pidfile);
}

/// The server leads its own process group; its pgid is the parent pid of the grandchild.
#[cfg(unix)]
fn pid_of_group_leader(grandchild: u32) -> u32 {
    let stat = std::fs::read_to_string(format!("/proc/{grandchild}/stat")).unwrap();
    let after = stat.rsplit(')').next().unwrap().trim_start();
    after.split_whitespace().nth(1).unwrap().parse().unwrap() // field 4: ppid
}

// ---- manager ----

#[test]
fn concurrent_opens_of_one_document_are_counted_once() {
    let (h, _rx) = collector();
    let m = Arc::new(manager(h));
    let a = uri("file:///w/race.ts");
    let threads: Vec<_> = (0..8)
        .map(|_| {
            let m = m.clone();
            let a = a.clone();
            std::thread::spawn(move || m.open_document(a, "typescript", 1, "x\n".into()))
        })
        .collect();
    for t in threads {
        t.join().unwrap().unwrap();
    }
    assert_eq!(m.running_servers(), vec!["fake-ts"]);
    // If open_docs had been bumped more than once, one close would leave the server running.
    m.close_document(&a).unwrap();
    assert!(m.running_servers().is_empty(), "double counted open_docs");
}

#[test]
fn concurrent_open_close_across_documents_stays_consistent() {
    let (h, _rx) = collector();
    let m = Arc::new(manager(h));
    let workers: Vec<_> = (0..4)
        .map(|n| {
            let m = m.clone();
            std::thread::spawn(move || {
                let u = uri(&format!("file:///w/stress{n}.ts"));
                for i in 0..15 {
                    m.open_document(u.clone(), "typescript", i, "x\n".into()).unwrap();
                    m.close_document(&u).unwrap();
                }
            })
        })
        .collect();
    for w in workers {
        w.join().unwrap();
    }
    assert!(m.running_servers().is_empty(), "leaked server: {:?}", m.running_servers());
}

struct Reentrant {
    mgr: std::sync::OnceLock<std::sync::Weak<LspManager>>,
    done: crossbeam_channel::Sender<Vec<String>>,
}
impl LspEventHandler for Reentrant {
    fn status(&self, _s: &str, st: ServerStatus) {
        if st == ServerStatus::Started {
            if let Some(m) = self.mgr.get().and_then(|w| w.upgrade()) {
                // Re-enter: same server id (needs the start lock) and the state lock.
                m.open_document(uri("file:///w/second.ts"), "typescript", 1, "y\n".into()).unwrap();
                let _ = self.done.send(m.running_servers());
            }
        }
    }
}

#[test]
fn started_handler_may_reenter_the_manager() {
    let (done_tx, done_rx) = crossbeam_channel::unbounded();
    let handler = Arc::new(Reentrant { mgr: Default::default(), done: done_tx });
    let mut reg = LanguageRegistry::empty();
    reg.register(fake_config("fake-ts", &["typescript"]));
    let m = Arc::new(LspManager::new(reg, std::env::temp_dir(), handler.clone()));
    handler.mgr.set(Arc::downgrade(&m)).unwrap();
    let m2 = m.clone();
    let t = std::thread::spawn(move || m2.open_document(uri("file:///w/first.ts"), "typescript", 1, "x\n".into()));
    let running = done_rx.recv_timeout(T).expect("deadlock: Started handler could not re-enter the manager");
    assert_eq!(running, vec!["fake-ts"]);
    t.join().unwrap().unwrap();
    assert!(m.is_open(&uri("file:///w/first.ts")) && m.is_open(&uri("file:///w/second.ts")));
}

#[cfg(unix)]
#[test]
fn slow_server_start_does_not_block_other_servers() {
    let (h, _rx) = collector();
    let mut reg = LanguageRegistry::empty();
    let script = format!("sleep 1.5; exec '{}'", fake_server_exe());
    reg.register(ServerConfig::new("slow", "/bin/sh", &["-c", &script], &["python"]));
    reg.register(fake_config("fast", &["typescript"]));
    let m = Arc::new(LspManager::new(reg, std::env::temp_dir(), h));
    let m2 = m.clone();
    let slow = std::thread::spawn(move || m2.open_document(uri("file:///w/s.py"), "python", 1, "x\n".into()));
    std::thread::sleep(Duration::from_millis(200));
    let t0 = Instant::now();
    m.open_document(uri("file:///w/f.ts"), "typescript", 1, "x\n".into()).unwrap();
    assert!(t0.elapsed() < Duration::from_millis(1200), "fast server waited for slow one: {:?}", t0.elapsed());
    slow.join().unwrap().unwrap();
    assert_eq!(m.running_servers(), vec!["fast", "slow"]);
}

#[test]
fn diagnostics_for_closed_documents_are_dropped_and_crash_clears_stale_ones() {
    enum Publish {}
    impl Request for Publish {
        type Params = Value;
        type Result = Value;
        const METHOD: &'static str = "test/publish";
    }
    let (h, rx) = collector();
    let m = manager(h);
    let a = uri("file:///w/a.ts");
    let b = uri("file:///w/b.ts");
    m.open_document(a.clone(), "typescript", 1, "x\n".into()).unwrap();
    m.open_document(b.clone(), "typescript", 1, "x\n".into()).unwrap();
    while rx.recv_timeout(Duration::from_millis(300)).is_ok() {} // drain open diagnostics

    let client = m.client("fake-ts").unwrap();
    let ghost = "file:///w/never-opened.ts";
    client.request::<Publish>(serde_json::json!({"uri": ghost}), T).unwrap();
    client.request::<Publish>(serde_json::json!({"uri": a.as_str()}), T).unwrap();
    // Only the open document's diagnostics get through, and the ghost one is filtered out.
    let d = next_diag(&rx);
    assert_eq!(d.uri, a);
    assert!(rx.recv_timeout(Duration::from_millis(300)).is_err(), "unexpected extra event");

    // Crash, then opening another document reaps the dead server and clears a's and b's problems.
    client.notify::<Crash>(()).unwrap();
    assert!(wait_stopped(&rx));
    m.open_document(uri("file:///w/c.ts"), "typescript", 1, "x\n".into()).unwrap();
    let mut cleared = Vec::new();
    while let Ok(ev) = rx.recv_timeout(Duration::from_millis(500)) {
        if let Ev::Diag(d) = ev {
            if d.diagnostics.is_empty() {
                cleared.push(d.uri);
            }
        }
    }
    assert!(cleared.contains(&a) && cleared.contains(&b), "stale diagnostics not cleared: {cleared:?}");
}
