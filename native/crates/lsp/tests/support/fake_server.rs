//! A minimal in-test LSP server. Answers initialize/hover/definition/references/rename/
//! documentSymbol/completion, emits diagnostics for lines containing `ERROR`, and has a few
//! `test/*` extensions for observing client behaviour.

use std::collections::HashMap;
use std::io::{BufReader, Read, Write};

use serde_json::{json, Value};
use vsm_lsp::framing::{read_message, write_message};
use vsm_lsp::lsp_types::Position;
use vsm_lsp::text::offset_at;

#[derive(Clone, Copy, Default)]
pub struct FakeOptions {
    pub full_sync: bool,
    /// Advertise `save: false` instead of save options.
    pub no_save: bool,
}

#[derive(Default)]
struct State {
    docs: HashMap<String, String>,
    opens: Vec<String>,
    changes: Vec<Value>,
    saves: usize,
    cancelled: Vec<i64>,
    config_reply: Value,
    register_reply: Value,
    folders_reply: Value,
    shutdown: bool,
}

fn diagnostics(uri: &str, text: &str, version: Option<i64>) -> Value {
    let diags: Vec<Value> = text
        .lines()
        .enumerate()
        .filter(|(_, l)| l.contains("ERROR"))
        .map(|(i, l)| {
            json!({
                "range": {"start": {"line": i, "character": 0},
                          "end": {"line": i, "character": l.encode_utf16().count()}},
                "severity": 1, "source": "fake", "message": "found ERROR",
            })
        })
        .collect();
    json!({"jsonrpc":"2.0","method":"textDocument/publishDiagnostics",
           "params": {"uri": uri, "version": version, "diagnostics": diags}})
}

