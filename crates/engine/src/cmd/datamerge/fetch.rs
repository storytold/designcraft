//! Fetch an `http` or `https` image for data merge.
//!
//! `http` uses a short-lived TCP connection. `https` uses the system `curl` when this build is
//! not targeting wasm. A failure is an error string; the caller turns it into a merge warning.

use std::io::{Read, Write};
use std::time::Duration;

use super::MAX_IMAGE_FILE_BYTES;

const MAX_REDIRECTS: u8 = 3;
const TIMEOUT: Duration = Duration::from_secs(5);

/// `Some` when `cell` is an http or https URL.
pub fn remote_url(cell: &str) -> Option<&str> {
    let text = cell.trim();
    let lower = text.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") { Some(text) } else { None }
}

pub fn fetch_url(url: &str) -> Result<Vec<u8>, String> {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = url;
        Err("remote images are not fetched in this build".into())
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        fetch_followed(url, 0)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn fetch_followed(url: &str, redirects: u8) -> Result<Vec<u8>, String> {
    let (https, host, port, path) = split_url(url)?;
    if https {
        return fetch_curl(url);
    }
    let (code, headers, body) = http_get(&host, port, &path)?;
    if matches!(code, 301 | 302 | 303 | 307 | 308) {
        if redirects >= MAX_REDIRECTS {
            return Err(format!("{url}: too many redirects"));
        }
        let location = headers.iter().find(|(k, _)| k == "location").map(|(_, v)| v.as_str()).unwrap_or("");
        if location.is_empty() {
            return Err(format!("{url}: redirect with no location"));
        }
        let next = resolve_redirect(url, location)?;
        if remote_url(&next).is_none() {
            return Err(format!("{url}: redirect is not http or https"));
        }
        return fetch_followed(&next, redirects.saturating_add(1));
    }
    if !(200..300).contains(&code) {
        return Err(format!("{url}: HTTP {code}"));
    }
    if body.is_empty() {
        return Err(format!("{url}: empty response"));
    }
    Ok(body)
}

#[cfg(not(target_arch = "wasm32"))]
fn fetch_curl(url: &str) -> Result<Vec<u8>, String> {
    let output = std::process::Command::new("curl")
        .args(["--fail", "--silent", "--show-error", "--location", "--max-redirs", "3", "--max-time", "10", "--output", "-", url])
        .output()
        .map_err(|err| format!("https fetch failed: {err}"))?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr);
        let detail = detail.trim();
        if detail.is_empty() {
            return Err(format!("{url}: https fetch failed"));
        }
        return Err(format!("{url}: {detail}"));
    }
    if output.stdout.is_empty() {
        return Err(format!("{url}: empty response"));
    }
    if output.stdout.len() as u64 > MAX_IMAGE_FILE_BYTES {
        return Err(format!("{url}: image is larger than {} MiB", MAX_IMAGE_FILE_BYTES / (1024 * 1024)));
    }
    Ok(output.stdout)
}

