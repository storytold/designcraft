//! DesignCraft as an HTTP render server: fill a template's tagged frames from XML (text and
//! images) and get back a PDF, PNG, JPEG, `.designcraft` or IDML file.
//!
//! ```text
//! designcraft-server [--listen ADDR] [--templates DIR] [--assets DIR]... [--workers N] [--max-body MB]
//!
//! GET  /health                      {"ok": true, "version"}
//! GET  /templates                   {"templates": [paths relative to --templates]}
//! GET  /templates/{path}/structure  {"root", "elements": [{tag, id, kind, text?}], "tags"}
//! POST /render                      JSON (render::Request) → the file; the import report is in the
//!                                   X-DesignCraft-Report header
//! ```
//!
//! See docs/server.md.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod render;

use std::io::Read as _;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use serde_json::{Value, json};

const USAGE: &str = "usage: designcraft-server [--listen ADDR] [--templates DIR] [--assets DIR]... [--workers N] [--max-body MB]
  --listen ADDR    address and port to listen on (default 127.0.0.1:8080)
  --templates DIR  folder of .designcraft and .idml templates that requests name by path
  --assets DIR     a folder the XML may name images in by absolute path; may be repeated
  --workers N      requests handled at the same time (default: the number of CPUs)
  --max-body MB    the largest request body accepted (default 200)";

struct Config {
    listen: String,
    folders: render::Folders,
    workers: usize,
    max_body: u64,
}

fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Config, String> {
    let mut cfg = Config {
        listen: "127.0.0.1:8080".into(),
        folders: render::Folders::default(),
        workers: std::thread::available_parallelism().map_or(4, |n| n.get()),
        max_body: 200,
    };
    let mut it = args.into_iter();
    while let Some(a) = it.next() {
        let mut value = || it.next().ok_or_else(|| format!("{a} needs a value"));
        match a.as_str() {
            "--listen" => cfg.listen = value()?,
            "--templates" => cfg.folders.templates = Some(folder(&value()?)?),
            "--assets" => cfg.folders.assets.push(folder(&value()?)?),
            "--workers" => cfg.workers = value()?.parse().ok().filter(|n| *n > 0).ok_or("--workers needs a number above 0")?,
            "--max-body" => cfg.max_body = value()?.parse().ok().filter(|n| *n > 0).ok_or("--max-body needs a number of megabytes above 0")?,
            other => return Err(format!("unknown option {other}")),
        }
    }
    Ok(cfg)
}

/// An existing folder, absolute.
fn folder(path: &str) -> Result<PathBuf, String> {
    let p = std::path::absolute(path).map_err(|e| format!("{path}: {e}"))?;
    if p.is_dir() { Ok(p) } else { Err(format!("{path} isn't a folder")) }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    let cfg = match parse_args(args) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("designcraft-server: {e}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    let server = match tiny_http::Server::http(&cfg.listen) {
        Ok(s) => Arc::new(s),
        Err(e) => {
            eprintln!("designcraft-server: can't listen on {}: {e}", cfg.listen);
            return ExitCode::FAILURE;
        }
    };
    eprintln!(
        "designcraft-server {} listening on http://{} (templates: {}, workers: {})",
        env!("CARGO_PKG_VERSION"),
        cfg.listen,
        cfg.folders.templates.as_ref().map_or("none".into(), |t| t.display().to_string()),
        cfg.workers
    );
    let cfg = Arc::new(cfg);
    let workers: Vec<_> = (0..cfg.workers)
        .map(|_| {
            let (server, cfg) = (server.clone(), cfg.clone());
            std::thread::spawn(move || {
                while let Ok(req) = server.recv() {
                    handle(req, &cfg);
                }
            })
        })
        .collect();
    for w in workers {
        let _ = w.join();
    }
    ExitCode::SUCCESS
}

fn header(name: &str, value: &str) -> Option<tiny_http::Header> {
    tiny_http::Header::from_bytes(name.as_bytes(), value.as_bytes()).ok()
}

/// JSON with everything outside ASCII escaped, so it fits in a header.
fn ascii_json(v: &Value) -> String {
    let mut out = String::new();
    for c in v.to_string().chars() {
        if c.is_ascii() && !c.is_ascii_control() {
            out.push(c);
        } else {
            let mut buf = [0u16; 2];
            for unit in c.encode_utf16(&mut buf) {
                out.push_str(&format!("\\u{unit:04x}"));
            }
        }
    }
    out
}

fn respond(req: tiny_http::Request, status: u16, content_type: &str, body: Vec<u8>, extra: Vec<tiny_http::Header>) {
    let mut r = tiny_http::Response::from_data(body).with_status_code(status);
    if let Some(h) = header("Content-Type", content_type) {
        r = r.with_header(h);
    }
    for h in extra {
        r = r.with_header(h);
    }
    let _ = req.respond(r);
}

fn respond_json(req: tiny_http::Request, status: u16, body: &Value) {
    respond(req, status, "application/json", body.to_string().into_bytes(), vec![]);
}

