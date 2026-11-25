//! Local-only HTTP bridge (`127.0.0.1:17890`) used by the danDown Browser Extension
//! to hand off intercepted downloads along with cookies, referrer, and User-Agent.

use std::collections::HashMap;

use log::{debug, warn};
use serde::{Deserialize, Serialize};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

use crate::manager::{DownloadOptions, DownloadsManager};

pub const BRIDGE_ADDR: &str = "127.0.0.1:17890";

#[derive(Debug, Deserialize)]
struct ExtensionCapturePayload {
    url: String,
    title: Option<String>,
    referrer: Option<String>,
    user_agent: Option<String>,
    cookie_header: Option<String>,
    headers: Option<HashMap<String, String>>,
    chunk_count: Option<i64>,
}

#[derive(Debug, Serialize)]
struct BridgeResponse {
    ok: bool,
    message: String,
}

pub fn spawn_browser_bridge() {
    tokio::spawn(async move {
        let listener = match TcpListener::bind(BRIDGE_ADDR).await {
            Ok(l) => {
                debug!("danDown browser bridge listening on http://{BRIDGE_ADDR}");
                l
            }
            Err(err) => {
                warn!("Could not bind browser bridge on {BRIDGE_ADDR}: {err}");
                return;
            }
        };

        loop {
            let Ok((stream, _)) = listener.accept().await else {
                continue;
            };
            tokio::spawn(handle_connection(stream));
        }
    });
}

async fn handle_connection(mut stream: TcpStream) {
    let mut buf = Vec::with_capacity(4096);
    let mut tmp = [0u8; 2048];
    let header_end = loop {
        match stream.read(&mut tmp).await {
            Ok(0) | Err(_) => return,
            Ok(n) => {
                buf.extend_from_slice(&tmp[..n]);
                if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                    break pos + 4;
                }
                if buf.len() > 64 * 1024 {
                    return;
                }
            }
        }
    };

    let head_str = String::from_utf8_lossy(&buf[..header_end]).into_owned();
    let mut lines = head_str.lines();
    let req_line = lines.next().unwrap_or("");
    let mut parts = req_line.split_whitespace();
    let method = parts.next().unwrap_or("");
    let path = parts.next().unwrap_or("");

    let content_length = lines
        .find_map(|l| {
            let (k, v) = l.split_once(':')?;
            if k.trim().eq_ignore_ascii_case("content-length") {
                v.trim().parse::<usize>().ok()
            } else {
                None
            }
        })
        .unwrap_or(0);

    if content_length > 256 * 1024 {
        let _ = write_json(&mut stream, 413, false, "Payload too large").await;
        return;
    }

    while buf.len() < header_end + content_length {
        match stream.read(&mut tmp).await {
            Ok(0) | Err(_) => break,
            Ok(n) => buf.extend_from_slice(&tmp[..n]),
        }
    }

    if method == "OPTIONS" {
        let _ = write_cors_preflight(&mut stream).await;
        return;
    }

    match (method, path) {
        ("GET", "/ping") => {
            let _ = write_json(&mut stream, 200, true, "danDown bridge online").await;
        }
        ("POST", "/download") => {
            let body = &buf[header_end..buf.len().min(header_end + content_length)];
            match serde_json::from_slice::<ExtensionCapturePayload>(body) {
                Ok(payload) => {
                    let clean_title = payload
                        .title
                        .as_deref()
                        .map(crate::client::sanitize_file_name)
                        .filter(|s| !s.is_empty());
                    let mut headers = payload.headers.unwrap_or_default();
                    if let Some(referer) = payload.referrer.filter(|s| !s.is_empty()) {
                        headers.entry("Referer".to_string()).or_insert(referer);
                    }
                    if let Some(ua) = payload.user_agent.filter(|s| !s.is_empty()) {
                        headers.entry("User-Agent".to_string()).or_insert(ua);
                    }
                    if let Some(cookie) = payload.cookie_header.filter(|s| !s.is_empty()) {
                        headers.entry("Cookie".to_string()).or_insert(cookie);
                    }

                    let options = DownloadOptions {
                        file_path: None,
                        file_name: clean_title,
                        chunk_count: payload.chunk_count.unwrap_or(8),
                        proxy: None,
                        auth: None,
                        headers: if headers.is_empty() {
                            None
                        } else {
                            Some(headers)
                        },
                        cookies: None,
                        speed_limit: None,
                        max_retries: Some(5),
                        delay_secs: None,
                        backoff_factor: Some(2.0),
                        timeout_secs: Some(30.0),
                        start_at_unix_ms: None,
                        engine: None,
                        aria2_path: None,
                    };

                    match DownloadsManager::add_new_download(payload.url, options).await {
                        Ok(()) => {
                            let _ = write_json(&mut stream, 200, true, "Download added to danDown")
                                .await;
                        }
                        Err(err) => {
                            let _ = write_json(&mut stream, 400, false, &err).await;
                        }
                    }
                }
                Err(err) => {
                    let _ =
                        write_json(&mut stream, 400, false, &format!("Invalid JSON: {err}")).await;
                }
            }
        }
        _ => {
            let _ = write_json(&mut stream, 404, false, "Not found").await;
        }
    }
}

async fn write_cors_preflight(stream: &mut TcpStream) -> std::io::Result<()> {
    let resp = concat!(
        "HTTP/1.1 204 No Content\r\n",
        "Access-Control-Allow-Origin: *\r\n",
        "Access-Control-Allow-Methods: GET, POST, OPTIONS\r\n",
        "Access-Control-Allow-Headers: Content-Type\r\n",
        "Content-Length: 0\r\n",
        "Connection: close\r\n\r\n"
    );
    stream.write_all(resp.as_bytes()).await
}

async fn write_json(
    stream: &mut TcpStream,
    status: u16,
    ok: bool,
    message: &str,
) -> std::io::Result<()> {
    let status_text = match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        413 => "Payload Too Large",
        _ => "Error",
    };
    let body = serde_json::to_vec(&BridgeResponse {
        ok,
        message: message.to_string(),
    })
    .unwrap_or_default();

    let head = format!(
        "HTTP/1.1 {status} {status_text}\r\nContent-Type: application/json\r\nAccess-Control-Allow-Origin: *\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes()).await?;
    stream.write_all(&body).await
}
