//! Authenticated loopback JSON-lines control server: one request per line, one reply per line.
//! This is the transport the MCP server (`designcraft-cli mcp --connect`) wraps.
//!
//! The server only ever reads requests: a line that is not a JSON object with a string `method`
//! (an HTTP request line, stray text, or binary data) gets one error reply and the connection is
//! closed, so nothing after it is executed. Lines, requests, idle time, and concurrent connections
//! are bounded; unauthenticated input never reaches the UI thread. See `docs/control-protocol.md`.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

use designcraft_ui_egui::ControlRequest;
use serde_json::{Value, json};

/// Longest accepted request line (bytes, without the newline).
pub const MAX_LINE: usize = 4 * 1024 * 1024;
/// Connections served at once; further ones get an error line and are closed.
pub const MAX_CONNECTIONS: usize = 16;
const MAX_REQUESTS_PER_CONNECTION: usize = 4_096;
const SOCKET_TIMEOUT: Duration = Duration::from_secs(30);
const APP_REPLY_TIMEOUT: Duration = Duration::from_secs(60);
const MIN_TOKEN_BYTES: usize = 32;
const MAX_TOKEN_BYTES: usize = 1_024;
const TOKEN_ENV: &str = "DESIGNCRAFT_CONTROL_TOKEN";

/// Start a bounded control server. When no token was configured, a fresh capability token is
/// printed to stderr so a deliberate local client can copy it into `DESIGNCRAFT_CONTROL_TOKEN`.
pub fn start(port: u16, ctx: egui::Context) -> Result<Receiver<ControlRequest>, String> {
    let (token, generated) = control_token()?;
    let (tx, rx) = channel::<ControlRequest>();
    let listener = TcpListener::bind(("127.0.0.1", port)).map_err(|e| format!("failed to bind 127.0.0.1:{port}: {e}"))?;
    eprintln!("designcraft: authenticated control server listening on 127.0.0.1:{port}");
    if generated {
        eprintln!("designcraft: control token: {token}");
        eprintln!("designcraft: set {TOKEN_ENV} to this token in clients");
    }
    std::thread::Builder::new()
        .name("designcraft-control-listener".into())
        .spawn(move || accept_loop(listener, tx, ctx, token))
        .map_err(|e| format!("failed to start control listener: {e}"))?;
    Ok(rx)
}

fn control_token() -> Result<(String, bool), String> {
    match std::env::var(TOKEN_ENV) {
        Ok(token) => {
            validate_token(&token)?;
            Ok((token, false))
        }
        Err(std::env::VarError::NotUnicode(_)) => Err(format!("{TOKEN_ENV} must be valid UTF-8")),
        Err(std::env::VarError::NotPresent) => {
            let mut bytes = [0_u8; 32];
            getrandom::fill(&mut bytes).map_err(|e| format!("could not generate a control token: {e}"))?;
            let mut token = String::with_capacity(bytes.len() * 2);
            for byte in bytes {
                use std::fmt::Write as _;
                write!(token, "{byte:02x}").map_err(|_| "could not encode the generated control token".to_string())?;
            }
            Ok((token, true))
        }
    }
}

fn validate_token(token: &str) -> Result<(), String> {
    let len = token.len();
    if (MIN_TOKEN_BYTES..=MAX_TOKEN_BYTES).contains(&len) {
        Ok(())
    } else {
        Err(format!("{TOKEN_ENV} must contain between {MIN_TOKEN_BYTES} and {MAX_TOKEN_BYTES} bytes"))
    }
}

fn accept_loop(listener: TcpListener, tx: Sender<ControlRequest>, ctx: egui::Context, token: String) {
    let active = Arc::new(AtomicUsize::new(0));
    let token: Arc<str> = Arc::from(token);
    for incoming in listener.incoming() {
        let mut stream = match incoming {
            Ok(stream) => stream,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => {
                eprintln!("designcraft: control listener stopped: {e}");
                break;
            }
        };
        let Some(slot) = ConnectionSlot::acquire(Arc::clone(&active)) else {
            let _ = stream.set_write_timeout(Some(SOCKET_TIMEOUT));
            let _ = writeln!(stream, "{}", json!({"ok": false, "error": "too many control connections"}));
            continue;
        };
        let tx = tx.clone();
        let ctx = ctx.clone();
        let token = Arc::clone(&token);
        if let Err(e) = std::thread::Builder::new().name("designcraft-control-client".into()).spawn(move || {
            let _slot = slot;
            serve(stream, &tx, &ctx, &token);
        }) {
            eprintln!("designcraft: could not serve control connection: {e}");
        }
    }
}

