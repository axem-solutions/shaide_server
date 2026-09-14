use std::path::PathBuf;

use anyhow::{Result, bail};
use sqlx::ConnectOptions;
use sqlx_turso::TursoConnectOptions;

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../shaide-db/migrations");

#[tokio::main]
async fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let action = args.next();
    let database = args.next().map(PathBuf::from);
    let (Some(action), Some(database)) = (action, database) else {
        bail!("usage: migrate <run|revert> <database-file>");
    };
    if !matches!(action.as_str(), "run" | "revert") || args.next().is_some() {
        bail!("usage: migrate <run|revert> <database-file>");
    }

    if let Some(parent) = database
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
    {
        tokio::fs::create_dir_all(parent).await?;
    }
    let mut conn = TursoConnectOptions::new()
        .filename(database)
        .create_if_missing(action == "run")
        .connect()
        .await?;

    if action == "run" {
        MIGRATOR.run(&mut conn).await?;
    } else {
        use sqlx::migrate::Migrate;

        let applied = conn.list_applied_migrations(&MIGRATOR.table_name).await?;
        let target = applied.iter().rev().nth(1).map_or(0, |m| m.version);
        MIGRATOR.undo(&mut conn, target).await?;
    }
    sqlx::Connection::close(conn).await?;
    Ok(())
}
