use futures_util::StreamExt;
use std::{
    sync::{
        atomic::{AtomicBool, AtomicI64, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::{
    select,
    time::{sleep, timeout},
};

use crate::{
    client::ClientError,
    dispatch,
    models::UpdateDownload,
    repository::{chunk::ChunkRepository, download::DownloadRepository},
    spawn,
    worker::{hls::is_hls_source, status::ChunkDownloadStatus, LiveChunk},
};

use super::*;

/// Minimum remaining bytes in an active chunk before an idle worker will split it in half.
pub const MIN_SPLIT_REMAINING_BYTES: i64 = 512 * 1024;

/// Pure helper so dynamic work-stealing split math can be unit-tested deterministically.
pub fn compute_split_point(current_pos: i64, end_byte: i64) -> Option<(i64, i64)> {
    let remaining = end_byte - current_pos + 1;
    if remaining < MIN_SPLIT_REMAINING_BYTES {
        return None;
    }
    let split_start = current_pos + (remaining / 2);
    if split_start <= current_pos || split_start > end_byte {
        return None;
    }
    Some((split_start - 1, split_start))
}

impl DownloadWorker {
    pub async fn start_download(self: &Arc<Self>) {
        use ChunkDownloadStatus::*;

        let (chunks, cancel_token, max_retries) = {
            let worker = self.data.read().await;
            (
                worker.chunks.clone(),
                Arc::clone(&worker.cancel_token),
                worker.download.max_retries,
            )
        };

        for c in &chunks {
            let initial_wrote = self
                .report
                .chunks_wrote_bytes
                .get(&c.chunk_index)
                .map(|b| b.load(Ordering::SeqCst) as i64)
                .unwrap_or(c.downloaded_bytes);

            self.live_chunks.insert(
                c.chunk_index,
                Arc::new(LiveChunk {
                    chunk_index: c.chunk_index,
                    current_pos: AtomicI64::new(c.start_byte + initial_wrote),
                    end_byte: AtomicI64::new(c.end_byte),
                    active: AtomicBool::new(true),
                }),
            );
            self.chunks_status.insert(c.chunk_index, Downloading);
        }

        for initial_chunk in chunks {
            let worker_clone = Arc::clone(self);
            let cancel_token = Arc::clone(&cancel_token);

            spawn!("download_chunk", {
                let mut chunk = initial_chunk;
                let mut retries = 0;

                loop {
                    worker_clone
                        .update_chunk_status(chunk.chunk_index, Downloading)
                        .await;

                    let st = select! {
                        status = worker_clone.download_chunk(&chunk) => {
                            match status {
                                Ok(()) => Finished,
                                Err(err) if err.is_retryable() => Trying(err),
                                Err(err) => Errored(err),
                            }
                        },
                        _ = cancel_token.cancelled() => Paused,
                    };

                    match st {
                        Finished => {
                            if let Some(live) = worker_clone.live_chunks.get(&chunk.chunk_index) {
                                live.active.store(false, Ordering::Release);
                            }

                            // Dynamic work-stealing: before idling this connection slot, check
                            // whether another active chunk still has >= 512 KiB left and split it.
                            if !cancel_token.is_cancelled() {
                                if let Some(stolen) = worker_clone.try_steal_chunk().await {
                                    worker_clone
                                        .update_chunk_status(chunk.chunk_index, Finished)
                                        .await;
                                    chunk = stolen;
                                    retries = 0;
                                    continue;
                                }
                            }

                            worker_clone
                                .finalize_if_last_chunk(chunk.chunk_index)
                                .await;
                            break;
                        }
                        Paused => {
                            if let Some(live) = worker_clone.live_chunks.get(&chunk.chunk_index) {
                                live.active.store(false, Ordering::Release);
                            }
                            let _ = worker_clone.flush_file().await;
                            worker_clone
                                .update_chunk_status(chunk.chunk_index, Paused)
                                .await;
                            break;
                        }
                        Errored(err) => {
                            if let Some(live) = worker_clone.live_chunks.get(&chunk.chunk_index) {
                                live.active.store(false, Ordering::Release);
                            }
                            let _ = worker_clone.flush_file().await;
                            worker_clone
                                .update_chunk_status(chunk.chunk_index, Errored(err))
                                .await;
                            cancel_token.cancel();
                            break;
                        }
                        Trying(err) => {
                            if cancel_token.is_cancelled() {
                                if let Some(live) =
                                    worker_clone.live_chunks.get(&chunk.chunk_index)
                                {
                                    live.active.store(false, Ordering::Release);
                                }
                                worker_clone
                                    .update_chunk_status(chunk.chunk_index, Paused)
                                    .await;
                                break;
                            }

                            if retries < max_retries {
                                worker_clone
                                    .update_chunk_status(chunk.chunk_index, Trying(err.clone()))
                                    .await;

                                if matches!(err, ClientError::UnexpectedChunkHash) {
                                    chunk.expected_hash = None;
                                    let _ = dispatch!(
                                        manager,
                                        ResetChunk,
                                        (chunk.download_id, chunk.chunk_index)
                                    );
                                    if let Some(live) =
                                        worker_clone.live_chunks.get(&chunk.chunk_index)
                                    {
                                        live.current_pos
                                            .store(chunk.start_byte, Ordering::Release);
                                    }
                                }

                                retries += 1;
                                let delay = worker_clone.backoff_delay(retries).await;
                                let slept = select! {
                                    _ = sleep(delay) => true,
                                    _ = cancel_token.cancelled() => false,
                                };

                                if !slept {
                                    worker_clone
                                        .update_chunk_status(chunk.chunk_index, Paused)
                                        .await;
                                    break;
                                }
                            } else {
                                if let Some(live) =
                                    worker_clone.live_chunks.get(&chunk.chunk_index)
                                {
                                    live.active.store(false, Ordering::Release);
                                }
                                let _ = worker_clone.flush_file().await;
                                worker_clone
                                    .update_chunk_status(chunk.chunk_index, Errored(err))
                                    .await;
                                cancel_token.cancel();
                                break;
                            }
                        }
                        Downloading => {}
                    }
                }
            });
        }
    }

    async fn flush_file(&self) -> Result<(), ClientError> {
        let file = self.data.read().await.file.clone();
        file.flush().await.map_err(ClientError::DiskWrite)
    }

    async fn finalize_if_last_chunk(&self, finished_chunk_index: i64) {
        let _ = self.flush_file().await;

        let is_unknown_size = self.data.read().await.download.total_bytes <= 0;
        if is_unknown_size {
            let final_bytes = self.report.total_downloaded_bytes.load(Ordering::SeqCst) as i64;
            if final_bytes > 0 {
                let _ = DownloadRepository::update(
                    self.download_id,
                    UpdateDownload {
                        total_bytes: Some(final_bytes),
                        ..Default::default()
                    },
                )
                .await;
                let _ = ChunkRepository::update_end_byte(
                    self.download_id,
                    finished_chunk_index,
                    final_bytes - 1,
                )
                .await;
            }
        }

        self.update_chunk_status(finished_chunk_index, ChunkDownloadStatus::Finished)
            .await;
    }

    /// Finds the active chunk with the largest remaining byte range and splits its un-downloaded
    /// tail in half so this idle worker can download the second half in parallel.
    async fn try_steal_chunk(&self) -> Option<DownloadChunk> {
        let _guard = self.split_lock.lock().await;

        let (supports_range, total_bytes) = {
            let w = self.data.read().await;
            (w.download.supports_range, w.download.total_bytes)
        };

        if !supports_range || total_bytes <= 0 {
            return None;
        }

        let mut victim: Option<(Arc<LiveChunk>, i64)> = None;
        for entry in self.live_chunks.iter() {
            let live = entry.value();
            if !live.active.load(Ordering::Acquire) {
                continue;
            }
            let pos = live.current_pos.load(Ordering::Acquire);
            let end = live.end_byte.load(Ordering::Acquire);
            let rem = end - pos + 1;
            if rem >= MIN_SPLIT_REMAINING_BYTES {
                if victim.as_ref().map_or(true, |(_, best_rem)| rem > *best_rem) {
                    victim = Some((Arc::clone(live), rem));
                }
            }
        }

        let (victim_live, _) = victim?;
        let pos = victim_live.current_pos.load(Ordering::Acquire);
        let old_end = victim_live.end_byte.load(Ordering::Acquire);
        let (new_victim_end, new_start) = compute_split_point(pos, old_end)?;

        // Shrink the slow chunk's end boundary first; its stream loop checks `end_byte`
        // on every packet and stops cleanly at `new_victim_end`.
        victim_live.end_byte.store(new_victim_end, Ordering::Release);

        let new_index = self.next_chunk_index.fetch_add(1, Ordering::SeqCst);

        if ChunkRepository::update_end_byte(
            self.download_id,
            victim_live.chunk_index,
            new_victim_end,
        )
        .await
        .is_err()
        {
            victim_live.end_byte.store(old_end, Ordering::Release);
            return None;
        }

        if ChunkRepository::create(self.download_id, new_index, (new_start, old_end))
            .await
            .is_err()
        {
            let _ = ChunkRepository::update_end_byte(
                self.download_id,
                victim_live.chunk_index,
                old_end,
            )
            .await;
            victim_live.end_byte.store(old_end, Ordering::Release);
            return None;
        }

        let stolen = DownloadChunk {
            download_id: self.download_id,
            chunk_index: new_index,
            start_byte: new_start,
            end_byte: old_end,
            downloaded_bytes: 0,
            expected_hash: None,
        };

        {
            let mut w = self.data.write().await;
            if let Some(existing) = w
                .chunks
                .iter_mut()
                .find(|c| c.chunk_index == victim_live.chunk_index)
            {
                existing.end_byte = new_victim_end;
            }
            w.chunks.push(stolen.clone());
        }

        self.report
            .chunks_wrote_bytes
            .insert(new_index, std::sync::atomic::AtomicU64::new(0));

        // Register the stolen chunk as Downloading BEFORE the caller marks its old chunk Finished,
        // so `calculate_worker_status` never sees a false all-finished state.
        self.live_chunks.insert(
            new_index,
            Arc::new(LiveChunk {
                chunk_index: new_index,
                current_pos: AtomicI64::new(new_start),
                end_byte: AtomicI64::new(old_end),
                active: AtomicBool::new(true),
            }),
        );
        self.chunks_status
            .insert(new_index, ChunkDownloadStatus::Downloading);

        Some(stolen)
    }

    async fn download_chunk(self: &Arc<Self>, chunk: &DownloadChunk) -> Result<(), ClientError> {
        {
            let w = self.data.read().await;
            if is_hls_source(&w.download.url, &w.download.content_type) {
                drop(w);
                return self.download_hls_stream(chunk.chunk_index).await;
            }
        }

        if self
            .validate_chunk(chunk.expected_hash.clone(), chunk.chunk_index)
            .await
            .is_err()
        {
            return Err(ClientError::UnexpectedChunkHash);
        }

        let live = self.live_chunks.get(&chunk.chunk_index).map(|l| l.clone());

        let mut downloaded_bytes = match self.report.chunks_wrote_bytes.get(&chunk.chunk_index) {
            Some(bytes) => bytes.load(Ordering::SeqCst) as i64,
            None => 0,
        };

        let start_byte = chunk.start_byte;
        let current_end = live
            .as_ref()
            .map(|l| l.end_byte.load(Ordering::Acquire))
            .unwrap_or(chunk.end_byte);

        if start_byte + downloaded_bytes > current_end {
            return Ok(());
        }

        let (range, timeout_secs, file, is_known_size) = {
            let w = self.data.read().await;
            let is_known_size = w.download.total_bytes > 0;
            let range = if w.download.supports_range && is_known_size {
                Some((start_byte + downloaded_bytes, current_end))
            } else {
                None
            };

            (
                range,
                w.download.timeout_secs.max(5) as u64,
                w.file.clone(),
                is_known_size,
            )
        };

        let mut stream = self.client.stream(range).await?;

        loop {
            let limit_end = live
                .as_ref()
                .map(|l| l.end_byte.load(Ordering::Acquire))
                .unwrap_or(chunk.end_byte);

            if is_known_size && start_byte + downloaded_bytes > limit_end {
                return Ok(());
            }

            match timeout(Duration::from_secs(timeout_secs), stream.next()).await {
                Ok(Some(Ok(bytes))) => {
                    if bytes.is_empty() {
                        continue;
                    }

                    let offset = start_byte + downloaded_bytes;
                    let write_slice = if is_known_size {
                        let max_len = (limit_end - offset + 1).max(0) as usize;
                        if max_len == 0 {
                            return Ok(());
                        }
                        &bytes[..bytes.len().min(max_len)]
                    } else {
                        &bytes[..]
                    };

                    let bytes_len = write_slice.len() as u64;

                    file.write(chunk.chunk_index, offset as u64, write_slice.to_vec())
                        .await
                        .map_err(ClientError::DiskWrite)?;

                    downloaded_bytes += bytes_len as i64;
                    if let Some(ref l) = live {
                        l.current_pos
                            .store(start_byte + downloaded_bytes, Ordering::Release);
                    }

                    self.limiter(bytes_len).await;

                    let _ = dispatch!(registry, UpdateNetworkReport, (self.download_id, bytes_len));

                    if is_known_size && start_byte + downloaded_bytes > limit_end {
                        return Ok(());
                    }
                }
                Ok(Some(Err(err))) => {
                    return Err(err);
                }
                Ok(None) => {
                    return Ok(());
                }
                Err(_) => {
                    return Err(ClientError::StreamTimeout);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_point_divides_remaining_bytes_cleanly() {
        // 2 MiB remaining: [0..=2_097_151]
        let (victim_end, new_start) = compute_split_point(0, 2_097_151).unwrap();
        assert_eq!(victim_end + 1, new_start);
        assert_eq!(victim_end, 1_048_575);
        assert_eq!(new_start, 1_048_576);
    }

    #[test]
    fn split_point_refuses_small_tail() {
        assert!(compute_split_point(0, MIN_SPLIT_REMAINING_BYTES - 2).is_none());
    }
}
