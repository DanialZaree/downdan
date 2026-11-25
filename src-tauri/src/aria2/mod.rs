//! System-installed `aria2c` adapter for danDown's SQLite-backed download manager.
//! Reuses `crate::engine::rpc::Aria2Client` for all JSON-RPC 2.0 communication.

use std::{
    path::{Path, PathBuf},
    process::Stdio,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

use dashmap::DashMap;
use log::{debug, warn};
use once_cell::sync::Lazy;
use serde::Serialize;
use tokio::{
    process::{Child, Command},
    sync::Mutex,
    time::sleep,
};

use crate::{
    client::ProxyType,
    emitter::Emitter,
    engine::rpc::{Aria2Client, DownloadOptions as RpcDownloadOptions, DownloadTask},
    file::File,
    manager::DownloadOptions,
    models::{UpdateChunk, UpdateDownload},
    repository::{chunk::ChunkRepository, download::DownloadRepository, engine::EngineRepository},
};

const ARIA2_RPC_PORT: u16 = 16800;

#[derive(Debug, Clone, Serialize)]
pub struct Aria2StatusInfo {
    pub available: bool,
    pub binary_path: Option<String>,
    pub version: Option<String>,
    pub rpc_running: bool,
    pub message: String,
}

#[derive(Clone, Serialize)]
struct SpeedAndRemaining {
    speed: u64,
    remaining_time: u64,
}

struct Aria2Runtime {
    child: Option<Child>,
    client: Aria2Client,
}

static RUNTIME: Lazy<Mutex<Option<Aria2Runtime>>> = Lazy::new(|| Mutex::new(None));
static SIDECAR_CLIENT: Lazy<Mutex<Option<Aria2Client>>> = Lazy::new(|| Mutex::new(None));
static ACTIVE_GIDS: Lazy<DashMap<i64, String>> = Lazy::new(DashMap::new);
static KNOWN_TOTALS: Lazy<DashMap<i64, u64>> = Lazy::new(DashMap::new);
static POLLER_RUNNING: AtomicBool = AtomicBool::new(false);

/// Registers the bundled `aria2c` sidecar's `Aria2Client` so all downloads reuse
/// the same daemon without spawning a second `aria2c` process.
pub async fn register_sidecar_client(client: Aria2Client) {
    *SIDECAR_CLIENT.lock().await = Some(client);
}

pub async fn is_ready_or_available(custom_path: Option<&str>) -> bool {
    if SIDECAR_CLIENT.lock().await.is_some() {
        return true;
    }
    find_aria2_binary(custom_path).is_some()
}

/// Finds the `aria2c` executable from an optional user override, adjacent to the
/// danDown binary, system `PATH`, or common OS install directories.
pub fn find_aria2_binary(custom_path: Option<&str>) -> Option<PathBuf> {
    if let Some(custom) = custom_path.map(str::trim).filter(|s| !s.is_empty()) {
        let p = PathBuf::from(custom);
        if p.is_file() {
            return Some(p);
        }
    }

    let bin_name = if cfg!(windows) { "aria2c.exe" } else { "aria2c" };

    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let adjacent = dir.join(bin_name);
            if adjacent.is_file() {
                return Some(adjacent);
            }
        }
    }

    if let Ok(path_var) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path_var) {
            let candidate = dir.join(bin_name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }

    common_install_candidates()
        .into_iter()
        .find(|candidate| candidate.is_file())
}

fn common_install_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    #[cfg(target_os = "windows")]
    {
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            candidates.push(PathBuf::from(&local).join("Microsoft/WinGet/Links/aria2c.exe"));
            candidates.push(PathBuf::from(&local).join("Programs/aria2/aria2c.exe"));
        }
        if let Ok(profile) = std::env::var("USERPROFILE") {
            candidates.push(PathBuf::from(&profile).join("scoop/shims/aria2c.exe"));
        }
        candidates.push(PathBuf::from(r"C:\ProgramData\chocolatey\bin\aria2c.exe"));
        candidates.push(PathBuf::from(r"C:\Program Files\aria2\aria2c.exe"));
    }
    #[cfg(not(target_os = "windows"))]
    {
        candidates.push(PathBuf::from("/opt/homebrew/bin/aria2c"));
        candidates.push(PathBuf::from("/usr/local/bin/aria2c"));
        candidates.push(PathBuf::from("/usr/bin/aria2c"));
    }
    candidates
}

