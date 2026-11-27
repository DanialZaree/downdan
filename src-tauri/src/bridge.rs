//! Local-only HTTP bridge (`127.0.0.1:17890`) used by the danDown Browser Extension
//! to probe URLs, hand off intercepted downloads, query live download & chunk progress,
//! and control pause/resume/cancel/open actions directly from the IDM-style popup.

use std::{
    collections::HashMap,
    sync::{atomic::Ordering, RwLock},
    time::Duration,
};

use log::{debug, warn};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

use crate::{
    aria2, dispatch,
    manager::{DownloadOptions, DownloadsManager},
    registry::Registry,
    repository::{chunk::ChunkRepository, download::DownloadRepository},
};

pub const BRIDGE_ADDR: &str = "127.0.0.1:17890";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AppDefaultsPayload {
    default_download_dir: Option<String>,
    chunk_count: i64,
}

static APP_DEFAULTS: Lazy<RwLock<AppDefaultsPayload>> = Lazy::new(|| {
    RwLock::new(AppDefaultsPayload {
        default_download_dir: None,
        chunk_count: 8,
    })
});

#[derive(Debug, Deserialize)]
struct ExtensionCapturePayload {
    url: String,
    title: Option<String>,
    file_path: Option<String>,
    referrer: Option<String>,
    user_agent: Option<String>,
    cookie_header: Option<String>,
    headers: Option<HashMap<String, String>>,
    chunk_count: Option<i64>,
    start_paused: Option<bool>,
}

#[derive(Debug, Serialize)]
struct BridgeResponse {
    ok: bool,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    download_id: Option<i64>,
}

#[derive(Debug, Serialize)]
struct ProbeResponse {
    ok: bool,
    content_length: Option<u64>,
    content_type: Option<String>,
    file_name: Option<String>,
    supports_range: bool,
}

#[derive(Debug, Serialize)]
struct PickFolderResponse {
    ok: bool,
    path: Option<String>,
}

#[derive(Debug, Serialize)]
struct ChunkStatusItem {
    index: i64,
    downloaded_bytes: u64,
    total_bytes: u64,
    status: String,
}

