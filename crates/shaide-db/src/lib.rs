pub mod api_usage;
pub mod daily_usage;
pub mod embedding_models;
pub mod error;
pub mod models;
mod users;

use std::path::Path;

use anyhow::Result;
pub use models::{InsertModelDAO, ModelDAO};
use sqlx::{Pool, Sqlite, SqlitePool, sqlite::SqliteConnectOptions};
pub use users::{Role, UserDAO};

#[derive(Clone)]
pub struct DbConn {
    pub pool: Pool<Sqlite>,
}

impl DbConn {
    pub async fn new(db_file: &Path) -> Result<Self> {
        tokio::fs::create_dir_all(db_file.parent().unwrap()).await?;

        let options = SqliteConnectOptions::new()
            // Reduce SQLITE_BUSY (code 5) errors. Note that the error message "database is locked" should not be confused with SQLITE_LOCKED.
            // For more details, see:
            // 1. https://til.simonwillison.net/sqlite/enabling-wal-mode
            // 2. https://www.sqlite.org/wal.html
            .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
            .filename(db_file)
            .create_if_missing(true);
        let pool = SqlitePool::connect_with(options).await?;
        sqlx::migrate!("./migrations").run(&pool).await?;
        Ok(Self { pool })
    }
}
