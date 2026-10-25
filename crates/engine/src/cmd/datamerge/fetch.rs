//! Fetch an `http` or `https` image for data merge.
//!
//! `http` uses a short-lived TCP connection. `https` uses the system `curl` when this build is
//! not targeting wasm. A failure is an error string; the caller turns it into a merge warning.

#[cfg(not(target_arch = "wasm32"))]
use std::io::{Read, Write};
#[cfg(not(target_arch = "wasm32"))]
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, ToSocketAddrs};
#[cfg(not(target_arch = "wasm32"))]
use std::time::Duration;

#[cfg(not(target_arch = "wasm32"))]
use super::MAX_IMAGE_FILE_BYTES;

#[cfg(not(target_arch = "wasm32"))]
const MAX_REDIRECTS: u8 = 3;
#[cfg(not(target_arch = "wasm32"))]
const TIMEOUT: Duration = Duration::from_secs(5);

#[cfg(test)]
thread_local! {
    static ALLOW_PRIVATE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Tests that speak to a local server set this. Release builds have no bypass.
#[cfg(test)]
pub(crate) fn allow_private_for_test(yes: bool) {
    ALLOW_PRIVATE.with(|flag| flag.set(yes));
}

/// Turns the private-address bypass on until dropped. Test threads are reused, so the flag
/// must not stay set after the test that needed a local server.
#[cfg(test)]
pub(crate) struct PrivateFetchGuard;

#[cfg(test)]
impl PrivateFetchGuard {
    pub(crate) fn enable() -> Self {
        allow_private_for_test(true);
        Self
    }
}

#[cfg(test)]
impl Drop for PrivateFetchGuard {
    fn drop(&mut self) {
        allow_private_for_test(false);
    }
}

/// `Some` when `cell` is an http or https URL with no control characters.
pub fn remote_url(cell: &str) -> Option<&str> {
    let text = cell.trim();
    if text.chars().any(char::is_control) {
        return None;
    }
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
    if url.chars().any(char::is_control) {
        return Err("the picture URL contains a control character".into());
    }
    let (https, host, port, path) = split_url(url)?;
    let addr = public_socket(&host, port)?;
    if https {
        return fetch_curl(url, &host, port, addr, redirects);
    }
    let (code, headers, body) = http_get(&host, addr, &path)?;
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
fn fetch_curl(url: &str, host: &str, port: u16, addr: SocketAddr, redirects: u8) -> Result<Vec<u8>, String> {
    let max = MAX_IMAGE_FILE_BYTES.to_string();
    let host_token = if host.contains(':') { format!("[{host}]") } else { host.to_string() };
    let resolve = match addr.ip() {
        IpAddr::V6(ip) => format!("{host_token}:{port}:[{ip}]"),
        IpAddr::V4(ip) => format!("{host_token}:{port}:{ip}"),
    };
    let mut child = std::process::Command::new("curl")
        .args([
            "--silent",
            "--show-error",
            "--include",
            "--proto",
            "=http,https",
            "--proto-redir",
            "=http,https",
            "--max-redirs",
            "0",
            "--max-time",
            "10",
            "--max-filesize",
            &max,
            "--resolve",
            &resolve,
            url,
        ])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|err| format!("https fetch failed: {err}"))?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let cap = MAX_IMAGE_FILE_BYTES.saturating_add(65_536);
    let read = stdout.map(|out| read_capped(out, cap)).transpose();
    let err_text = stderr.map(|err| read_capped(err, 8_192)).transpose();
    let status = child.wait().map_err(|err| format!("https fetch failed: {err}"))?;
    let body = match read {
        Ok(Some(bytes)) => bytes,
        Ok(None) => return Err(format!("{url}: https fetch failed")),
        Err(err) => {
            let _ = child.kill();
            return Err(format!("{url}: {err}"));
        }
    };
    if !status.success() {
        let detail = match &err_text {
            Ok(Some(bytes)) => String::from_utf8_lossy(bytes).trim().to_string(),
            _ => String::new(),
        };
        if detail.is_empty() {
            return Err(size_error(url));
        }
        return Err(format!("{url}: {detail}"));
    }
    let head_end = body.windows(4).position(|w| w == b"\r\n\r\n").ok_or_else(|| format!("{url}: incomplete HTTP response"))?;
    let head = std::str::from_utf8(&body[..head_end]).map_err(|_| format!("{url}: response headers are not UTF-8"))?;
    let mut lines = head.split("\r\n");
    let status_line = lines.next().unwrap_or("");
    let code = status_line.split_whitespace().nth(1).and_then(|n| n.parse::<u16>().ok()).ok_or_else(|| format!("{url}: bad status line"))?;
    let mut headers = Vec::new();
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            headers.push((name.trim().to_ascii_lowercase(), value.trim().to_string()));
        }
    }
    let mut payload = body[head_end.saturating_add(4)..].to_vec();
    if let Some(len) = headers.iter().find(|(k, _)| k == "content-length").and_then(|(_, v)| v.parse::<usize>().ok())
        && payload.len() > len
    {
        payload.truncate(len);
    }
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
    if payload.is_empty() {
        return Err(format!("{url}: empty response"));
    }
    if payload.len() as u64 > MAX_IMAGE_FILE_BYTES {
        return Err(size_error(url));
    }
    Ok(payload)
}