#[derive(Debug, Serialize)]
struct DownloadStatusResponse {
    ok: bool,
    download_id: i64,
    file_name: String,
    file_path: String,
    url: String,
    content_type: String,
    status: String,
    downloaded_bytes: u64,
    total_bytes: u64,
    speed_bps: u64,
    disk_speed_bps: u64,
    eta_secs: u64,
    supports_range: bool,
    chunk_count: i64,
    chunks: Vec<ChunkStatusItem>,
    error_message: Option<String>,
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

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Ok(hi), Ok(lo)) = (
                u8::from_str_radix(&input[i + 1..i + 2], 16),
                u8::from_str_radix(&input[i + 2..i + 3], 16),
            ) {
                out.push((hi << 4) | lo);
                i += 3;
                continue;
            }
        } else if bytes[i] == b'+' {
            out.push(b' ');
            i += 1;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn parse_query(raw_path: &str) -> (&str, HashMap<String, String>) {
    let mut map = HashMap::new();
    let Some((path, query)) = raw_path.split_once('?') else {
        return (raw_path, map);
    };
    for pair in query.split('&') {
        if let Some((k, v)) = pair.split_once('=') {
            map.insert(percent_decode(k), percent_decode(v));
        }
    }
    (path, map)
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
    let raw_path = parts.next().unwrap_or("");
    let (path, query) = parse_query(raw_path);

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
        let _ = write_json(&mut stream, 413, false, "Payload too large", None).await;
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
            let _ = write_json(&mut stream, 200, true, "danDown bridge online", None).await;
        }
        ("GET", "/defaults") => {
            let payload = APP_DEFAULTS
                .read()
                .map(|g| g.clone())
                .unwrap_or(AppDefaultsPayload {
                    default_download_dir: None,
                    chunk_count: 8,
                });
            let _ = write_raw_json(&mut stream, 200, &payload).await;
        }
        ("POST", "/defaults") => {
            let body = &buf[header_end..buf.len().min(header_end + content_length)];
            if let Ok(parsed) = serde_json::from_slice::<AppDefaultsPayload>(body) {
                if let Ok(mut guard) = APP_DEFAULTS.write() {
                    guard.default_download_dir = parsed
                        .default_download_dir
                        .as_deref()
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(String::from);
                    guard.chunk_count = parsed.chunk_count.clamp(1, 16);
                }
                let _ = write_json(&mut stream, 200, true, "Defaults synced", None).await;
            } else {
                let _ = write_json(&mut stream, 400, false, "Invalid defaults payload", None).await;
            }
        }
        ("GET", "/probe") => {
            let Some(url) = query.get("url").cloned().filter(|u| !u.is_empty()) else {
                let _ = write_json(&mut stream, 400, false, "Missing url parameter", None).await;
                return;
            };
            handle_probe(&mut stream, &url).await;
        }
        ("POST", "/show") => {
            use tauri::Manager;
            if let Some(win) = Registry::get_state().app_handle.get_webview_window("main") {
                let _ = win.show();
                let _ = win.unminimize();
                let _ = win.set_focus();
            }
            let _ = write_json(&mut stream, 200, true, "danDown window shown", None).await;
        }
        ("POST", "/pick-folder") => {
            let dir = query.get("dir").cloned();
            handle_pick_folder(&mut stream, dir).await;
        }
        ("GET", "/status") => {
            let Some(id) = query.get("id").and_then(|v| v.parse::<i64>().ok()) else {
                let _ = write_json(&mut stream, 400, false, "Missing id parameter", None).await;
                return;
            };
            handle_status(&mut stream, id).await;
        }
        ("POST", "/pause") => {
            let Some(id) = query.get("id").and_then(|v| v.parse::<i64>().ok()) else {
                let _ = write_json(&mut stream, 400, false, "Missing id parameter", None).await;
                return;
            };
            let _ = dispatch!(registry, PauseDownload, (id));
            let _ = write_json(&mut stream, 200, true, "Download paused", Some(id)).await;
        }
        ("POST", "/resume") => {
            let Some(id) = query.get("id").and_then(|v| v.parse::<i64>().ok()) else {
                let _ = write_json(&mut stream, 400, false, "Missing id parameter", None).await;
                return;
            };
            let _ = dispatch!(registry, ResumeDownload, (id));
            let _ = write_json(&mut stream, 200, true, "Download resumed", Some(id)).await;
        }
        ("POST", "/cancel") => {
            let Some(id) = query.get("id").and_then(|v| v.parse::<i64>().ok()) else {
                let _ = write_json(&mut stream, 400, false, "Missing id parameter", None).await;
                return;
            };
            let _ = dispatch!(registry, RemoveDownload, (id, true));
            let _ = write_json(&mut stream, 200, true, "Download cancelled", Some(id)).await;
        }
        ("POST", "/open") => {
            let Some(id) = query.get("id").and_then(|v| v.parse::<i64>().ok()) else {
                let _ = write_json(&mut stream, 400, false, "Missing id parameter", None).await;
                return;
            };
            let mode = query.get("mode").map(String::as_str).unwrap_or("file");
            handle_open(&mut stream, id, mode).await;
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
                    let clean_dir = payload
                        .file_path
                        .as_deref()
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(String::from)
                        .or_else(|| {
                            APP_DEFAULTS
                                .read()
                                .ok()
                                .and_then(|d| d.default_download_dir.clone())
                        });
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

                    let start_paused = payload.start_paused.unwrap_or(false);
                    let options = DownloadOptions {
                        file_path: clean_dir,
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

                    match DownloadsManager::add_new_download_returning_id(payload.url, options)
                        .await
                    {
                        Ok(download_id) => {
                            if start_paused {
                                let _ = dispatch!(registry, PauseDownload, (download_id));
                            }
                            let _ = write_json(
                                &mut stream,
                                200,
                                true,
                                if start_paused {
                                    "Download queued for later"
                                } else {
                                    "Download added to danDown"
                                },
                                Some(download_id),
                            )
                            .await;
                        }
                        Err(err) => {
                            let _ = write_json(&mut stream, 400, false, &err, None).await;
                        }
                    }
                }
                Err(err) => {
                    let _ = write_json(
                        &mut stream,
                        400,
                        false,
                        &format!("Invalid JSON: {err}"),
                        None,
                    )
                    .await;
                }
            }
        }
        _ => {
            let _ = write_json(&mut stream, 404, false, "Not found", None).await;
        }
    }
}