struct ConnectionSlot(Arc<AtomicUsize>);

impl ConnectionSlot {
    fn acquire(active: Arc<AtomicUsize>) -> Option<Self> {
        active.fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| (n < MAX_CONNECTIONS).then_some(n + 1)).ok().map(|_| Self(active))
    }
}

impl Drop for ConnectionSlot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

fn serve(stream: TcpStream, tx: &Sender<ControlRequest>, ctx: &egui::Context, token: &str) {
    if stream.set_read_timeout(Some(SOCKET_TIMEOUT)).is_err() || stream.set_write_timeout(Some(SOCKET_TIMEOUT)).is_err() {
        return;
    }
    let Ok(read) = stream.try_clone() else { return };
    serve_lines(read, stream, token, |method, params| {
        let (req, rrx) = ControlRequest::new(method, params);
        tx.send(req).ok()?;
        ctx.request_repaint();
        Some(rrx.recv_timeout(APP_REPLY_TIMEOUT).unwrap_or_else(|_| json!({"ok": false, "error": "timeout"})))
    });
}

/// One parsed line.
enum Line {
    Request {
        id: Value,
        token: Option<String>,
        method: String,
        params: Value,
    },
    Blank,
    /// Not a request: reply with this error and close the connection.
    Reject(String),
}

fn parse_line(bytes: &[u8]) -> Line {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return Line::Reject("not a control request (invalid UTF-8); closing the connection".into());
    };
    let text = text.trim();
    if text.is_empty() {
        return Line::Blank;
    }
    let msg = match serde_json::from_str::<Value>(text) {
        Ok(v) => v,
        Err(e) => return Line::Reject(format!("bad JSON ({e}); expected one JSON request object per line; closing the connection")),
    };
    let Some(obj) = msg.as_object() else {
        return Line::Reject("not a control request (expected a JSON object); closing the connection".into());
    };
    let Some(method) = obj.get("method").and_then(Value::as_str) else {
        return Line::Reject("not a control request (missing string `method`); closing the connection".into());
    };
    if method.is_empty() || method.len() > 256 {
        return Line::Reject("not a control request (invalid `method`); closing the connection".into());
    }
    let id = match obj.get("id") {
        Some(Value::Number(id)) => Value::Number(id.clone()),
        Some(Value::String(id)) if id.len() <= 128 => Value::String(id.clone()),
        Some(Value::Null) | None => Value::Null,
        _ => Value::Null,
    };
    Line::Request {
        id,
        token: obj.get("token").and_then(Value::as_str).map(str::to_string),
        method: method.to_string(),
        params: obj.get("params").cloned().unwrap_or_else(|| json!({})),
    }
}

/// Read one `\n`-terminated line of at most `max` bytes into `buf` (newline stripped).
/// `Ok(false)` at end of stream; `Err` when the line is too long or the read fails.
fn read_bounded_line(reader: &mut impl BufRead, buf: &mut Vec<u8>, max: usize) -> Result<bool, String> {
    buf.clear();
    let limit = u64::try_from(max).unwrap_or(u64::MAX).saturating_add(1);
    let n = reader.take(limit).read_until(b'\n', buf).map_err(|e| e.to_string())?;
    if n == 0 {
        return Ok(false);
    }
    if buf.last() == Some(&b'\n') {
        buf.pop();
        if buf.last() == Some(&b'\r') {
            buf.pop();
        }
        return Ok(true);
    }
    if buf.len() > max {
        return Err(format!("request line longer than {max} bytes; closing the connection"));
    }
    // Preserve the existing EOF-terminated final-line behavior. A live partial request is still
    // bounded by the socket read timeout.
    Ok(true)
}

