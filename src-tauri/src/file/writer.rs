use std::io::SeekFrom;
use std::sync::{Arc, OnceLock};

use tokio::fs::{self, OpenOptions};
use tokio::io::{AsyncSeekExt, AsyncWriteExt, BufWriter};
use tokio::sync::{mpsc, oneshot};

use crate::{dispatch, emitter::Emitter, spawn};

pub const WRITER_QUEUE_CAPACITY: usize = 256;
const BUFFER_CAPACITY: usize = 256 * 1024;

pub enum WriteCommand {
    Chunk {
        chunk_index: i64,
        offset: u64,
        bytes: Vec<u8>,
    },
    Flush(oneshot::Sender<Result<(), String>>),
}

#[derive(Clone, Debug)]
pub struct FileWriterHandle {
    tx: mpsc::Sender<WriteCommand>,
    io_error: Arc<OnceLock<String>>,
}

impl FileWriterHandle {
    pub async fn write(&self, chunk_index: i64, offset: u64, bytes: Vec<u8>) -> Result<(), String> {
        if let Some(err) = self.io_error.get() {
            return Err(err.clone());
        }

        self.tx
            .send(WriteCommand::Chunk {
                chunk_index,
                offset,
                bytes,
            })
            .await
            .map_err(|_| {
                self.io_error
                    .get()
                    .cloned()
                    .unwrap_or_else(|| "file writer channel closed unexpectedly".to_string())
            })
    }

    pub async fn flush(&self) -> Result<(), String> {
        if let Some(err) = self.io_error.get() {
            return Err(err.clone());
        }

        let (tx, rx) = oneshot::channel();
        if self.tx.send(WriteCommand::Flush(tx)).await.is_err() {
            return Err(self
                .io_error
                .get()
                .cloned()
                .unwrap_or_default());
        }

        rx.await.unwrap_or_else(|_| Ok(()))
    }
}

impl super::File {
    pub(super) async fn setup_file_writer(
        download_id: i64,
        file_path: &str,
        total_bytes: u64,
    ) -> anyhow::Result<FileWriterHandle> {
        let file_exists = fs::metadata(file_path).await.is_ok();

        let file = OpenOptions::new()
            .write(true)
            .create(true)
            .open(file_path)
            .await?;

        if !file_exists && total_bytes > 0 {
            file.set_len(total_bytes).await?;
        }

        let (tx, mut rx) = mpsc::channel::<WriteCommand>(WRITER_QUEUE_CAPACITY);
        let io_error = Arc::new(OnceLock::new());
        let io_error_writer = Arc::clone(&io_error);

        spawn!("file_writer", {
            let mut writer = BufWriter::with_capacity(BUFFER_CAPACITY, file);
            let mut cursor: Option<u64> = None;

            while let Some(cmd) = rx.recv().await {
                match cmd {
                    WriteCommand::Chunk {
                        chunk_index,
                        offset,
                        bytes,
                    } => {
                        let bytes_len = bytes.len() as u64;
                        if bytes_len == 0 {
                            continue;
                        }

                        let io_res: std::io::Result<()> = async {
                            if cursor != Some(offset) {
                                writer.seek(SeekFrom::Start(offset)).await?;
                            }
                            writer.write_all(&bytes).await?;
                            cursor = Some(offset + bytes_len);

                            // Only flush to the OS when the queue drains so sequential packets
                            // coalesce inside the 256 KiB buffer instead of syscall-thrashing.
                            if rx.is_empty() {
                                writer.flush().await?;
                            }
                            Ok(())
                        }
                        .await;

                        if let Err(err) = io_res {
                            let msg = err.to_string();
                            let _ = io_error_writer.set(msg.clone());
                            Emitter::emit_error(format!("Disk write error: {msg}"));
                            rx.close();
                            break;
                        }

                        let _ = dispatch!(
                            registry,
                            UpdateDiskReport,
                            (download_id, chunk_index, bytes_len)
                        );

                        let _ = dispatch!(
                            registry,
                            UpdateChunkBufferReport,
                            (download_id, chunk_index, bytes)
                        );
                    }
                    WriteCommand::Flush(done) => {
                        let res = writer.flush().await.map_err(|e| e.to_string());
                        if let Err(ref msg) = res {
                            let _ = io_error_writer.set(msg.clone());
                        }
                        let _ = done.send(res);
                    }
                }
            }

            let _ = writer.flush().await;
        });

        Ok(FileWriterHandle { tx, io_error })
    }
}
