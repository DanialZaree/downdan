use std::{
    sync::{atomic::Ordering, Arc},
    time::Instant,
};

use anyhow::{Context, Result};
use blake3::{Hash, Hasher};

use crate::{dispatch, registry::Registry};

pub const MAX_CHUNKS: u64 = 16;
pub const MIN_INITIAL_CHUNK_BYTES: u64 = 256 * 1024;
pub const UNKNOWN_SIZE_END: i64 = i64::MAX;

const PROGRESS_UPDATE_THRESHOLD: u8 = 5;
const TIME_UPDATE_THRESHOLD: u64 = 10;
const TIME_MAX_STALE: u64 = 300;

impl super::DownloadsManager {
    pub(super) fn effective_chunk_count(requested: i64, content_length: u64) -> u64 {
        if content_length == 0 {
            return 1;
        }
        let clamped = requested.clamp(1, MAX_CHUNKS as i64) as u64;
        let max_by_size = (content_length / MIN_INITIAL_CHUNK_BYTES).max(1);
        clamped.min(max_by_size)
    }

    pub(super) fn get_chunk_ranges(content_length: u64, chunk_count: u64) -> Vec<(u64, u64)> {
        let chunk = chunk_count.max(1).min(content_length.max(1));
        let mut ranges = Vec::with_capacity(chunk as usize);

        if content_length == 0 {
            return vec![(0, 0)];
        }

        let base_chunk_size = content_length / chunk;
        let remainder = content_length % chunk;

        let mut start = 0;

        for i in 0..chunk {
            let extra = if i < remainder { 1 } else { 0 };
            let end = start + base_chunk_size + extra - 1;

            ranges.push((start, end));
            start = end + 1;
        }

        ranges
    }

    pub(super) async fn update_chunks_monitor() {
        let reports = Arc::clone(&Registry::get_state().reports);

        for r in reports.iter() {
            if r.total_bytes == 0 {
                continue;
            }

            let total_downloaded_bytes = r.total_downloaded_bytes.load(Ordering::Relaxed);
            let last_update_downloaded_bytes =
                r.last_update_downloaded_bytes.load(Ordering::Relaxed);

            let last_update_time = Arc::clone(&r.last_update_time);
            let mut last_update_time = last_update_time.lock().await;

            let percent = (100 * total_downloaded_bytes / r.total_bytes).min(100) as u8;
            let last_percent = (100 * last_update_downloaded_bytes / r.total_bytes).min(100) as u8;
            let is_five_percent =
                percent.saturating_sub(last_percent) >= PROGRESS_UPDATE_THRESHOLD;

            let elapsed = last_update_time.elapsed().as_secs();
            let more_than_ten_seconds = elapsed >= TIME_UPDATE_THRESHOLD;
            let more_than_five_minutes = elapsed >= TIME_MAX_STALE;

            if more_than_five_minutes || (is_five_percent && more_than_ten_seconds) {
                let _ = dispatch!(manager, UpdateChunks, (*r.key(), false));

                *last_update_time = Instant::now();
                r.last_update_downloaded_bytes
                    .store(total_downloaded_bytes, Ordering::Relaxed);
            }
        }
    }

    pub async fn hash_chunk(download_id: i64, chunk_index: i64) -> Result<Hash> {
        let reports = Arc::clone(&Registry::get_state().reports);

        let report = reports
            .get(&download_id)
            .with_context(|| format!("report not found for download id {}", download_id))?;

        let buffer = report.buffer.get(&chunk_index).with_context(|| {
            format!(
                "buffer not found for download id {} with chunk index {}",
                download_id, chunk_index
            )
        })?;

        let buffer_lock = buffer.lock().await;

        if buffer_lock.first.len() + buffer_lock.last.len() < 2048 {
            return Err(anyhow::anyhow!(
                "buffer for download id {} with chunk index {} must be at least 2048 bytes",
                download_id,
                chunk_index
            ));
        }

        let mut hasher = Hasher::new();
        hasher.update(&buffer_lock.first);
        hasher.update(&buffer_lock.last);

        Ok(hasher.finalize())
    }
}

#[cfg(test)]
mod tests {
    use crate::manager::DownloadsManager;

    #[test]
    fn chunk_ranges_cover_exact_byte_length() {
        let ranges = DownloadsManager::get_chunk_ranges(1_000, 3);
        assert_eq!(ranges, vec![(0, 333), (334, 666), (667, 999)]);
        let total: u64 = ranges.iter().map(|(s, e)| e - s + 1).sum();
        assert_eq!(total, 1_000);
    }

    #[test]
    fn effective_chunk_count_respects_16_cap_and_small_files() {
        assert_eq!(DownloadsManager::effective_chunk_count(16, 100 * 1024 * 1024), 16);
        assert_eq!(DownloadsManager::effective_chunk_count(32, 100 * 1024 * 1024), 16);
        assert_eq!(DownloadsManager::effective_chunk_count(8, 100 * 1024), 1);
        assert_eq!(DownloadsManager::effective_chunk_count(8, 600 * 1024), 2);
    }
}