/// Serve authenticated requests until the peer closes, framing fails, or the request budget is
/// exhausted. The handler runs only after authentication.
fn serve_lines<R: Read, W: Write>(read: R, mut out: W, token: &str, mut handle: impl FnMut(String, Value) -> Option<Value>) {
    let mut reader = BufReader::new(read);
    let mut buf = Vec::new();
    let mut requests = 0;
    loop {
        let reject = match read_bounded_line(&mut reader, &mut buf, MAX_LINE) {
            Ok(false) => return,
            Err(e) => e,
            Ok(true) => match parse_line(&buf) {
                Line::Blank => continue,
                Line::Reject(e) => e,
                Line::Request { id, token: supplied, method, params } => {
                    if !supplied.as_deref().is_some_and(|candidate| constant_time_eq(candidate.as_bytes(), token.as_bytes())) {
                        let _ = writeln!(out, "{}", json!({"id": id, "ok": false, "error": "unauthorized"})).and_then(|()| out.flush());
                        return;
                    }
                    requests += 1;
                    let Some(mut reply) = handle(method, params) else { return };
                    if let Some(object) = reply.as_object_mut() {
                        object.insert("id".into(), id);
                    }
                    if writeln!(out, "{reply}").and_then(|()| out.flush()).is_err() {
                        return;
                    }
                    if requests >= MAX_REQUESTS_PER_CONNECTION {
                        return;
                    }
                    continue;
                }
            },
        };
        let _ = writeln!(out, "{}", json!({"ok": false, "error": reject})).and_then(|()| out.flush());
        return;
    }
}

