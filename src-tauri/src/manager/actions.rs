use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{anyhow, Context};
use dashmap::DashMap;
use futures_util::future::join_all;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use tauri_plugin_http::reqwest::Url;
use tokio::select;
use tokio_util::sync::CancellationToken;

use super::chunk::UNKNOWN_SIZE_END;
use crate::{
    aria2,
    client::{AuthType, Client, ProxyType},
    dispatch,
    emitter::Emitter,
    file::File,
    models::{NewDownload, UpdateChunk, UpdateDownload},
    registry::Registry,
    repository::{chunk::ChunkRepository, download::DownloadRepository, engine::EngineRepository},
    worker::{DownloadStatus, DownloadWorker},
};

static SCHEDULED_DOWNLOADS: Lazy<DashMap<i64, CancellationToken>> = Lazy::new(DashMap::new);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DownloadEngine {
    Native,
    Aria2,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DownloadOptions {
    pub file_path: Option<String>,
    pub file_name: Option<String>,
    pub chunk_count: i64,
    pub proxy: Option<ProxyType>,
    pub auth: Option<AuthType>,
    pub headers: Option<HashMap<String, String>>,
    pub cookies: Option<HashMap<String, String>>,
    pub speed_limit: Option<i64>,
    pub max_retries: Option<i64>,
    pub delay_secs: Option<f64>,
    pub backoff_factor: Option<f64>,
    pub timeout_secs: Option<f64>,
    pub start_at_unix_ms: Option<i64>,
    pub engine: Option<DownloadEngine>,
    pub aria2_path: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct BatchResult {
    pub added: usize,
    pub failed: Vec<String>,
}

pub fn cancel_scheduled_download(download_id: i64) -> bool {
    if let Some((_, token)) = SCHEDULED_DOWNLOADS.remove(&download_id) {
        token.cancel();
        true
    } else {
        false
    }
}

impl super::DownloadsManager {
    async fn resolve_target_path(
        requested_dir: &Option<String>,
        file_name: &str,
    ) -> Result<(String, String), String> {
        let file_path = match requested_dir {
            Some(path) if !path.trim().is_empty() => {
                let mut path_buf = PathBuf::from(path);
                path_buf.push(file_name);
                File::get_available_filename(&path_buf.to_string_lossy()).await?
            }
            _ => {
                let default_path = File::get_default_path(file_name).await?;
                File::get_available_filename(&default_path).await?
            }
        };
        let resolved_name = File::get_file_name(&file_path)?;
        Ok((file_path, resolved_name))
    }

    #[allow(clippy::too_many_arguments)]
    fn build_new_download(
        url: String,
        options: &DownloadOptions,
        chunk_count: i64,
        content_type: String,
        extension: String,
        file_name: String,
        file_path: String,
        total_bytes: i64,
        supports_range: bool,
    ) -> NewDownload {
        NewDownload {
            auth: options
                .auth
                .as_ref()
                .and_then(|v| serde_json::to_string(v).ok()),
            backoff_factor: options.backoff_factor,
            chunk_count,
            content_type,
            cookies: options
                .cookies
                .as_ref()
                .and_then(|v| serde_json::to_string(v).ok()),
            delay_secs: options.delay_secs,
            extension,
            file_name,
            file_path,
            headers: options
                .headers
                .as_ref()
                .and_then(|v| serde_json::to_string(v).ok()),
            max_retries: options.max_retries,
            proxy: options
                .proxy
                .as_ref()
                .and_then(|v| serde_json::to_string(v).ok()),
            speed_limit: options.speed_limit.filter(|l| *l > 0),
            status: "queued".to_string(),
            timeout_secs: options.timeout_secs,
            total_bytes,
            url,
            supports_range: i64::from(supports_range),
        }
    }

    pub async fn add_new_download(url: String, options: DownloadOptions) -> Result<(), String> {
        let lower_url = url.trim().to_ascii_lowercase();
        let is_stream_playlist = crate::worker::hls::is_hls_source(&url, "");
        let is_aria2_only_scheme = lower_url.starts_with("magnet:")
            || lower_url.starts_with("ftp://")
            || lower_url.starts_with("sftp://")
            || lower_url.ends_with(".torrent")
            || lower_url.ends_with(".metalink");

        // HLS (.m3u8) and DASH (.mpd) playlists must use the native stream engine
        // so all media segments are fetched and stitched into a playable file.
        let wants_aria2 = !is_stream_playlist
            && (!matches!(options.engine, Some(DownloadEngine::Native)) || is_aria2_only_scheme);

        if is_aria2_only_scheme
            || (wants_aria2 && aria2::is_ready_or_available(options.aria2_path.as_deref()).await)
        {
            return Self::add_aria2_download(url, options).await;
        }

        let client = Client::new(
            &url,
            &options.auth,
            &options.proxy,
            &options.headers,
            &options.cookies,
        )
        .map_err(|e| e.to_string())?;

        let response = client.inspect().await.map_err(|e| e.to_string())?;
        let preferred_name = options
            .file_name
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|stem| {
                if std::path::Path::new(stem).extension().is_some() {
                    stem.to_string()
                } else {
                    format!("{stem}.{}", response.extension)
                }
            })
            .unwrap_or_else(|| response.file_name.clone());
        let (file_path, file_name) =
            Self::resolve_target_path(&options.file_path, &preferred_name).await?;

        let known_length = response.content_length.filter(|len| *len > 0);
        let supports_range = response.supports_range && known_length.is_some();

        let chunk_count = match known_length {
            Some(len) if supports_range => {
                Self::effective_chunk_count(options.chunk_count, len) as i64
            }
            _ => 1,
        };

        let new_download = Self::build_new_download(
            response.url,
            &options,
            chunk_count,
            response.content_type,
            response.extension,
            file_name,
            file_path,
            known_length.unwrap_or(0) as i64,
            supports_range,
        );

        let download_id = DownloadRepository::add(new_download)
            .await
            .map_err(|e| e.to_string())?;

        let range = match known_length {
            Some(len) => Self::get_chunk_ranges(len, chunk_count as u64),
            None => vec![(0, UNKNOWN_SIZE_END as u64)],
        };

        ChunkRepository::create_all(download_id, range)
            .await
            .map_err(|e| {
                e.iter()
                    .map(|f| f.to_string())
                    .collect::<Vec<String>>()
                    .join(", ")
            })?;

        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);

        if let Some(target_ms) = options.start_at_unix_ms.filter(|t| *t > now_ms + 500) {
            let wait_ms = (target_ms - now_ms) as u64;
            if let Ok(download) = DownloadRepository::find(download_id).await {
                Emitter::emit_event("download_item", download);
            }
            let token = CancellationToken::new();
            SCHEDULED_DOWNLOADS.insert(download_id, token.clone());
            tokio::spawn(async move {
                select! {
                    _ = tokio::time::sleep(Duration::from_millis(wait_ms)) => {
                        SCHEDULED_DOWNLOADS.remove(&download_id);
                        let _ = dispatch!(registry, NewDownload, (download_id));
                    }
                    _ = token.cancelled() => {}
                }
            });
        } else {
            let _ = dispatch!(registry, NewDownload, (download_id));
        }

        Ok(())
    }

    async fn add_aria2_download(url: String, options: DownloadOptions) -> Result<(), String> {
        let (inferred_name, ext) = aria2::infer_name_from_uri(&url);
        let (file_path, file_name) =
            Self::resolve_target_path(&options.file_path, &inferred_name).await?;

        let new_download = Self::build_new_download(
            url.trim().to_string(),
            &options,
            options.chunk_count.clamp(1, 16),
            "application/octet-stream".to_string(),
            ext,
            file_name,
            file_path,
            0,
            true,
        );

        let download_id = DownloadRepository::add(new_download)
            .await
            .map_err(|e| e.to_string())?;

        ChunkRepository::create_all(download_id, vec![(0, 0)])
            .await
            .map_err(|e| {
                e.iter()
                    .map(|f| f.to_string())
                    .collect::<Vec<String>>()
                    .join(", ")
            })?;

        EngineRepository::set(download_id, "aria2", None)
            .await
            .map_err(|e| e.to_string())?;

        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);

        if let Some(target_ms) = options.start_at_unix_ms.filter(|t| *t > now_ms + 500) {
            let wait_ms = (target_ms - now_ms) as u64;
            if let Ok(download) = DownloadRepository::find(download_id).await {
                Emitter::emit_event("download_item", download);
            }
            let token = CancellationToken::new();
            SCHEDULED_DOWNLOADS.insert(download_id, token.clone());
            let opts = options.clone();
            tokio::spawn(async move {
                select! {
                    _ = tokio::time::sleep(Duration::from_millis(wait_ms)) => {
                        SCHEDULED_DOWNLOADS.remove(&download_id);
                        if let Err(err) = aria2::start_download(download_id, &opts).await {
                            Emitter::emit_error(err);
                        }
                    }
                    _ = token.cancelled() => {}
                }
            });
        } else {
            aria2::start_download(download_id, &options).await?;
        }

        Ok(())
    }

    /// Fallback for downloads where aria2's WinTLS/Schannel fails an SSL/TLS handshake:
    /// inspects the URL with danDown's `rustls` client, updates chunks, and queues it on
    /// the native Rust engine.
    pub async fn fallback_to_native(download_id: i64) -> Result<(), String> {
        let existing = DownloadRepository::find(download_id)
            .await
            .map_err(|e| e.to_string())?;

        let client = Client::new(
            &existing.url,
            &existing.auth,
            &existing.proxy,
            &existing.headers,
            &existing.cookies,
        )
        .map_err(|e| e.to_string())?;

        let response = client.inspect().await.map_err(|e| e.to_string())?;
        let known_length = response.content_length.filter(|len| *len > 0);
        let supports_range = response.supports_range && known_length.is_some();
        let chunk_count = match known_length {
            Some(len) if supports_range => {
                Self::effective_chunk_count(existing.chunk_count, len) as i64
            }
            _ => 1,
        };

        EngineRepository::set(download_id, "native", None)
            .await
            .map_err(|e| e.to_string())?;

        DownloadRepository::update(
            download_id,
            UpdateDownload {
                url: Some(response.url),
                status: Some("queued".to_string()),
                total_bytes: Some(known_length.unwrap_or(0) as i64),
                chunk_count: Some(chunk_count),
                error_message: Some(String::new()),
                ..Default::default()
            },
        )
        .await
        .map_err(|e| e.to_string())?;

        let pool = Registry::get_pool();
        let _ = sqlx::query("DELETE FROM download_chunks WHERE download_id = ?")
            .bind(download_id)
            .execute(pool)
            .await;

        let range = match known_length {
            Some(len) => Self::get_chunk_ranges(len, chunk_count as u64),
            None => vec![(0, UNKNOWN_SIZE_END as u64)],
        };
        ChunkRepository::create_all(download_id, range)
            .await
            .map_err(|e| {
                e.iter()
                    .map(|f| f.to_string())
                    .collect::<Vec<String>>()
                    .join(", ")
            })?;

        let _ = dispatch!(registry, NewDownload, (download_id));
        Ok(())
    }

    pub async fn add_batch_downloads(
        urls: Vec<String>,
        options: DownloadOptions,
    ) -> Result<BatchResult, String> {
        let mut added = 0usize;
        let mut failed = Vec::new();

        for raw in urls {
            let trimmed = raw.trim().to_string();
            if trimmed.is_empty() {
                continue;
            }
            match Self::add_new_download(trimmed.clone(), options.clone()).await {
                Ok(()) => added += 1,
                Err(err) => failed.push(format!("{trimmed}: {err}")),
            }
        }

        Ok(BatchResult { added, failed })
    }

    /// Updates an expired signed CDN link (e.g. 403/410) without discarding already
    /// downloaded & BLAKE3-verified chunks, and optionally resumes immediately.
    pub async fn refresh_download_url(
        download_id: i64,
        new_url: String,
        resume: bool,
    ) -> Result<(), String> {
        let trimmed = new_url.trim();
        let parsed = Url::parse(trimmed).map_err(|e| format!("Invalid URL: {e}"))?;
        if parsed.scheme() != "http" && parsed.scheme() != "https" {
            return Err("URL must start with http:// or https://".to_string());
        }

        let existing = DownloadRepository::find(download_id)
            .await
            .map_err(|e| e.to_string())?;

        if existing.status == "downloading" {
            return Err("Pause the download before updating its URL".to_string());
        }

        DownloadRepository::update(
            download_id,
            UpdateDownload {
                url: Some(parsed.to_string()),
                error_message: Some(String::new()),
                ..Default::default()
            },
        )
        .await
        .map_err(|e| e.to_string())?;

        if let Ok(updated) = DownloadRepository::find(download_id).await {
            Emitter::emit_event("download_item", &updated);
        }

        if resume {
            let _ = dispatch!(registry, ResumeDownload, (download_id));
        }

        Ok(())
    }

    pub(super) async fn start_download_action(
        self: &Arc<Self>,
        download_id: i64,
    ) -> anyhow::Result<()> {
        Self::start_monitoring();
        let download_worker = DownloadWorker::new(download_id).await?;

        download_worker.start_download().await;

        Ok(())
    }

    pub(super) async fn update_download_status_action(
        self: &Arc<Self>,
        status: DownloadStatus,
        error_message: Option<String>,
        download_id: i64,
    ) -> anyhow::Result<()> {
        match status {
            DownloadStatus::Failed | DownloadStatus::Completed | DownloadStatus::Paused => {
                let _ = dispatch!(manager, UpdateChunks, (download_id, true));
            }
            _ => {}
        }

        DownloadRepository::update(
            download_id,
            UpdateDownload {
                status: Some(status.to_string()),
                error_message,
                ..Default::default()
            },
        )
        .await?;

        let download = DownloadRepository::find(download_id).await?;
        Emitter::emit_event("download_item", &download);

        if matches!(status, DownloadStatus::Completed) {
            Emitter::emit_notification("Download Completed", download.file_name);
        }

        Ok(())
    }

    pub(super) async fn pause_download_action(
        self: &Arc<Self>,
        download_id: i64,
    ) -> anyhow::Result<()> {
        if cancel_scheduled_download(download_id) {
            DownloadRepository::update(
                download_id,
                UpdateDownload {
                    status: Some("paused".to_string()),
                    ..Default::default()
                },
            )
            .await?;
            if let Ok(download) = DownloadRepository::find(download_id).await {
                Emitter::emit_event("download_item", &download);
            }
            return Ok(());
        }

        let workers = Arc::clone(&Registry::get_state().workers);
        if let Some(worker) = workers.get(&download_id) {
            let worker = worker.read().await;
            worker.cancel_token.cancel();
            return Ok(());
        }

        // Also handle items sitting in the pending queue before a worker is created.
        {
            let pending = Arc::clone(&Registry::get_state().pending_queue);
            let mut q = pending.lock().await;
            q.retain(|id| *id != download_id);
        }
        DownloadRepository::update(
            download_id,
            UpdateDownload {
                status: Some("paused".to_string()),
                ..Default::default()
            },
        )
        .await?;
        if let Ok(download) = DownloadRepository::find(download_id).await {
            Emitter::emit_event("download_item", &download);
        }

        Ok(())
    }

    pub(super) async fn update_chunks_action(
        self: &Arc<Self>,
        download_id: i64,
        clean_after_update: bool,
    ) -> anyhow::Result<()> {
        let workers = Arc::clone(&Registry::get_state().workers);
        let worker = workers.get(&download_id).context(anyhow!(
            "update chunk error: cannot find worker with download id {}",
            download_id
        ))?;

        let reports = Arc::clone(&Registry::get_state().reports);

        let worker = worker.write().await;
        let update_chunks_futures = worker.chunks.iter().map(|chunk| {
            let chunk_index = chunk.chunk_index;
            let wrote_bytes = reports
                .get(&download_id)
                .and_then(|r| {
                    r.chunks_wrote_bytes
                        .get(&chunk_index)
                        .map(|v| v.load(Ordering::Relaxed))
                })
                .unwrap_or(0);

            async move {
                let expected_hash = Self::hash_chunk(download_id, chunk_index)
                    .await
                    .ok()
                    .map(|h| h.to_string());

                UpdateChunk {
                    downloaded_bytes: Some(wrote_bytes as i64),
                    expected_hash,
                    chunk_index,
                }
            }
        });

        let update_chunks = join_all(update_chunks_futures).await;

        if let Err(errors) = ChunkRepository::update_all(download_id, update_chunks).await {
            errors.iter().for_each(|err| {
                Emitter::emit_error(err.to_string());
            });
        }

        if clean_after_update {
            let _ = dispatch!(registry, CleanDownloadedItemData, (download_id));
        }

        Ok(())
    }

    pub(super) async fn reset_chunk_action(
        self: &Arc<Self>,
        download_id: i64,
        chunk_index: i64,
    ) -> anyhow::Result<()> {
        ChunkRepository::update(
            download_id,
            UpdateChunk {
                chunk_index,
                downloaded_bytes: Some(0),
                expected_hash: None,
            },
        )
        .await?;

        let reports = Arc::clone(&Registry::get_state().reports);

        let report = reports.get(&download_id).context(anyhow!(
            "reset chunk error: cannot find report with download id {}",
            download_id
        ))?;

        let chunk_counter = report.chunks_wrote_bytes.get(&chunk_index).context(anyhow!(
            "reset chunk error: cannot find chunk wrote bytes in report with download id {} and chunk index {}",
            download_id,
            chunk_index
        ))?;

        let prev_chunk_bytes = chunk_counter.swap(0, Ordering::AcqRel);

        let saturating_fetch_sub = |atom: &AtomicU64, amount: u64| {
            let _ = atom.fetch_update(Ordering::AcqRel, Ordering::Acquire, |cur| {
                Some(cur.saturating_sub(amount))
            });
        };

        saturating_fetch_sub(&report.total_downloaded_bytes, prev_chunk_bytes);
        saturating_fetch_sub(&report.total_wrote_bytes, prev_chunk_bytes);

        Ok(())
    }
}