async fn read_binary_version(binary: &Path) -> Option<String> {
    let output = Command::new(binary).arg("--version").output().await.ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .next()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
}

pub async fn check_status(custom_path: Option<String>) -> Aria2StatusInfo {
    let Some(binary) = find_aria2_binary(custom_path.as_deref()) else {
        return Aria2StatusInfo {
            available: false,
            binary_path: None,
            version: None,
            rpc_running: false,
            message: "aria2c executable not found. Install aria2 or set a custom aria2c path."
                .to_string(),
        };
    };

    let version = read_binary_version(&binary).await;
    let rpc_running = {
        let guard = RUNTIME.lock().await;
        match guard.as_ref() {
            Some(rt) => rt.client.get_version().await.is_ok(),
            None => false,
        }
    };

    Aria2StatusInfo {
        available: true,
        binary_path: Some(binary.to_string_lossy().into_owned()),
        version,
        rpc_running,
        message: if rpc_running {
            "aria2c found and JSON-RPC daemon is active".to_string()
        } else {
            "aria2c found (daemon starts automatically on first aria2 download)".to_string()
        },
    }
}

async fn ensure_daemon(custom_path: Option<&str>) -> Result<Aria2Client, String> {
    if let Some(sidecar) = SIDECAR_CLIENT.lock().await.clone() {
        if sidecar.get_version().await.is_ok() {
            return Ok(sidecar);
        }
    }

    let mut guard = RUNTIME.lock().await;

    if let Some(rt) = guard.as_mut() {
        let alive = match rt.child.as_mut() {
            Some(child) => matches!(child.try_wait(), Ok(None)),
            None => false,
        };
        if alive {
            return Ok(rt.client.clone());
        }
        *guard = None;
    }

    let binary = find_aria2_binary(custom_path).ok_or_else(|| {
        "aria2c executable not found. Install aria2 (e.g. `winget install aria2.aria2` / `brew install aria2`) or configure its path in Settings."
            .to_string()
    })?;

    let secret = format!("danDown_{:016x}", fastrand::u64(..));
    let mut cmd = Command::new(&binary);
    cmd.args([
        "--enable-rpc=true",
        "--rpc-listen-all=false",
        &format!("--rpc-listen-port={ARIA2_RPC_PORT}"),
        &format!("--rpc-secret={secret}"),
        "--continue=true",
        "--auto-file-renaming=true",
        "--allow-overwrite=false",
        "--file-allocation=none",
        "--max-tries=5",
        "--retry-wait=2",
        "--quiet=true",
    ])
    .stdin(Stdio::null())
    .stdout(Stdio::null())
    .stderr(Stdio::null());

    #[cfg(target_os = "windows")]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let child = cmd
        .spawn()
        .map_err(|e| format!("Failed to start aria2c ({}): {e}", binary.display()))?;

    debug!(
        "Spawned aria2c RPC daemon from {} on port {}",
        binary.display(),
        ARIA2_RPC_PORT
    );

    let client = Aria2Client::new(ARIA2_RPC_PORT, secret).map_err(|e| e.to_string())?;
    *guard = Some(Aria2Runtime {
        child: Some(child),
        client: client.clone(),
    });
    drop(guard);

    for _ in 0..25 {
        if client.get_version().await.is_ok() {
            return Ok(client);
        }
        sleep(Duration::from_millis(80)).await;
    }

    Err("Timed out waiting for aria2c JSON-RPC daemon to start".to_string())
}

fn format_proxy_url(
    scheme: &str,
    host: &str,
    port: u16,
    username: &Option<String>,
    password: &Option<String>,
) -> String {
    let auth = match (username, password) {
        (Some(u), Some(p)) if !u.is_empty() => format!("{u}:{p}@"),
        (Some(u), None) if !u.is_empty() => format!("{u}@"),
        _ => String::new(),
    };
    format!("{scheme}://{auth}{host}:{port}")
}