fn handle(mut req: tiny_http::Request, cfg: &Config) {
    let method = req.method().clone();
    let url = req.url().split('?').next().unwrap_or("").to_string();
    let path = url.trim_end_matches('/');
    let started = std::time::Instant::now();
    let status = match (&method, path) {
        (tiny_http::Method::Get, "/health") => {
            respond_json(req, 200, &json!({"ok": true, "version": env!("CARGO_PKG_VERSION")}));
            200
        }
        (tiny_http::Method::Get, "/templates") => match render::templates(&cfg.folders) {
            Ok(t) => {
                respond_json(req, 200, &json!({"templates": t}));
                200
            }
            Err(e) => fail(req, &e),
        },
        (tiny_http::Method::Get, p) if p.starts_with("/templates/") && p.ends_with("/structure") => {
            let name = percent_decode(&p["/templates/".len()..p.len() - "/structure".len()]);
            match guarded(|| render::structure(&name, &cfg.folders)) {
                Ok(v) => {
                    respond_json(req, 200, &v);
                    200
                }
                Err(e) => fail(req, &e),
            }
        }
        (tiny_http::Method::Post, "/render") => {
            let limit = cfg.max_body * 1024 * 1024;
            if req.body_length().is_some_and(|n| n as u64 > limit) {
                respond_json(req, 413, &json!({"error": format!("the body is over {} MB (--max-body)", cfg.max_body)}));
                return;
            }
            let mut body = Vec::new();
            if let Err(e) = req.as_reader().take(limit + 1).read_to_end(&mut body) {
                fail(req, &render::Error::BadRequest(format!("can't read the body: {e}")));
                return;
            }
            if body.len() as u64 > limit {
                respond_json(req, 413, &json!({"error": format!("the body is over {} MB (--max-body)", cfg.max_body)}));
                return;
            }
            match serde_json::from_slice::<render::Request>(&body) {
                Err(e) => fail(req, &render::Error::BadRequest(format!("not a render request: {e}"))),
                Ok(r) => match guarded(|| render::render(&r, &cfg.folders)) {
                    Ok(out) => {
                        let mut extra: Vec<tiny_http::Header> = Vec::new();
                        extra.extend(header("X-DesignCraft-Report", &ascii_json(&out.report)));
                        extra.extend(header("Content-Disposition", &format!("attachment; filename=\"render.{}\"", out.extension)));
                        respond(req, 200, out.content_type, out.bytes, extra);
                        200
                    }
                    Err(e) => fail(req, &e),
                },
            }
        }
        (_, "/health" | "/templates" | "/render") => {
            respond_json(req, 405, &json!({"error": "method not allowed"}));
            405
        }
        _ => {
            respond_json(
                req,
                404,
                &json!({"error": "not found", "endpoints": ["GET /health", "GET /templates", "GET /templates/{path}/structure", "POST /render"]}),
            );
            404
        }
    };
    eprintln!("{method} {url} → {status} ({:.0} ms)", started.elapsed().as_secs_f64() * 1000.0);
}

fn fail(req: tiny_http::Request, e: &render::Error) -> u16 {
    respond_json(req, e.status(), &e.body());
    e.status()
}

/// A panic in the engine becomes a 500 and the worker goes on.
fn guarded<T>(f: impl FnOnce() -> Result<T, render::Error>) -> Result<T, render::Error> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).unwrap_or_else(|p| {
        let msg = p.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| p.downcast_ref::<String>().cloned()).unwrap_or_default();
        Err(render::Error::Internal(format!("the engine failed: {msg}")))
    })
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = |c: u8| (c as char).to_digit(16);
        match (b[i], b.get(i + 1).copied().and_then(hex), b.get(i + 2).copied().and_then(hex)) {
            (b'%', Some(h), Some(l)) => {
                out.push((h * 16 + l) as u8);
                i += 3;
            }
            (c, _, _) => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arguments() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path().to_string_lossy().to_string();
        let args = ["--listen", "0.0.0.0:9000", "--templates", &d, "--assets", &d, "--workers", "2", "--max-body", "5"];
        let cfg = parse_args(args.iter().map(|s| s.to_string())).unwrap();
        assert_eq!((cfg.listen.as_str(), cfg.workers, cfg.max_body, cfg.folders.assets.len()), ("0.0.0.0:9000", 2, 5, 1));
        assert!(parse_args(["--workers".to_string(), "0".to_string()]).is_err());
        assert!(parse_args(["--templates".to_string(), d.clone() + "/missing"]).is_err());
        assert!(parse_args(["--bogus".to_string()]).is_err());
    }

    #[test]
    fn header_json_is_ascii() {
        let escaped = ascii_json(&json!({"t": "Gr\u{fc}\u{df}e"}));
        assert!(escaped.is_ascii() && escaped.contains("u00fc") && escaped.contains("u00df"), "{escaped}");
        assert_eq!(serde_json::from_str::<Value>(&escaped).unwrap()["t"], "Gr\u{fc}\u{df}e", "still the same JSON");
        assert_eq!(percent_decode("issue%202.designcraft"), "issue 2.designcraft");
    }
}
