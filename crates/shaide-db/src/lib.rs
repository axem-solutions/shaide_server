pub mod api_usage;
pub mod daily_usage;
pub mod embedding_models;
pub mod error;
pub mod models;
mod users;

use std::path::Path;

use anyhow::Result;
pub use models::{InsertModelDAO, ModelDAO};
use sqlx_turso::{TursoConnectOptions, TursoPool};
pub use users::{UserDAO, UserRole};

#[derive(Clone)]
pub struct DbConn {
    pub pool: TursoPool,
}

impl DbConn {
    pub async fn new(db_file: &Path) -> Result<Self> {
        tokio::fs::create_dir_all(db_file.parent().unwrap()).await?;

        let options = TursoConnectOptions::new()
            .filename(db_file)
            .create_if_missing(true);
        let pool = TursoPool::connect_with(options).await?;
        sqlx::migrate!("./migrations").run(&pool).await?;
        Ok(Self { pool })
    }
}