pub fn to_rpc_options(
    dir: &str,
    out_file: Option<&str>,
    options: &DownloadOptions,
) -> RpcDownloadOptions {
    let mut header_lines = Vec::new();
    if let Some(headers) = &options.headers {
        for (k, v) in headers {
            header_lines.push(format!("{k}: {v}"));
        }
    }
    if let Some(cookies) = &options.cookies {
        let cookie_str = cookies
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join("; ");
        if !cookie_str.is_empty() {
            header_lines.push(format!("Cookie: {cookie_str}"));
        }
    }

    let mut extra = serde_json::Map::new();
    extra.insert("min-split-size".into(), "1M".into());
    extra.insert("continue".into(), "true".into());
    if let Some(limit) = options.speed_limit.filter(|l| *l > 0) {
        extra.insert("max-download-limit".into(), limit.to_string().into());
    }
    if let Some(retries) = options.max_retries {
        extra.insert("max-tries".into(), retries.max(0).to_string().into());
    }
    if let Some(timeout) = options.timeout_secs {
        extra.insert("timeout".into(), (timeout.max(5.0) as i64).to_string().into());
    }
    if let Some(proxy) = &options.proxy {
        match proxy {
            ProxyType::Http {
                host,
                port,
                username,
                password,
            } => {
                extra.insert(
                    "all-proxy".into(),
                    format_proxy_url("http", host, *port, username, password).into(),
                );
            }
            ProxyType::Https {
                host,
                port,
                username,
                password,
            } => {
                extra.insert(
                    "all-proxy".into(),
                    format_proxy_url("https", host, *port, username, password).into(),
                );
            }
            ProxyType::Socks5 { .. } | ProxyType::System => {}
        }
    }

    let conns = options.chunk_count.clamp(1, 16) as u32;
    RpcDownloadOptions {
        dir: Some(dir.to_string()),
        out: out_file.filter(|s| !s.is_empty()).map(str::to_string),
        header: (!header_lines.is_empty()).then_some(header_lines),
        split: Some(conns),
        max_connection_per_server: Some(conns),
        extra,
    }
}

pub fn infer_name_from_uri(url: &str) -> (String, String) {
    if url.to_ascii_lowercase().starts_with("magnet:") {
        let dn = url
            .split('&')
            .find_map(|part| part.trim_start_matches("magnet:?").strip_prefix("dn="))
            .map(|raw| raw.replace('+', " "))
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "magnet-download".to_string());
        return (dn, "torrent".to_string());
    }

    let path_name = url
        .split('?')
        .next()
        .unwrap_or(url)
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or("download.bin")
        .to_string();

    let ext = Path::new(&path_name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("bin")
        .to_ascii_lowercase();

    (path_name, ext)
}

pub async fn start_download(download_id: i64, options: &DownloadOptions) -> Result<(), String> {
    let download = DownloadRepository::find(download_id)
        .await
        .map_err(|e| e.to_string())?;

    let client = ensure_daemon(options.aria2_path.as_deref()).await?;

    let dir = Path::new(&download.file_path)
        .parent()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| ".".to_string());

    let out_name = (!download.url.to_ascii_lowercase().starts_with("magnet:"))
        .then_some(download.file_name.as_str());

    let rpc_opts = to_rpc_options(&dir, out_name, options);
    let gid = client
        .add_uri(vec![download.url], Some(rpc_opts))
        .await
        .map_err(|e| e.to_string())?;

    EngineRepository::update_gid(download_id, &gid)
        .await
        .map_err(|e| e.to_string())?;
    ACTIVE_GIDS.insert(download_id, gid);

    set_download_status(download_id, "downloading", None).await?;
    ensure_poller();
    Ok(())
}

pub async fn pause_download(download_id: i64) -> Result<(), String> {
    if let Some(gid) = resolve_gid(download_id).await {
        if let Some(client) = active_client().await {
            let _ = client.force_pause(gid).await;
        }
    }

    ACTIVE_GIDS.remove(&download_id);
    set_download_status(download_id, "paused", None).await
}

pub async fn resume_download(download_id: i64) -> Result<(), String> {
    let download = DownloadRepository::find(download_id)
        .await
        .map_err(|e| e.to_string())?;

    let existing_gid = resolve_gid(download_id).await;
    let client = ensure_daemon(None).await?;

    if let Some(gid) = existing_gid {
        if client.unpause(gid.clone()).await.is_ok() {
            ACTIVE_GIDS.insert(download_id, gid);
            set_download_status(download_id, "downloading", Some(String::new())).await?;
            ensure_poller();
            return Ok(());
        }
    }

    let options = DownloadOptions {
        file_path: Path::new(&download.file_path)
            .parent()
            .map(|p| p.to_string_lossy().into_owned()),
        file_name: None,
        chunk_count: download.chunk_count,
        proxy: download.proxy,
        auth: download.auth,
        headers: download.headers,
        cookies: download.cookies,
        speed_limit: download.speed_limit,
        max_retries: Some(download.max_retries),
        delay_secs: Some(download.delay_secs),
        backoff_factor: Some(download.backoff_factor),
        timeout_secs: Some(download.timeout_secs as f64),
        start_at_unix_ms: None,
        engine: None,
        aria2_path: None,
    };

    start_download(download_id, &options).await
}