async fn handle_probe(stream: &mut TcpStream, url: &str) {
    let client = match tauri_plugin_http::reqwest::Client::builder()
        .timeout(Duration::from_secs(6))
        .build()
    {
        Ok(c) => c,
        Err(_) => {
            let _ = write_json(stream, 500, false, "Probe client failed", None).await;
            return;
        }
    };

    let resp = match client.head(url).send().await {
        Ok(r) if r.status().is_success() => r,
        _ => {
            // Fallback to ranged GET 0-0 for servers rejecting HEAD
            match client.get(url).header("Range", "bytes=0-0").send().await {
                Ok(r) if r.status().is_success() => r,
                _ => {
                    let _ = write_raw_json(
                        stream,
                        200,
                        &ProbeResponse {
                            ok: true,
                            content_length: None,
                            content_type: None,
                            file_name: None,
                            supports_range: false,
                        },
                    )
                    .await;
                    return;
                }
            }
        }
    };

    let headers = resp.headers();
    let content_type = headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.split(';').next().unwrap_or(s).trim().to_string())
        .filter(|s| !s.is_empty());

    let content_length = headers
        .get("content-range")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.rsplit('/').next())
        .and_then(|n| n.parse::<u64>().ok())
        .or_else(|| {
            headers
                .get("content-length")
                .and_then(|v| v.to_str().ok())
                .and_then(|n| n.parse::<u64>().ok())
        })
        .filter(|n| *n > 0);

    let supports_range = headers
        .get("accept-ranges")
        .and_then(|v| v.to_str().ok())
        .map(|v| v.to_ascii_lowercase().contains("bytes"))
        .unwrap_or(false)
        || headers.contains_key("content-range");

    let file_name = headers
        .get("content-disposition")
        .and_then(|v| v.to_str().ok())
        .and_then(|cd| {
            cd.split(';')
                .find_map(|part| {
                    let trimmed = part.trim();
                    trimmed
                        .strip_prefix("filename=")
                        .map(|v| v.trim_matches('"').to_string())
                })
        })
        .filter(|s| !s.is_empty());

    let _ = write_raw_json(
        stream,
        200,
        &ProbeResponse {
            ok: true,
            content_length,
            content_type,
            file_name,
            supports_range,
        },
    )
    .await;
}

