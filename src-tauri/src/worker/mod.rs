use anyhow::{anyhow, Context};
use dashmap::DashMap;
use std::{
    sync::{
        atomic::{AtomicBool, AtomicI64},
        Arc,
    },
    time::Instant,
};
use tokio::sync::{Mutex, RwLock};
use tokio_util::sync::CancellationToken;

use crate::{
    client::Client,
    file::FileWriterHandle,
    models::{Download, DownloadChunk},
    registry::{Registry, Report},
};

mod backoff;
mod bandwidth;
mod download;
pub mod hls;
mod status;
mod validation;

pub use status::DownloadStatus;

#[derive(Debug)]
pub struct LiveChunk {
    pub chunk_index: i64,
    pub current_pos: AtomicI64,
    pub end_byte: AtomicI64,
    pub active: AtomicBool,
}

#[derive(Clone, Debug)]
pub struct Worker {
    pub download: Download,
    pub chunks: Vec<DownloadChunk>,
    pub cancel_token: Arc<CancellationToken>,
    pub file: FileWriterHandle,
}

#[derive(Debug, Clone)]
pub struct DownloadWorker {
    pub download_id: i64,
    pub client: Client,
    data: Arc<RwLock<Worker>>,
    report: Arc<Report>,
    chunks_status: Arc<DashMap<i64, status::ChunkDownloadStatus>>,
    live_chunks: Arc<DashMap<i64, Arc<LiveChunk>>>,
    next_chunk_index: Arc<AtomicI64>,
    split_lock: Arc<Mutex<()>>,
    stream_duration: Arc<Mutex<Option<Instant>>>,
    received_bytes: Arc<Mutex<u64>>,
    last_worker_status: Arc<Mutex<DownloadStatus>>,
}

impl DownloadWorker {
    pub async fn new(download_id: i64) -> anyhow::Result<Arc<Self>> {
        let chunks_status = Arc::new(DashMap::new());
        let live_chunks = Arc::new(DashMap::new());
        let state = Registry::get_state();
        let worker = state
            .workers
            .get(&download_id)
            .context(anyhow!("cannot find worker with id {}", download_id))?;
        let report = state
            .reports
            .get(&download_id)
            .context(anyhow!("cannot find report with id {}", download_id))?;

        let (client, max_idx) = {
            let w = worker.read().await;
            let client = Client::new(
                &w.download.url,
                &w.download.auth,
                &w.download.proxy,
                &w.download.headers,
                &w.download.cookies,
            )
            .map_err(|e| anyhow!(e.to_string()))?;

            let max_idx = w
                .chunks
                .iter()
                .map(|c| c.chunk_index)
                .max()
                .unwrap_or(-1);
            (client, max_idx)
        };

        let all_chunks = crate::repository::chunk::ChunkRepository::find_all(download_id)
            .await
            .unwrap_or_default();
        let next_idx = all_chunks
            .iter()
            .map(|c| c.chunk_index)
            .max()
            .unwrap_or(max_idx)
            + 1;

        Ok(Arc::new(Self {
            download_id,
            client,
            data: Arc::clone(&worker),
            report: Arc::clone(&report),
            chunks_status,
            live_chunks,
            next_chunk_index: Arc::new(AtomicI64::new(next_idx)),
            split_lock: Arc::new(Mutex::new(())),
            stream_duration: Arc::new(Mutex::new(None)),
            received_bytes: Arc::new(Mutex::new(0)),
            last_worker_status: Arc::new(Mutex::new(DownloadStatus::Unknown)),
        }))
    }
}
