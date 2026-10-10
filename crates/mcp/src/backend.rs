//! Where MCP tool calls end up: a control-channel method call.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use serde_json::{Value, json};

const MIN_CONTROL_TOKEN_BYTES: usize = 32;
const MAX_CONTROL_TOKEN_BYTES: usize = 1_024;
const MAX_CONTROL_REQUEST_BYTES: usize = 4 * 1024 * 1024;
const MAX_CONTROL_REPLY_BYTES: usize = 64 * 1024 * 1024;

/// Something that answers control-channel methods (`engine.execute`, `document.inspect`,
/// `ui.pointer`, `ui.render`, …). See `designcraft_ui_egui::control` for the full list.
pub trait Backend {
    /// Call one method. `Ok` carries the `result`, `Err` the error message.
    fn call(&mut self, method: &str, params: Value) -> Result<Value, String>;
    /// True when a real UI is attached (`ui.screenshot`, `ui.click`, dialogs… work).
    fn has_ui(&self) -> bool;
    /// Short human description ("headless", "connected to 127.0.0.1:7979").
    fn describe(&self) -> String;
}

/// A running DesignCraft app, reached through its loopback control port.
pub struct Remote {
    addr: String,
    token: String,
    conn: Option<(BufReader<TcpStream>, TcpStream)>,
    next_id: u64,
}

impl Remote {
    /// Connect with the capability from `DESIGNCRAFT_CONTROL_TOKEN`.
    pub fn connect(addr: &str) -> std::io::Result<Self> {
        let token = std::env::var("DESIGNCRAFT_CONTROL_TOKEN").map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::PermissionDenied, format!("DESIGNCRAFT_CONTROL_TOKEN is required for a control connection: {e}"))
        })?;
        Self::connect_with_token(addr, token)
    }

    /// Connect to `addr` with an explicit capability token.
    pub fn connect_with_token(addr: &str, token: impl Into<String>) -> std::io::Result<Self> {
        let token = token.into();
        if !(MIN_CONTROL_TOKEN_BYTES..=MAX_CONTROL_TOKEN_BYTES).contains(&token.len()) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("control token must contain between {MIN_CONTROL_TOKEN_BYTES} and {MAX_CONTROL_TOKEN_BYTES} bytes"),
            ));
        }
        let mut r = Self { addr: addr.to_string(), token, conn: None, next_id: 1 };
        r.reconnect()?;
        Ok(r)
    }

    pub fn addr(&self) -> &str {
        &self.addr
    }

    fn reconnect(&mut self) -> std::io::Result<()> {
        self.conn = None;
        let mut last = std::io::Error::new(std::io::ErrorKind::NotFound, format!("cannot resolve {}", self.addr));
        for sa in self.addr.to_socket_addrs()? {
            if !sa.ip().is_loopback() {
                last =
                    std::io::Error::new(std::io::ErrorKind::PermissionDenied, format!("control connections must use a loopback address, not {sa}"));
                continue;
            }
            match TcpStream::connect_timeout(&sa, Duration::from_millis(800)) {
                Ok(s) => {
                    s.set_nodelay(true).ok();
                    // The app answers within 60 s (its own timeout); leave headroom.
                    s.set_read_timeout(Some(Duration::from_secs(90))).ok();
                    s.set_write_timeout(Some(Duration::from_secs(30))).ok();
                    let read = s.try_clone()?;
                    self.conn = Some((BufReader::new(read), s));
                    return Ok(());
                }
                Err(e) => last = e,
            }
        }
        Err(last)
    }

    fn roundtrip(&mut self, line: &str) -> std::io::Result<String> {
        if self.conn.is_none() {
            self.reconnect()?;
        }
        let Some((reader, writer)) = self.conn.as_mut() else {
            return Err(std::io::Error::new(std::io::ErrorKind::NotConnected, "not connected"));
        };
        writer.write_all(line.as_bytes())?;
        writer.write_all(b"\n")?;
        writer.flush()?;
        let mut reply = String::new();
        let bytes_read = reader.by_ref().take((MAX_CONTROL_REPLY_BYTES + 1) as u64).read_line(&mut reply)?;
        if bytes_read == 0 {
            return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "control channel closed"));
        }
        if bytes_read > MAX_CONTROL_REPLY_BYTES || !reply.ends_with('\n') {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid control reply framing"));
        }
        Ok(reply)
    }
}

impl Backend for Remote {
    fn call(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.next_id;
        self.next_id += 1;
        let line = json!({"id": id, "token": self.token, "method": method, "params": params}).to_string();
        if line.len() > MAX_CONTROL_REQUEST_BYTES {
            return Err(format!("control request is larger than {MAX_CONTROL_REQUEST_BYTES} bytes"));
        }
        // One retry with a fresh connection (the app may have restarted).
        let reply = match self.roundtrip(&line) {
            Ok(r) => r,
            Err(_) => {
                self.conn = None;
                self.roundtrip(&line).map_err(|e| {
                    self.conn = None;
                    format!("DesignCraft app at {} is not reachable: {e}", self.addr)
                })?
            }
        };
        let v: Value = serde_json::from_str(reply.trim()).map_err(|e| format!("bad reply from app: {e}"))?;
        if v.get("ok").and_then(Value::as_bool) == Some(true) {
            Ok(v.get("result").cloned().unwrap_or(Value::Null))
        } else {
            Err(v.get("error").and_then(Value::as_str).unwrap_or("unknown error").to_string())
        }
    }

    fn has_ui(&self) -> bool {
        true
    }

    fn describe(&self) -> String {
        format!("connected to the DesignCraft app at {}", self.addr)
    }
}