async fn handle_status(stream: &mut TcpStream, download_id: i64) {
    let Ok(download) = DownloadRepository::find(download_id).await else {
        let _ = write_json(stream, 404, false, "Download not found", None).await;
        return;
    };

    let mut total_bytes = download.total_bytes.max(0) as u64;
    let mut downloaded_bytes: u64 = 0;
    let mut speed_bps: u64 = 0;
    let mut disk_speed_bps: u64 = 0;
    let mut eta_secs: u64 = 0;
    let mut chunks_out: Vec<ChunkStatusItem> = Vec::new();

    if let Some(live) = aria2::get_live_progress(download_id) {
        downloaded_bytes = live.downloaded_bytes;
        if live.total_bytes > 0 {
            total_bytes = live.total_bytes;
        }
        speed_bps = live.speed_bps;
        disk_speed_bps = live.speed_bps;
        eta_secs = live.eta_secs;
    } else if let Some(report) = Registry::get_state().reports.get(&download_id) {
        downloaded_bytes = report.total_downloaded_bytes.load(Ordering::Relaxed);
        if report.total_bytes > 0 {
            total_bytes = report.total_bytes;
        }
        speed_bps = report.speed_bps.load(Ordering::Relaxed);
        let wrote_hist = report.wrote_history.lock().await;
        if !wrote_hist.is_empty() {
            disk_speed_bps = wrote_hist.iter().sum::<u64>() / wrote_hist.len() as u64;
        } else {
            disk_speed_bps = speed_bps;
        }
        if speed_bps > 0 && total_bytes > downloaded_bytes {
            eta_secs = (total_bytes - downloaded_bytes) / speed_bps;
        }
        if let Ok(db_chunks) = ChunkRepository::find_all(download_id).await {
            for c in db_chunks {
                let len = if c.end_byte >= c.start_byte {
                    (c.end_byte - c.start_byte + 1) as u64
                } else {
                    0
                };
                let done_b = report
                    .chunks_wrote_bytes
                    .get(&c.chunk_index)
                    .map(|v| v.load(Ordering::Relaxed))
                    .unwrap_or_else(|| c.downloaded_bytes.max(0) as u64);
                let st = if len > 0 && done_b >= len {
                    "Done"
                } else if download.status == "paused" {
                    "Paused"
                } else if download.status == "downloading" {
                    if done_b > 0 {
                        "Receiving"
                    } else {
                        "Connecting"
                    }
                } else if download.status == "failed" {
                    "Disconnected"
                } else {
                    "Waiting"
                };
                chunks_out.push(ChunkStatusItem {
                    index: c.chunk_index + 1,
                    downloaded_bytes: done_b,
                    total_bytes: len,
                    status: st.to_string(),
                });
            }
        }
    } else if download.status == "completed" {
        downloaded_bytes = total_bytes;
    } else if let Ok(chunks) = ChunkRepository::find_all(download_id).await {
        downloaded_bytes = chunks
            .iter()
            .map(|c| c.downloaded_bytes.max(0) as u64)
            .sum();
    }

    // For aria2 or completed/paused downloads without per-chunk DB rows, synthesize
    // N balanced connection segments matching download.chunk_count so the segmented
    // bar & details table stay accurate and smooth.
    let count = download.chunk_count.clamp(1, 16) as usize;
    if chunks_out.len() <= 1 && count >= 1 {
        chunks_out.clear();
        let seg_total = if total_bytes > 0 {
            total_bytes / count as u64
        } else {
            0
        };
        let seg_done_base = downloaded_bytes / count as u64;
        let seg_done_rem = downloaded_bytes % count as u64;
        for i in 0..count {
            let st_total = if i == count - 1 && total_bytes > 0 {
                total_bytes - seg_total * (count as u64 - 1)
            } else {
                seg_total
            };
            let st_done = if download.status == "completed" {
                st_total
            } else {
                (seg_done_base + if (i as u64) < seg_done_rem { 1 } else { 0 })
                    .min(if st_total > 0 { st_total } else { u64::MAX })
            };
            let st_label = if download.status == "completed" || (st_total > 0 && st_done >= st_total)
            {
                "Done"
            } else if download.status == "paused" {
                "Paused"
            } else if download.status == "failed" {
                "Disconnected"
            } else if download.status == "downloading" {
                if speed_bps > 0 || st_done > 0 {
                    "Receiving"
                } else {
                    "Connecting"
                }
            } else {
                "Connecting"
            };
            chunks_out.push(ChunkStatusItem {
                index: (i + 1) as i64,
                downloaded_bytes: st_done,
                total_bytes: st_total,
                status: st_label.to_string(),
            });
        }
    }

    let payload = DownloadStatusResponse {
        ok: true,
        download_id,
        file_name: download.file_name,
        file_path: download.file_path,
        url: download.url,
        content_type: download.content_type,
        status: download.status,
        downloaded_bytes,
        total_bytes,
        speed_bps,
        disk_speed_bps,
        eta_secs,
        supports_range: download.supports_range,
        chunk_count: download.chunk_count,
        chunks: chunks_out,
        error_message: download.error_message.filter(|s| !s.is_empty()),
    };

    let _ = write_raw_json(stream, 200, &payload).await;
}

