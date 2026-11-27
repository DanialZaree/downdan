use sqlx::{sqlite::SqliteConnectOptions, SqlitePool};
use std::{path::PathBuf, str::FromStr};

impl super::Registry {
    pub fn get_pool() -> &'static SqlitePool {
        &Self::get_state().pool
    }

    pub(super) async fn init_db(path: &Result<PathBuf, tauri::Error>) -> SqlitePool {
        let opts = if cfg!(debug_assertions) {
            if let Ok(raw_url) = std::env::var("DATABASE_URL") {
                SqliteConnectOptions::from_str(&raw_url)
                    .expect("invalid DATABASE_URL")
                    .create_if_missing(true)
                    .foreign_keys(true)
            } else {
                SqliteConnectOptions::new()
                    .filename("database.db")
                    .create_if_missing(true)
                    .foreign_keys(true)
            }
        } else {
            let data_path = path.as_ref().expect("failed to get app data directory");
            if std::fs::metadata(data_path).is_err() {
                std::fs::create_dir_all(data_path)
                    .expect("failed to create danDown data directory");
            }
            let db_file = data_path.join("database.db");
            SqliteConnectOptions::new()
                .filename(db_file)
                .create_if_missing(true)
                .foreign_keys(true)
        };

        let pool = SqlitePool::connect_with(opts).await.unwrap();

        sqlx::migrate!().run(&pool).await.unwrap();

        pool
    }
}
