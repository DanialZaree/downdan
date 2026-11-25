use crate::registry::Registry;

pub struct EngineRepository;

impl EngineRepository {
    pub async fn set(download_id: i64, engine: &str, gid: Option<&str>) -> Result<(), sqlx::Error> {
        let pool = Registry::get_pool();
        sqlx::query(
            r#"
            INSERT INTO download_engines (download_id, engine, gid)
            VALUES (?, ?, ?)
            ON CONFLICT(download_id) DO UPDATE SET
                engine = excluded.engine,
                gid = COALESCE(excluded.gid, download_engines.gid)
            "#,
        )
        .bind(download_id)
        .bind(engine)
        .bind(gid)
        .execute(pool)
        .await
        .map(|_| ())
    }

    pub async fn update_gid(download_id: i64, gid: &str) -> Result<(), sqlx::Error> {
        let pool = Registry::get_pool();
        sqlx::query("UPDATE download_engines SET gid = ? WHERE download_id = ?")
            .bind(gid)
            .bind(download_id)
            .execute(pool)
            .await
            .map(|_| ())
    }

    pub async fn get(download_id: i64) -> Result<Option<(String, Option<String>)>, sqlx::Error> {
        let pool = Registry::get_pool();
        sqlx::query_as::<_, (String, Option<String>)>(
            "SELECT engine, gid FROM download_engines WHERE download_id = ?",
        )
        .bind(download_id)
        .fetch_optional(pool)
        .await
    }
}