#[cfg(not(target_arch = "wasm32"))]
fn http_get(host: &str, port: u16, path: &str) -> Result<(u16, Vec<(String, String)>, Vec<u8>), String> {
    let addr = std::net::ToSocketAddrs::to_socket_addrs(&(host, port))
        .map_err(|err| format!("{host}:{port}: {err}"))?
        .next()
        .ok_or_else(|| format!("{host}:{port}: no address"))?;
    let mut stream = std::net::TcpStream::connect_timeout(&addr, TIMEOUT).map_err(|err| format!("{host}:{port}: {err}"))?;
    stream.set_read_timeout(Some(TIMEOUT)).map_err(|err| err.to_string())?;
    stream.set_write_timeout(Some(TIMEOUT)).map_err(|err| err.to_string())?;
    let request = format!("GET {path} HTTP/1.0\r\nHost: {host}\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).map_err(|err| err.to_string())?;
    let mut buf = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                let next = (buf.len() as u64).saturating_add(n as u64);
                if next > MAX_IMAGE_FILE_BYTES.saturating_add(65_536) {
                    return Err(format!("{host}: image is larger than {} MiB", MAX_IMAGE_FILE_BYTES / (1024 * 1024)));
                }
                buf.extend_from_slice(&chunk[..n]);
            }
            Err(err) if err.kind() == std::io::ErrorKind::TimedOut || err.kind() == std::io::ErrorKind::WouldBlock => {
                return Err(format!("{host}:{port}: timed out"));
            }
            Err(err) => return Err(format!("{host}:{port}: {err}")),
        }
    }
    let head_end = buf.windows(4).position(|w| w == b"\r\n\r\n").ok_or_else(|| format!("{host}: incomplete HTTP response"))?;
    let head = std::str::from_utf8(&buf[..head_end]).map_err(|_| format!("{host}: response headers are not UTF-8"))?;
    let mut lines = head.split("\r\n");
    let status = lines.next().unwrap_or("");
    let code = status.split_whitespace().nth(1).and_then(|n| n.parse().ok()).ok_or_else(|| format!("{host}: bad status line"))?;
    let mut headers = Vec::new();
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            headers.push((name.trim().to_ascii_lowercase(), value.trim().to_string()));
        }
    }
    let mut body = buf.split_off(head_end.saturating_add(4));
    if let Some(len) = headers.iter().find(|(k, _)| k == "content-length").and_then(|(_, v)| v.parse::<usize>().ok())
        && body.len() > len
    {
        body.truncate(len);
    }
    if body.len() as u64 > MAX_IMAGE_FILE_BYTES {
        return Err(format!("{host}: image is larger than {} MiB", MAX_IMAGE_FILE_BYTES / (1024 * 1024)));
    }
    Ok((code, headers, body))
}

fn split_url(url: &str) -> Result<(bool, String, u16, String), String> {
    let text = url.trim();
    let lower = text.to_ascii_lowercase();
    let (https, rest) = if lower.starts_with("https://") {
        (true, &text["https://".len()..])
    } else if lower.starts_with("http://") {
        (false, &text["http://".len()..])
    } else {
        return Err(format!("{url}: not an http or https URL"));
    };
    if rest.is_empty() {
        return Err(format!("{url}: missing host"));
    }
    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], rest[i..].to_string()),
        None => (rest, "/".to_string()),
    };
    let hostport = authority.rsplit_once('@').map(|(_, host)| host).unwrap_or(authority);
    if hostport.is_empty() {
        return Err(format!("{url}: missing host"));
    }
    let (host, port) = if let Some(rest) = hostport.strip_prefix('[') {
        let end = rest.find(']').ok_or_else(|| format!("{url}: bad host"))?;
        let host = rest[..end].to_string();
        let after = &rest[end + 1..];
        let port = if let Some(p) = after.strip_prefix(':') {
            p.parse::<u16>().map_err(|_| format!("{url}: bad port"))?
        } else if after.is_empty() {
            if https { 443 } else { 80 }
        } else {
            return Err(format!("{url}: bad host"));
        };
        (host, port)
    } else if let Some((host, port)) = hostport.rsplit_once(':') {
        if port.chars().all(|c| c.is_ascii_digit()) && !host.is_empty() {
            (host.to_string(), port.parse::<u16>().map_err(|_| format!("{url}: bad port"))?)
        } else {
            (hostport.to_string(), if https { 443 } else { 80 })
        }
    } else {
        (hostport.to_string(), if https { 443 } else { 80 })
    };
    if host.is_empty() {
        return Err(format!("{url}: missing host"));
    }
    let path = if path.is_empty() { "/".to_string() } else { path };
    Ok((https, host, port, path))
}

fn resolve_redirect(base: &str, location: &str) -> Result<String, String> {
    let location = location.trim();
    if remote_url(location).is_some() {
        return Ok(location.to_string());
    }
    if let Some(stripped) = location.strip_prefix("//") {
        let scheme = if base.to_ascii_lowercase().starts_with("https://") { "https" } else { "http" };
        return Ok(format!("{scheme}://{stripped}"));
    }
    let (https, host, port, _) = split_url(base)?;
    let scheme = if https { "https" } else { "http" };
    let default_port = if https { 443 } else { 80 };
    let origin = if port == default_port { format!("{scheme}://{host}") } else { format!("{scheme}://{host}:{port}") };
    if let Some(path) = location.strip_prefix('/') {
        return Ok(format!("{origin}/{path}"));
    }
    Err(format!("{base}: cannot follow redirect {location}"))
}