#[cfg(not(target_arch = "wasm32"))]
fn size_error(url: &str) -> String {
    format!("{url}: image is larger than {} MiB", MAX_IMAGE_FILE_BYTES / (1024 * 1024))
}

#[cfg(not(target_arch = "wasm32"))]
fn read_capped(mut reader: impl Read, cap: u64) -> Result<Vec<u8>, String> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        match reader.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                let next = (buf.len() as u64).saturating_add(n as u64);
                if next > cap {
                    return Err(format!("image is larger than {} MiB", MAX_IMAGE_FILE_BYTES / (1024 * 1024)));
                }
                buf.extend_from_slice(&chunk[..n]);
            }
            Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(err) => return Err(err.to_string()),
        }
    }
    Ok(buf)
}

#[cfg(not(target_arch = "wasm32"))]
fn http_get(host: &str, addr: SocketAddr, path: &str) -> Result<(u16, Vec<(String, String)>, Vec<u8>), String> {
    if path.chars().any(char::is_control) || host.chars().any(char::is_control) {
        return Err("the picture URL contains a control character".into());
    }
    let port = addr.port();
    let mut stream = std::net::TcpStream::connect_timeout(&addr, TIMEOUT).map_err(|err| format!("{host}: {err}"))?;
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

/// Resolves `host` once and returns a socket only when every address is public.
/// The caller connects to that socket, so a later lookup cannot switch the target.
#[cfg(not(target_arch = "wasm32"))]
fn public_socket(host: &str, port: u16) -> Result<SocketAddr, String> {
    if host.is_empty() || host.chars().any(char::is_control) {
        return Err("the picture URL contains a control character".into());
    }
    let addrs = (host, port).to_socket_addrs().map_err(|err| format!("{host}: {err}"))?;
    let mut chosen: Option<SocketAddr> = None;
    for addr in addrs {
        if !private_targets_allowed() && !address_is_public(addr.ip()) {
            return Err(format!("{host}: refusing a non-public address"));
        }
        if chosen.is_none() {
            chosen = Some(addr);
        }
    }
    chosen.ok_or_else(|| format!("{host}: no address"))
}

#[cfg(not(target_arch = "wasm32"))]
fn private_targets_allowed() -> bool {
    #[cfg(test)]
    {
        ALLOW_PRIVATE.with(|flag| flag.get())
    }
    #[cfg(not(test))]
    {
        false
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn address_is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => ipv4_is_public(v4),
        IpAddr::V6(v6) => ipv6_is_public(v6),
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn ipv4_is_public(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    if a == 0 || a == 10 || a == 127 || a >= 224 {
        return false;
    }
    // Carrier-grade NAT, 100.64.0.0/10.
    if a == 100 && (b & 0b1100_0000) == 64 {
        return false;
    }
    // Link-local, including the metadata address 169.254.169.254.
    if a == 169 && b == 254 {
        return false;
    }
    // 172.16.0.0/12.
    if a == 172 && (b & 0xf0) == 16 {
        return false;
    }
    if a == 192 && b == 168 {
        return false;
    }
    // IETF protocol assignments, plus the three documentation networks.
    if a == 192 && b == 0 && (c == 0 || c == 2) {
        return false;
    }
    if a == 198 && b == 51 && c == 100 {
        return false;
    }
    if a == 203 && b == 0 && c == 113 {
        return false;
    }
    // Benchmarking, 198.18.0.0/15.
    if a == 198 && (b == 18 || b == 19) {
        return false;
    }
    true
}

#[cfg(not(target_arch = "wasm32"))]
fn ipv6_is_public(ip: Ipv6Addr) -> bool {
    // An IPv4 address written as IPv6 is judged as IPv4, and mapped form is never public.
    if ip.to_ipv4_mapped().is_some() {
        return false;
    }
    let s = ip.segments();
    if s[0] == 0x2002 {
        let embedded = Ipv4Addr::new((s[1] >> 8) as u8, s[1] as u8, (s[2] >> 8) as u8, s[2] as u8);
        return ipv4_is_public(embedded);
    }
    if s[0] == 0x2001 && s[1] == 0 {
        let embedded = Ipv4Addr::new(!(s[6] >> 8) as u8, !(s[6] as u8), !(s[7] >> 8) as u8, !(s[7] as u8));
        return ipv4_is_public(embedded);
    }
    if s[0] == 0x2001 && s[1] == 0x0db8 {
        return false;
    }
    // Unique local fc00::/7, link-local fe80::/10, and the deprecated site-local fec0::/10.
    if s[0] & 0xfe00 == 0xfc00 || s[0] & 0xffc0 == 0xfe80 || s[0] & 0xffc0 == 0xfec0 {
        return false;
    }
    // Global unicast is 2000::/3. That excludes loopback, unspecified, and multicast.
    s[0] & 0xe000 == 0x2000
}

#[cfg(not(target_arch = "wasm32"))]
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

#[cfg(not(target_arch = "wasm32"))]
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

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::{IpAddr, Ipv4Addr, PrivateFetchGuard, address_is_public, fetch_url, public_socket, remote_url};

    #[test]
    fn control_characters_are_not_a_remote_url() {
        // A trailing newline is trimmed with the rest of the cell. A break inside the URL is refused.
        assert!(remote_url("http://example.com/a\r\nX-Injected: 1").is_none());
        assert!(remote_url("  http://example.com/ok\n").is_some());
        let err = fetch_url("http://127.0.0.1/a\r\nX: 1").expect_err("control character");
        assert!(err.contains("control"), "{err}");
    }

    #[test]
    fn loopback_is_refused() {
        let err = fetch_url("http://127.0.0.1/ok.png").expect_err("loopback");
        assert!(err.contains("non-public"), "{err}");
    }

    #[test]
    fn a_test_may_resolve_loopback_without_connecting() {
        let _guard = PrivateFetchGuard::enable();
        let addr = public_socket("127.0.0.1", 9).expect("loopback allowed for this test");
        assert!(addr.ip().is_loopback());
    }

    #[test]
    fn private_and_documentation_addresses_are_not_public() {
        let refused = [
            Ipv4Addr::new(0, 0, 0, 0),
            Ipv4Addr::new(10, 1, 2, 3),
            Ipv4Addr::new(100, 64, 0, 1),
            Ipv4Addr::new(127, 0, 0, 1),
            Ipv4Addr::new(169, 254, 169, 254),
            Ipv4Addr::new(172, 16, 0, 1),
            Ipv4Addr::new(192, 0, 0, 1),
            Ipv4Addr::new(192, 0, 2, 1),
            Ipv4Addr::new(192, 168, 1, 1),
            Ipv4Addr::new(198, 18, 0, 1),
            Ipv4Addr::new(198, 51, 100, 1),
            Ipv4Addr::new(203, 0, 113, 1),
            Ipv4Addr::new(224, 0, 0, 1),
            Ipv4Addr::BROADCAST,
        ];
        for ip in refused {
            assert!(!address_is_public(IpAddr::V4(ip)), "{ip}");
        }
        assert!(address_is_public(IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1))));
        let refused_v6 = ["::", "::1", "fe80::1", "fd00::1", "fec0::1", "::ffff:1.1.1.1", "2001:db8::1", "2002:0a00:0001::"];
        for text in refused_v6 {
            let ip: IpAddr = text.parse().expect(text);
            assert!(!address_is_public(ip), "{text}");
        }
        let public: IpAddr = "2606:4700:4700::1111".parse().expect("public v6");
        assert!(address_is_public(public));
    }
}