fn range0() -> Value {
    json!({"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 3}})
}

/// Run the server until `exit`/EOF/crash. Returns the process exit code.
pub fn run<R: Read, W: Write>(input: R, mut out: W, opts: FakeOptions) -> i32 {
    let mut r = BufReader::new(input);
    let mut st = State::default();
    let send = |v: Value, out: &mut W| {
        let _ = write_message(out, &v);
    };
    loop {
        let msg = match read_message(&mut r) {
            Ok(Some(m)) => m,
            _ => return 2,
        };
        let method = msg.get("method").and_then(Value::as_str).unwrap_or("").to_string();
        let id = msg.get("id").cloned();
        let params = msg.get("params").cloned().unwrap_or(Value::Null);
        let reply = |result: Value| json!({"jsonrpc":"2.0","id": id.clone(), "result": result});

        // Responses to our own server->client requests.
        if method.is_empty() {
            match id.as_ref().and_then(Value::as_i64) {
                Some(9001) => st.config_reply = msg.get("result").cloned().unwrap_or(Value::Null),
                Some(9002) => st.register_reply = msg.get("result").cloned().unwrap_or(Value::Null),
                Some(9003) => st.folders_reply = msg.get("result").cloned().unwrap_or(Value::Null),
                _ => {}
            }
            continue;
        }

        let uri = params["textDocument"]["uri"].as_str().unwrap_or("").to_string();
        match method.as_str() {
            "initialize" => {
                let sync = if opts.full_sync { 1 } else { 2 };
                send(
                    reply(json!({
                        "capabilities": {
                            "textDocumentSync": {"openClose": true, "change": sync, "save": if opts.no_save { json!(false) } else { json!({"includeText": false}) }},
                            "hoverProvider": true, "definitionProvider": true,
                            "referencesProvider": true, "renameProvider": true,
                            "documentSymbolProvider": true,
                            "completionProvider": {"triggerCharacters": ["."]},
                        },
                        "serverInfo": {"name": "fake"},
                    })),
                    &mut out,
                );
            }
            "initialized" => {
                send(json!({"jsonrpc":"2.0","id":9001,"method":"workspace/configuration",
                            "params":{"items":[{"section":"a"},{"section":"b"}]}}), &mut out);
                send(json!({"jsonrpc":"2.0","id":9002,"method":"client/registerCapability",
                            "params":{"registrations":[]}}), &mut out);
                send(json!({"jsonrpc":"2.0","id":9003,"method":"workspace/workspaceFolders"}), &mut out);
            }
            "textDocument/didOpen" => {
                let item = &params["textDocument"];
                let text = item["text"].as_str().unwrap_or("").to_string();
                let u = item["uri"].as_str().unwrap_or("").to_string();
                send(diagnostics(&u, &text, item["version"].as_i64()), &mut out);
                st.opens.push(format!("{}:{}", u, item["languageId"].as_str().unwrap_or("")));
                st.docs.insert(u, text);
            }
            "textDocument/didChange" => {
                let changes = params["contentChanges"].as_array().cloned().unwrap_or_default();
                let text = st.docs.entry(uri.clone()).or_default();
                for c in &changes {
                    let new = c["text"].as_str().unwrap_or("");
                    if c.get("range").is_some_and(|r| !r.is_null()) {
                        let pos = |p: &Value| Position::new(p["line"].as_u64().unwrap_or(0) as u32, p["character"].as_u64().unwrap_or(0) as u32);
                        let s = offset_at(text, pos(&c["range"]["start"]));
                        let e = offset_at(text, pos(&c["range"]["end"]));
                        text.replace_range(s..e, new);
                    } else {
                        *text = new.to_string();
                    }
                }
                st.changes.push(json!({
                    "version": params["textDocument"]["version"],
                    "incremental": changes.iter().any(|c| c.get("range").is_some_and(|r| !r.is_null())),
                    "count": changes.len(),
                }));
                let text = st.docs[&uri].clone();
                send(diagnostics(&uri, &text, params["textDocument"]["version"].as_i64()), &mut out);
            }
            "textDocument/didSave" => st.saves += 1,
            "textDocument/didClose" => {
                st.docs.remove(&uri);
            }
            "textDocument/hover" => {
                let p = &params["position"];
                send(
                    reply(json!({"contents": {"kind": "markdown",
                        "value": format!("hover {}:{} {}", p["line"], p["character"], uri)}})),
                    &mut out,
                );
            }
            "textDocument/definition" => {
                send(reply(json!({"uri": uri, "range": range0()})), &mut out);
            }
            "textDocument/references" => {
                send(
                    reply(json!([{"uri": uri, "range": range0()},
                                 {"uri": "file:///other.ts", "range": range0()}])),
                    &mut out,
                );
            }
            "textDocument/rename" => {
                let name = params["newName"].as_str().unwrap_or("");
                send(
                    reply(json!({"changes": {uri.clone(): [{"range": range0(), "newText": name}]}})),
                    &mut out,
                );
            }
            "textDocument/documentSymbol" => {
                send(
                    reply(json!([{"name": "main", "kind": 12, "range": range0(), "selectionRange": range0()}])),
                    &mut out,
                );
            }
            "textDocument/completion" => {
                send(
                    reply(json!({"isIncomplete": false, "items": [{"label": "foo"}, {"label": "bar"}]})),
                    &mut out,
                );
            }
            "test/hang" => {} // never answered
            "test/state" => {
                send(
                    reply(json!({
                        "opens": st.opens, "changes": st.changes, "saves": st.saves,
                        "cancelled": st.cancelled, "configReply": st.config_reply,
                        "registerReply": st.register_reply,
                        "foldersReply": st.folders_reply,
                        "openDocs": st.docs.len(),
                    })),
                    &mut out,
                );
            }
            "test/crash" => return 99,
            "test/publish" => {
                let u = params["uri"].as_str().unwrap_or("");
                send(diagnostics(u, "ERROR", None), &mut out);
                send(reply(Value::Null), &mut out);
            }
            "$/cancelRequest" => {
                if let Some(i) = params["id"].as_i64() {
                    st.cancelled.push(i);
                }
            }
            "shutdown" => {
                st.shutdown = true;
                send(reply(Value::Null), &mut out);
            }
            "exit" => return if st.shutdown { 0 } else { 1 },
            m if id.is_some() && !m.starts_with("$/") => {
                send(
                    json!({"jsonrpc":"2.0","id": id.clone(),
                           "error": {"code": -32601, "message": format!("unknown method {m}")}}),
                    &mut out,
                );
            }
            _ => {}
        }
    }
}