async fn handle_open(stream: &mut TcpStream, download_id: i64, mode: &str) {
    let Ok(download) = DownloadRepository::find(download_id).await else {
        let _ = write_json(stream, 404, false, "Download not found", None).await;
        return;
    };

    let path = download.file_path;
    let result = if mode == "folder" {
        #[cfg(target_os = "windows")]
        {
            std::process::Command::new("explorer")
                .arg(format!("/select,{}", path))
                .spawn()
        }
        #[cfg(target_os = "macos")]
        {
            std::process::Command::new("open")
                .args(["-R", &path])
                .spawn()
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            let parent = std::path::Path::new(&path)
                .parent()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.clone());
            std::process::Command::new("xdg-open").arg(parent).spawn()
        }
    } else {
        #[cfg(target_os = "windows")]
        {
            std::process::Command::new("cmd")
                .args(["/C", "start", "", &path])
                .spawn()
        }
        #[cfg(target_os = "macos")]
        {
            std::process::Command::new("open").arg(&path).spawn()
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            std::process::Command::new("xdg-open").arg(&path).spawn()
        }
    };

    match result {
        Ok(_) => {
            let _ = write_json(stream, 200, true, "Opened", Some(download_id)).await;
        }
        Err(err) => {
            let _ = write_json(stream, 500, false, &err.to_string(), None).await;
        }
    }
}

async fn handle_pick_folder(stream: &mut TcpStream, initial_dir: Option<String>) {
    use tauri_plugin_dialog::DialogExt;
    let app = Registry::get_state().app_handle.clone();
    let app_for_dialog = app.clone();
    let (tx, rx) = tokio::sync::oneshot::channel();
    let run_res = app.run_on_main_thread(move || {
        let mut builder = app_for_dialog
            .dialog()
            .file()
            .set_title("Select Download Folder");
        if let Some(dir) = initial_dir.filter(|s| !s.trim().is_empty()) {
            let clean = dir.trim().trim_start_matches(r"\\?\");
            let p = std::path::PathBuf::from(clean);
            if p.is_dir() {
                builder = builder.set_directory(p);
            }
        }
        builder.pick_folder(move |picked| {
            let path_str = picked
                .and_then(|fp| fp.into_path().ok())
                .map(|p| {
                    p.to_string_lossy()
                        .trim_start_matches(r"\\?\")
                        .to_string()
                });
            let _ = tx.send(path_str);
        });
    });
    if let Err(err) = run_res {
        let _ = write_json(stream, 500, false, &err.to_string(), None).await;
        return;
    }
    let path = rx.await.ok().flatten();
    let _ = write_raw_json(stream, 200, &PickFolderResponse { ok: true, path }).await;
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
    download_id: Option<i64>,
) -> std::io::Result<()> {
    write_raw_json(
        stream,
        status,
        &BridgeResponse {
            ok,
            message: message.to_string(),
            download_id,
        },
    )
    .await
}

async fn write_raw_json<T: Serialize>(
    stream: &mut TcpStream,
    status: u16,
    value: &T,
) -> std::io::Result<()> {
    let status_text = match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        413 => "Payload Too Large",
        _ => "Error",
    };
    let body = serde_json::to_vec(value).unwrap_or_default();

    let head = format!(
        "HTTP/1.1 {status} {status_text}\r\nContent-Type: application/json\r\nAccess-Control-Allow-Origin: *\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes()).await?;
    stream.write_all(&body).await
}
