//! A tiny local HTTP endpoint the Haul browser extension sends downloads to.
//!
//! Only reachable from this PC (127.0.0.1). Requests must carry `X-Haul: 1` (which makes the
//! browser preflight them) and come from a browser extension (or no browser at all), so web
//! pages can't push downloads into Haul.

use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};

pub const DEFAULT_PORT: u16 = 17771;

#[derive(serde::Deserialize, Default, Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AddRequest {
    pub url: String,
    pub filename: String,
    pub referrer: String,
    pub cookies: String,
    pub user_agent: String,
    pub file_size: u64,
}

pub async fn serve() {
    let mut bound_port = 0u16;
    let mut listener: Option<TcpListener> = None;
    loop {
        let (enabled, port) = crate::settings::with(|s| (s.bridge_enabled, s.bridge_port));
        if !enabled {
            listener = None;
            bound_port = 0;
            tokio::time::sleep(Duration::from_secs(2)).await;
            continue;
        }
        if listener.is_none() || bound_port != port {
            match TcpListener::bind(("127.0.0.1", port)).await {
                Ok(l) => {
                    listener = Some(l);
                    bound_port = port;
                }
                Err(e) => {
                    crate::paths::log(format!("browser bridge: can't listen on {port}: {e}"));
                    tokio::time::sleep(Duration::from_secs(10)).await;
                    continue;
                }
            }
        }
        let l = listener.as_ref().unwrap();
        // Re-check settings every couple of seconds even when idle.
        match tokio::time::timeout(Duration::from_secs(2), l.accept()).await {
            Ok(Ok((sock, addr))) if addr.ip().is_loopback() => {
                tokio::spawn(async move {
                    let _ = tokio::time::timeout(Duration::from_secs(15), handle(sock, port)).await;
                });
            }
            _ => {}
        }
    }
}

fn origin_ok(origin: &str) -> bool {
    origin.is_empty()
        || origin.starts_with("chrome-extension://")
        || origin.starts_with("moz-extension://")
        || origin.starts_with("extension://")
}

async fn handle(sock: TcpStream, port: u16) -> std::io::Result<()> {
    let (rd, mut wr) = sock.into_split();
    let mut rd = BufReader::new(rd);
    let mut line = String::new();
    rd.read_line(&mut line).await?;
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let path = parts.next().unwrap_or("").to_string();
    let (mut origin, mut host, mut marker, mut len) = (String::new(), String::new(), false, 0usize);
    let mut total = 0;
    loop {
        let mut h = String::new();
        let n = rd.read_line(&mut h).await?;
        total += n;
        if n == 0 || h == "\r\n" || h == "\n" || total > 32_768 {
            break;
        }
        if let Some((k, v)) = h.split_once(':') {
            let v = v.trim().to_string();
            match k.trim().to_ascii_lowercase().as_str() {
                "origin" => origin = v,
                "host" => host = v,
                "x-haul" => marker = v == "1",
                "content-length" => len = v.parse().unwrap_or(0),
                _ => {}
            }
        }
    }
    let host_ok = host.is_empty() || host == format!("127.0.0.1:{port}") || host == format!("localhost:{port}");
    if !host_ok || !origin_ok(&origin) {
        return respond(&mut wr, "403 Forbidden", &origin, "{}").await;
    }
    if method == "OPTIONS" {
        return respond(&mut wr, "204 No Content", &origin, "").await;
    }
    match (method.as_str(), path.as_str()) {
        ("GET", "/ping") => {
            let body = serde_json::json!({ "app": "haul", "version": env!("CARGO_PKG_VERSION") }).to_string();
            respond(&mut wr, "200 OK", &origin, &body).await
        }
        ("POST", "/add") if marker && len > 0 && len < 1_000_000 => {
            let mut body = vec![0u8; len];
            rd.read_exact(&mut body).await?;
            let req: AddRequest = match serde_json::from_slice(&body) {
                Ok(r) => r,
                Err(_) => return respond(&mut wr, "400 Bad Request", &origin, "{}").await,
            };
            let ok = crate::commands::incoming(req).await;
            let body = serde_json::json!({ "ok": ok.is_ok(), "error": ok.err().unwrap_or_default() }).to_string();
            respond(&mut wr, "200 OK", &origin, &body).await
        }
        _ => respond(&mut wr, "404 Not Found", &origin, "{}").await,
    }
}

async fn respond(wr: &mut tokio::net::tcp::OwnedWriteHalf, status: &str, origin: &str, body: &str) -> std::io::Result<()> {
    let mut out = format!(
        "HTTP/1.1 {status}\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\nCache-Control: no-store\r\n",
        body.len()
    );
    if !origin.is_empty() && origin_ok(origin) {
        out.push_str(&format!(
            "Access-Control-Allow-Origin: {origin}\r\nAccess-Control-Allow-Methods: GET, POST, OPTIONS\r\nAccess-Control-Allow-Headers: Content-Type, X-Haul\r\nAccess-Control-Allow-Private-Network: true\r\n"
        ));
    }
    out.push_str("\r\n");
    out.push_str(body);
    wr.write_all(out.as_bytes()).await?;
    wr.shutdown().await
}

#[cfg(test)]
mod tests {
    #[test]
    fn origins() {
        assert!(super::origin_ok(""));
        assert!(super::origin_ok("chrome-extension://abc"));
        assert!(!super::origin_ok("https://evil.example"));
        assert!(!super::origin_ok("null"));
    }
}