pub async fn remove_download(download_id: i64, remove_file: bool) -> Result<(), String> {
    if let Some((_, gid)) = ACTIVE_GIDS.remove(&download_id) {
        if let Some(client) = active_client().await {
            let _ = client.force_remove(gid.clone()).await;
            let _ = client.remove_download_result(gid).await;
        }
    }

    let file_path = DownloadRepository::delete(download_id)
        .await
        .map_err(|e| e.to_string())?;
    if remove_file {
        let _ = File::remove_file(&file_path);
    }
    Ok(())
}

pub async fn shutdown() {
    let mut guard = RUNTIME.lock().await;
    if let Some(mut rt) = guard.take() {
        let _ = rt.client.shutdown().await;
        if let Some(mut child) = rt.child.take() {
            let _ = child.kill();
        }
    }
}

async fn resolve_gid(download_id: i64) -> Option<String> {
    if let Some(g) = ACTIVE_GIDS.get(&download_id) {
        return Some(g.clone());
    }
    EngineRepository::get(download_id)
        .await
        .ok()
        .flatten()
        .and_then(|(_, g)| g)
}

async fn active_client() -> Option<Aria2Client> {
    if let Some(sidecar) = SIDECAR_CLIENT.lock().await.clone() {
        return Some(sidecar);
    }
    RUNTIME.lock().await.as_ref().map(|rt| rt.client.clone())
}

async fn set_download_status(
    download_id: i64,
    status: &str,
    error_message: Option<String>,
) -> Result<(), String> {
    DownloadRepository::update(
        download_id,
        UpdateDownload {
            status: Some(status.to_string()),
            error_message,
            ..Default::default()
        },
    )
    .await
    .map_err(|e| e.to_string())?;

    if let Ok(updated) = DownloadRepository::find(download_id).await {
        Emitter::emit_event("download_item", &updated);
    }
    Ok(())
}

fn ensure_poller() {
    if POLLER_RUNNING
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return;
    }

    tokio::spawn(async move {
        loop {
            if ACTIVE_GIDS.is_empty() {
                POLLER_RUNNING.store(false, Ordering::SeqCst);
                break;
            }

            if let Some(client) = active_client().await {
                let entries: Vec<(i64, String)> = ACTIVE_GIDS
                    .iter()
                    .map(|e| (*e.key(), e.value().clone()))
                    .collect();

                for (download_id, gid) in entries {
                    if let Err(err) = poll_single(&client, download_id, &gid).await {
                        warn!("aria2 poll failed for download {download_id}: {err}");
                    }
                }
            }

            sleep(Duration::from_millis(500)).await;
        }
    });
}

