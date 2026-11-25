use crate::registry::Registry;

use std::{
    sync::{atomic::Ordering, Arc},
    time::{Duration, Instant},
};
use tokio::time::sleep;

impl super::DownloadWorker {
    pub async fn limiter(&self, bytes_len: u64) {
        let global_limit =
            Arc::clone(&Registry::get_state().bandwidth_limit).load(Ordering::Relaxed);
        let per_download_limit = self
            .data
            .read()
            .await
            .download
            .speed_limit
            .unwrap_or(0)
            .max(0) as f64;

        let bandwidth_limit = match (global_limit > 0.0, per_download_limit > 0.0) {
            (true, true) => global_limit.min(per_download_limit),
            (true, false) => global_limit,
            (false, true) => per_download_limit,
            (false, false) => return,
        };

        let mut received_bytes = self.received_bytes.lock().await;
        let mut duration = self.stream_duration.lock().await;

        if duration.is_none() {
            *duration = Some(Instant::now());
        }

        *received_bytes += bytes_len;

        if *received_bytes as f64 >= bandwidth_limit {
            if let Some(instant) = *duration {
                let elapsed = instant.elapsed().as_secs_f64();
                let expected = (*received_bytes as f64) / bandwidth_limit;
                if expected > elapsed {
                    sleep(Duration::from_secs_f64(expected - elapsed)).await;
                }
            }

            *duration = Some(Instant::now());
            *received_bytes = 0;
        } else if let Some(instant) = *duration {
            if instant.elapsed().as_secs_f64() >= 1.0 {
                *received_bytes = 0;
                *duration = Some(Instant::now());
            }
        }
    }
}
