use std::{path::Path, sync::Arc};

use tauri::Manager;

use crate::{
    aria2::{self, Aria2StatusInfo},
    dispatch,
    engine::EngineState,
    manager::{BatchResult, DownloadOptions, DownloadsManager},
    models::Download,
    repository::download::DownloadRepository,
};

#[tauri::command]
pub async fn check_aria2_status(custom_path: Option<String>) -> Aria2StatusInfo {
    aria2::check_status(custom_path).await
}

#[tauri::command]
pub async fn add_new_download(url: String, options: DownloadOptions) -> Result<(), String> {
    DownloadsManager::add_new_download(url, options).await
}

#[tauri::command]
pub async fn add_batch_downloads(
    urls: Vec<String>,
    options: DownloadOptions,
) -> Result<BatchResult, String> {
    DownloadsManager::add_batch_downloads(urls, options).await
}

/// Imports `.torrent` files (via aria2 `addTorrent`) or `.txt` / `.lst` URL lists dropped
/// directly onto the danDown window.
#[tauri::command]
pub async fn import_dropped_files(
    app: tauri::AppHandle,
    paths: Vec<String>,
) -> Result<usize, String> {
    let mut queued = 0usize;

    for raw_path in paths {
        let path = Path::new(&raw_path);
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();

        if ext == "torrent" {
            let bytes = tokio::fs::read(path)
                .await
                .map_err(|e| format!("Failed to read torrent file: {e}"))?;
            let b64 = encode_base64(&bytes);
            if let Some(engine) = app.try_state::<Arc<EngineState>>() {
                engine.ensure_ready().map_err(|e| e.to_string())?;
                engine
                    .client
                    .add_torrent(b64, None)
                    .await
                    .map_err(|e| e.to_string())?;
                queued += 1;
            } else {
                return Err("aria2c sidecar is not running to import .torrent files".to_string());
            }
        } else if matches!(ext.as_str(), "txt" | "lst" | "urls") {
            let text = tokio::fs::read_to_string(path)
                .await
                .map_err(|e| format!("Failed to read URL list: {e}"))?;
            let urls: Vec<String> = text
                .lines()
                .map(str::trim)
                .filter(|line| {
                    let lower = line.to_ascii_lowercase();
                    lower.starts_with("http://")
                        || lower.starts_with("https://")
                        || lower.starts_with("ftp://")
                        || lower.starts_with("sftp://")
                        || lower.starts_with("magnet:?")
                })
                .map(str::to_string)
                .collect();

            if !urls.is_empty() {
                let opts = DownloadOptions {
                    file_path: None,
                    file_name: None,
                    chunk_count: 8,
                    proxy: None,
                    auth: None,
                    headers: None,
                    cookies: None,
                    speed_limit: None,
                    max_retries: Some(3),
                    delay_secs: None,
                    backoff_factor: Some(2.0),
                    timeout_secs: Some(30.0),
                    start_at_unix_ms: None,
                    engine: None,
                    aria2_path: None,
                };
                let res = DownloadsManager::add_batch_downloads(urls, opts).await?;
                queued += res.added;
            }
        }
    }

    Ok(queued)
}

fn encode_base64(input: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(TABLE[((triple >> 18) & 0x3f) as usize] as char);
        out.push(TABLE[((triple >> 12) & 0x3f) as usize] as char);
        out.push(if chunk.len() > 1 {
            TABLE[((triple >> 6) & 0x3f) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[(triple & 0x3f) as usize] as char
        } else {
            '='
        });
    }
    out
}

#[tauri::command]
pub async fn update_download_url(id: i64, url: String, resume: bool) -> Result<(), String> {
    DownloadsManager::refresh_download_url(id, url, resume).await
}

#[tauri::command]
pub async fn get_download_list() -> Result<Vec<Download>, String> {
    DownloadRepository::find_all(None)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn pause_download(id: i64) {
    let _ = dispatch!(registry, PauseDownload, (id));
}

#[tauri::command]
pub fn resume_download(id: i64) {
    let _ = dispatch!(registry, ResumeDownload, (id));
}

#[tauri::command]
pub fn remove_download(id: i64, remove_file: bool) {
    let _ = dispatch!(registry, RemoveDownload, (id, remove_file));
}
