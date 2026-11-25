use crate::{
    models::{DownloadChunk, UpdateChunk},
    registry::Registry,
};

pub struct ChunkRepository;

impl ChunkRepository {
    pub async fn find_all(download_id: i64) -> Result<Vec<DownloadChunk>, sqlx::Error> {
        let pool = Registry::get_pool();
        sqlx::query_as!(
            DownloadChunk,
            "SELECT * FROM download_chunks WHERE download_id = ?;",
            download_id
        )
        .fetch_all(pool)
        .await
    }

    pub async fn create_all(
        download_id: i64,
        ranges: Vec<(u64, u64)>,
    ) -> Result<(), Vec<sqlx::Error>> {
        let mut errors = Vec::new();

        for (index, range) in ranges.iter().enumerate() {
            let (start, end) = range;
            if let Err(err) =
                Self::create(download_id, index as i64, (*start as i64, *end as i64)).await
            {
                errors.push(err)
            }
        }

        if errors.is_empty() {
            return Ok(());
        }

        Err(errors)
    }

    pub async fn create(
        download_id: i64,
        index: i64,
        range: (i64, i64),
    ) -> Result<(), sqlx::Error> {
        let pool = Registry::get_pool();

        let (start, end) = range;

        sqlx::query!(
            r#"
            INSERT INTO download_chunks (download_id, chunk_index, start_byte, end_byte)
            VALUES (?, ?, ?, ?)
            "#,
            download_id,
            index,
            start,
            end
        )
        .execute(pool)
        .await
        .map(|_| ())
    }

    pub async fn update_end_byte(
        download_id: i64,
        chunk_index: i64,
        end_byte: i64,
    ) -> Result<(), sqlx::Error> {
        let pool = Registry::get_pool();
        sqlx::query(
            "UPDATE download_chunks SET end_byte = ? WHERE download_id = ? AND chunk_index = ?",
        )
        .bind(end_byte)
        .bind(download_id)
        .bind(chunk_index)
        .execute(pool)
        .await
        .map(|_| ())
    }

    pub async fn update_all(
        download_id: i64,
        chunks: Vec<UpdateChunk>,
    ) -> Result<(), Vec<sqlx::Error>> {
        let mut results = Vec::new();

        for chunk in chunks {
            if let Err(err) = Self::update(download_id, chunk).await {
                results.push(err);
            }
        }

        if results.is_empty() {
            return Ok(());
        }

        Err(results)
    }

    pub async fn update(download_id: i64, chunk: UpdateChunk) -> Result<(), sqlx::Error> {
        let pool = Registry::get_pool();
        let bytes = chunk.downloaded_bytes.unwrap_or(0);

        match &chunk.expected_hash {
            Some(hash) => {
                sqlx::query(
                    "UPDATE download_chunks SET expected_hash = ?, downloaded_bytes = ? WHERE download_id = ? AND chunk_index = ?",
                )
                .bind(hash)
                .bind(bytes)
                .bind(download_id)
                .bind(chunk.chunk_index)
                .execute(pool)
                .await
                .map(|_| ())
            }
            None => {
                sqlx::query(
                    "UPDATE download_chunks SET expected_hash = NULL, downloaded_bytes = ? WHERE download_id = ? AND chunk_index = ?",
                )
                .bind(bytes)
                .bind(download_id)
                .bind(chunk.chunk_index)
                .execute(pool)
                .await
                .map(|_| ())
            }
        }
    }
}
