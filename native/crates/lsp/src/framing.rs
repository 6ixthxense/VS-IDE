//! `Content-Length` framed JSON-RPC messages (the LSP base protocol).

use std::io::{BufRead, Read, Write};

use serde_json::Value;

use crate::error::LspError;

/// Refuse absurd bodies instead of allocating them.
const MAX_BODY: usize = 64 * 1024 * 1024;
/// Longest accepted header line and number of header lines.
const MAX_HEADER_LINE: u64 = 8 * 1024;
const MAX_HEADER_LINES: usize = 64;

/// Read one message. Returns `Ok(None)` on clean EOF before a header.
pub fn read_message<R: BufRead>(r: &mut R) -> Result<Option<Value>, LspError> {
    let mut content_length: Option<usize> = None;
    let mut saw_header = false;
    let mut line = Vec::with_capacity(64);
    let mut header_lines = 0usize;
    loop {
        line.clear();
        // Bounded: a peer that never sends a newline must not grow the buffer forever.
        let n = r.by_ref().take(MAX_HEADER_LINE + 1).read_until(b'\n', &mut line)?;
        if line.len() as u64 > MAX_HEADER_LINE {
            return Err(LspError::Protocol("header line too long".into()));
        }
        header_lines += 1;
        if header_lines > MAX_HEADER_LINES {
            return Err(LspError::Protocol("too many header lines".into()));
        }
        if n == 0 {
            return if saw_header {
                Err(LspError::Protocol("EOF inside message header".into()))
            } else {
                Ok(None)
            };
        }
        saw_header = true;
        let text = String::from_utf8_lossy(&line);
        let text = text.trim_end_matches(['\r', '\n']);
        if text.is_empty() {
            break;
        }
        if let Some((name, value)) = text.split_once(':') {
            if name.trim().eq_ignore_ascii_case("content-length") {
                content_length = Some(
                    value
                        .trim()
                        .parse()
                        .map_err(|_| LspError::Protocol(format!("bad Content-Length `{value}`")))?,
                );
            }
        }
    }
    let len = content_length.ok_or_else(|| LspError::Protocol("missing Content-Length".into()))?;
    if len > MAX_BODY {
        return Err(LspError::Protocol(format!("message too large ({len} bytes)")));
    }
    let mut body = vec![0u8; len];
    r.read_exact(&mut body)?;
    Ok(Some(serde_json::from_slice(&body)?))
}

/// Write one message (header + body in a single write) and flush.
pub fn write_message<W: Write>(w: &mut W, msg: &Value) -> Result<(), LspError> {
    let body = serde_json::to_vec(msg)?;
    let mut buf = Vec::with_capacity(body.len() + 32);
    buf.extend_from_slice(format!("Content-Length: {}\r\n\r\n", body.len()).as_bytes());
    buf.extend_from_slice(&body);
    w.write_all(&buf)?;
    w.flush()?;
    Ok(())
}


#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io::Cursor;

    #[test]
    fn roundtrip_and_multiple_messages() {
        let mut buf = Vec::new();
        write_message(&mut buf, &json!({"jsonrpc":"2.0","id":1,"method":"a"})).unwrap();
        write_message(&mut buf, &json!({"jsonrpc":"2.0","method":"é€𝄞"})).unwrap();
        let mut r = Cursor::new(buf);
        assert_eq!(read_message(&mut r).unwrap().unwrap()["id"], 1);
        assert_eq!(read_message(&mut r).unwrap().unwrap()["method"], "é€𝄞");
        assert!(read_message(&mut r).unwrap().is_none());
    }

    #[test]
    fn tolerates_extra_headers_and_case() {
        let body = br#"{"x":1}"#;
        let raw = format!(
            "content-type: application/vscode-jsonrpc; charset=utf-8\r\nCONTENT-LENGTH: {}\r\n\r\n{}",
            body.len(),
            std::str::from_utf8(body).unwrap()
        );
        let mut r = Cursor::new(raw.into_bytes());
        assert_eq!(read_message(&mut r).unwrap().unwrap()["x"], 1);
    }

    #[test]
    fn bounds_header_and_body() {
        let mut long = b"X-Pad: ".to_vec();
        long.extend(std::iter::repeat_n(b'a', 20_000));
        long.extend_from_slice(b"\r\nContent-Length: 2\r\n\r\n{}");
        assert!(matches!(read_message(&mut Cursor::new(long)), Err(LspError::Protocol(m)) if m.contains("too long")));
        // no newline at all and endless data
        let endless = vec![b'a'; 100_000];
        assert!(matches!(read_message(&mut Cursor::new(endless)), Err(LspError::Protocol(_))));
        let mut many = Vec::new();
        for _ in 0..100 {
            many.extend_from_slice(b"X: 1\r\n");
        }
        many.extend_from_slice(b"Content-Length: 2\r\n\r\n{}");
        assert!(matches!(read_message(&mut Cursor::new(many)), Err(LspError::Protocol(m)) if m.contains("too many")));
        let huge = format!("Content-Length: {}\r\n\r\n", 65 * 1024 * 1024);
        assert!(matches!(read_message(&mut Cursor::new(huge.into_bytes())), Err(LspError::Protocol(m)) if m.contains("too large")));
        // a normal header line right at the limit still works
        let mut ok = format!("X: {}\r\n", "a".repeat(8000)).into_bytes();
        ok.extend_from_slice(b"Content-Length: 2\r\n\r\n{}");
        assert!(read_message(&mut Cursor::new(ok)).unwrap().is_some());
    }

    #[test]
    fn errors() {
        let mut r = Cursor::new(b"Foo: bar\r\n\r\n{}".to_vec());
        assert!(matches!(read_message(&mut r), Err(LspError::Protocol(_))));
        let mut r = Cursor::new(b"Content-Length: 10\r\n\r\n{}".to_vec());
        assert!(read_message(&mut r).is_err());
        let mut r = Cursor::new(b"Content-Length: abc\r\n\r\n".to_vec());
        assert!(matches!(read_message(&mut r), Err(LspError::Protocol(_))));
        let mut r = Cursor::new(b"Content-Length: 3\r\n".to_vec());
        assert!(matches!(read_message(&mut r), Err(LspError::Protocol(_))));
    }
}