fn constant_time_eq(candidate: &[u8], expected: &[u8]) -> bool {
    let mut different = candidate.len() ^ expected.len();
    for (index, expected_byte) in expected.iter().enumerate() {
        different |= usize::from(candidate.get(index).copied().unwrap_or(0) ^ expected_byte);
    }
    different == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOKEN: &str = "this-test-token-is-at-least-32-bytes";

    fn request(id: u64, method: &str) -> String {
        format!("{}\n", json!({"id": id, "token": TOKEN, "method": method}))
    }

    fn run(input: &[u8]) -> (Vec<Value>, Vec<String>) {
        let mut out = Vec::new();
        let mut calls = Vec::new();
        serve_lines(input, &mut out, TOKEN, |m, _p| {
            calls.push(m);
            Some(json!({"ok": true, "result": null}))
        });
        let replies = String::from_utf8(out).unwrap().lines().map(|line| serde_json::from_str::<Value>(line).unwrap()).collect();
        (replies, calls)
    }

    #[test]
    fn authenticated_requests_are_answered_in_order_with_ids() {
        let input = format!("{}\n{}\r\n{}", request(1, "a").trim_end(), request(2, "b").trim_end(), json!({"token": TOKEN, "method": "c"}));
        let (replies, calls) = run(input.as_bytes());
        assert_eq!(calls, ["a", "b", "c"]);
        assert_eq!(replies.len(), 3);
        assert_eq!(replies[0]["id"], 1);
        assert_eq!(replies[1]["id"], 2);
        assert_eq!(replies[2]["id"], Value::Null);
    }

    #[test]
    fn missing_token_is_rejected_before_dispatch() {
        let (replies, calls) = run(b"{\"id\":8,\"method\":\"ui.inspect\"}\n");
        assert!(calls.is_empty());
        assert_eq!(replies.len(), 1);
        assert_eq!(replies[0]["error"], "unauthorized");
    }

    #[test]
    fn http_request_closes_before_later_input() {
        let input = format!("POST / HTTP/1.1\r\nHost: 127.0.0.1:7979\r\n\r\n{}", request(7, "ui.inspect"));
        let (replies, calls) = run(input.as_bytes());
        assert!(calls.is_empty());
        assert_eq!(replies.len(), 1);
        assert_eq!(replies[0]["ok"], false);
        assert!(replies[0]["error"].as_str().unwrap().contains("closing the connection"));
    }

    #[test]
    fn non_request_json_closes_the_connection() {
        let valid = request(1, "a");
        for prefix in ["[1,2]\n", "{\"id\":1}\n", "{\"method\":5}\n", "\u{fffd}\n"] {
            let input = format!("{prefix}{valid}");
            let (replies, calls) = run(input.as_bytes());
            assert!(calls.is_empty(), "{calls:?}");
            assert_eq!(replies.len(), 1);
            assert_eq!(replies[0]["ok"], false);
        }
        let mut invalid_utf8 = vec![0xff, 0xfe, b'\n'];
        invalid_utf8.extend_from_slice(valid.as_bytes());
        let (replies, calls) = run(&invalid_utf8);
        assert!(calls.is_empty());
        assert_eq!(replies.len(), 1);
    }

    #[test]
    fn a_request_before_junk_still_runs() {
        let input = format!("{}GET / HTTP/1.1\n{}", request(1, "a"), request(2, "b"));
        let (replies, calls) = run(input.as_bytes());
        assert_eq!(calls, ["a"]);
        assert_eq!(replies.len(), 2);
        assert_eq!(replies[1]["ok"], false);
    }

    #[test]
    fn overlong_line_is_refused() {
        let mut input = vec![b'x'; MAX_LINE + 1];
        input.extend_from_slice(format!("\n{}", request(1, "a")).as_bytes());
        let (replies, calls) = run(&input);
        assert!(calls.is_empty(), "{calls:?}");
        assert_eq!(replies.len(), 1);
        assert!(replies[0]["error"].as_str().unwrap().contains("longer than"));
    }

    #[test]
    fn requests_per_connection_are_bounded() {
        let mut input = String::new();
        for id in 0..=MAX_REQUESTS_PER_CONNECTION {
            input.push_str(&request(id as u64, "a"));
        }
        let (replies, calls) = run(input.as_bytes());
        assert_eq!(calls.len(), MAX_REQUESTS_PER_CONNECTION);
        assert_eq!(replies.len(), MAX_REQUESTS_PER_CONNECTION);
    }

    #[test]
    fn line_at_the_limit_is_accepted_by_the_reader() {
        let mut buf = Vec::new();
        let line = vec![b'a'; 10];
        let mut input = line.clone();
        input.push(b'\n');
        assert_eq!(read_bounded_line(&mut &input[..], &mut buf, 10), Ok(true));
        assert_eq!(buf, line);
        assert!(read_bounded_line(&mut &[b'a'; 11][..], &mut buf, 10).is_err());
    }

    #[test]
    fn tcp_connection_is_closed_after_junk() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let read = stream.try_clone().unwrap();
            serve_lines(read, stream, TOKEN, |_, _| Some(json!({"ok": true})));
        });
        let mut client = TcpStream::connect(addr).unwrap();
        client.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        client.write_all(format!("{}hello\n{}", request(7, "x"), request(8, "y")).as_bytes()).unwrap();
        let mut text = String::new();
        client.read_to_string(&mut text).unwrap();
        server.join().unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2, "{text}");
        assert!(lines[0].contains("\"id\":7"));
        assert!(lines[1].contains("closing the connection"));
    }

    #[test]
    fn connection_slots_are_bounded_and_reusable() {
        let active = Arc::new(AtomicUsize::new(0));
        let mut slots = Vec::new();
        for _ in 0..MAX_CONNECTIONS {
            slots.push(ConnectionSlot::acquire(Arc::clone(&active)).unwrap());
        }
        assert!(ConnectionSlot::acquire(Arc::clone(&active)).is_none());
        drop(slots.pop());
        assert!(ConnectionSlot::acquire(active).is_some());
    }

    #[test]
    fn token_validation_and_comparison_are_bounded() {
        assert!(validate_token(TOKEN).is_ok());
        assert!(validate_token("short").is_err());
        assert!(constant_time_eq(b"same", b"same"));
        assert!(!constant_time_eq(b"same", b"different"));
        assert!(!constant_time_eq(b"same-extra", b"same"));
    }
}