async fn poll_single(client: &Aria2Client, download_id: i64, gid: &str) -> Result<(), String> {
    let task: DownloadTask = client
        .tell_status(gid.to_string())
        .await
        .map_err(|e| e.to_string())?;

    let speed_kb = task.download_speed / 1024;
    let remaining_time = task.eta_seconds.unwrap_or(0);

    Emitter::emit_event(&format!("downloaded_bytes_{download_id}"), task.completed_length);
    Emitter::emit_event(&format!("wrote_bytes_{download_id}"), task.completed_length);
    Emitter::emit_event(&format!("disk_speed_{download_id}"), speed_kb);
    Emitter::emit_event(
        &format!("speed_and_remaining_{download_id}"),
        SpeedAndRemaining {
            speed: speed_kb,
            remaining_time,
        },
    );

    if task.total_length > 0 {
        let prev_total = KNOWN_TOTALS.get(&download_id).map(|v| *v).unwrap_or(0);
        if prev_total != task.total_length {
            KNOWN_TOTALS.insert(download_id, task.total_length);
            let _ = DownloadRepository::update(
                download_id,
                UpdateDownload {
                    total_bytes: Some(task.total_length as i64),
                    ..Default::default()
                },
            )
            .await;
            if let Ok(download) = DownloadRepository::find(download_id).await {
                Emitter::emit_event("download_item", &download);
            }
        }
    }

    match task.status.as_str() {
        "complete" => {
            ACTIVE_GIDS.remove(&download_id);
            KNOWN_TOTALS.remove(&download_id);
            let final_bytes = task.total_length.max(task.completed_length) as i64;
            persist_chunk_progress(download_id, final_bytes).await;
            let _ = DownloadRepository::update(
                download_id,
                UpdateDownload {
                    status: Some("completed".to_string()),
                    total_bytes: (final_bytes > 0).then_some(final_bytes),
                    ..Default::default()
                },
            )
            .await;

            if let Ok(download) = DownloadRepository::find(download_id).await {
                Emitter::emit_event("download_item", &download);
                Emitter::emit_notification("Download Completed", download.file_name);
            }
        }
        "error" => {
            ACTIVE_GIDS.remove(&download_id);
            KNOWN_TOTALS.remove(&download_id);
            let is_tls_error = task
                .error_message
                .as_deref()
                .map(|m| {
                    let lower = m.to_ascii_lowercase();
                    lower.contains("ssl/tls") || lower.contains("tls") || lower.contains("handshake")
                })
                .unwrap_or(false);

            if is_tls_error && task.completed_length == 0 {
                debug!("aria2 TLS handshake failed for download {download_id}; falling back to native Rust engine");
                if crate::manager::DownloadsManager::fallback_to_native(download_id)
                    .await
                    .is_ok()
                {
                    return Ok(());
                }
            }

            persist_chunk_progress(download_id, task.completed_length as i64).await;
            let _ = set_download_status(download_id, "failed", task.error_message).await;
        }
        "paused" => {
            ACTIVE_GIDS.remove(&download_id);
            KNOWN_TOTALS.remove(&download_id);
            persist_chunk_progress(download_id, task.completed_length as i64).await;
            let _ = set_download_status(download_id, "paused", None).await;
        }
        "removed" => {
            ACTIVE_GIDS.remove(&download_id);
            KNOWN_TOTALS.remove(&download_id);
        }
        _ => {}
    }

    Ok(())
}

async fn persist_chunk_progress(download_id: i64, bytes: i64) {
    let _ = ChunkRepository::update(
        download_id,
        UpdateChunk {
            chunk_index: 0,
            downloaded_bytes: Some(bytes),
            expected_hash: None,
        },
    )
    .await;
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn sample_options() -> DownloadOptions {
        let mut headers = HashMap::new();
        headers.insert("Referer".into(), "https://example.com".into());
        DownloadOptions {
            file_path: None,
            file_name: None,
            chunk_count: 12,
            proxy: Some(ProxyType::Http {
                host: "127.0.0.1".into(),
                port: 8080,
                username: Some("alice".into()),
                password: Some("pw".into()),
            }),
            auth: None,
            headers: Some(headers),
            cookies: None,
            speed_limit: Some(2_097_152),
            max_retries: Some(5),
            delay_secs: None,
            backoff_factor: None,
            timeout_secs: Some(45.0),
            start_at_unix_ms: None,
            engine: None,
            aria2_path: None,
        }
    }

    #[test]
    fn builds_aria2_rpc_options_from_download_options() {
        let map = to_rpc_options("/tmp/dl", Some("archive.zip"), &sample_options()).to_rpc_map();
        assert_eq!(map.get("dir").and_then(|v| v.as_str()), Some("/tmp/dl"));
        assert_eq!(map.get("out").and_then(|v| v.as_str()), Some("archive.zip"));
        assert_eq!(map.get("split").and_then(|v| v.as_str()), Some("12"));
        assert_eq!(
            map.get("max-download-limit").and_then(|v| v.as_str()),
            Some("2097152")
        );
        assert_eq!(
            map.get("all-proxy").and_then(|v| v.as_str()),
            Some("http://alice:pw@127.0.0.1:8080")
        );
    }

    #[test]
    fn infers_magnet_and_http_names() {
        let (name, ext) =
            infer_name_from_uri("magnet:?xt=urn:btih:1234&dn=Ubuntu+24.04+LTS");
        assert_eq!(name, "Ubuntu 24.04 LTS");
        assert_eq!(ext, "torrent");

        let (name2, ext2) = infer_name_from_uri("ftp://mirror.example.com/pub/release.tar.gz?v=1");
        assert_eq!(name2, "release.tar.gz");
        assert_eq!(ext2, "gz");
    }
}
