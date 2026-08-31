// use std::fs;
use axum::{Json, Router, extract::Query, routing};
use chrono::NaiveDate;
use futures::{StreamExt, stream};
use shaide_common::{
    api::logs::{LogFileContent, LogFilesResponse, LogsQuery},
    path::logs_dir,
};
use shaide_db::DbConn;
use tokio::fs::{self};

use crate::{error::ShaideError, middlewares::Admin};

#[utoipa::path(
    get,
    path = "/v1/logs",
    tag = "logs",
    params(LogsQuery),
    responses((status = 200, description = "Log files", body = LogFilesResponse)),
    security(("bearer_token" = []))
)]
pub async fn logs_handler(
    _admin: Admin,
    Query(LogsQuery {
        start_date,
        end_date,
    }): Query<LogsQuery>,
) -> Result<Json<LogFilesResponse>, ShaideError> {
    let start_date = NaiveDate::parse_from_str(&start_date, "%Y-%m-%d")
        .map_err(|_| ShaideError::bad_request("Could not parse start_date".to_owned()))?;
    let end_date = NaiveDate::parse_from_str(&end_date, "%Y-%m-%d")
        .map_err(|_| ShaideError::bad_request("Could not parse end_date".to_owned()))?;
    let mut candidates = vec![];
    let logs_folder = logs_dir();
    let mut log_folder_read = fs::read_dir(logs_folder).await.unwrap();
    while let Ok(Some(dir_entry)) = log_folder_read.next_entry().await {
        let Some(file_name) = dir_entry.file_name().to_str().map(|s| s.to_owned()) else {
            continue;
        };
        let Ok(log_date) =
            NaiveDate::parse_from_str(&file_name[..10.min(file_name.len())], "%Y-%m-%d")
        else {
            continue;
        };
        if log_date >= start_date && log_date <= end_date {
            candidates.push((file_name, dir_entry.path()));
        }
    }
    let log_files = stream::iter(candidates)
        .map(|(file_name, path)| async move {
            let content = fs::read_to_string(&path).await.unwrap();
            LogFileContent { file_name, content }
        })
        .buffer_unordered(16)
        .collect()
        .await;
    let response = LogFilesResponse { log_files };
    Ok(Json(response))
}

pub fn logs_router(db: DbConn) -> Router {
    Router::new()
        .route("/v1/logs", routing::get(logs_handler))
        .with_state(db)
}
