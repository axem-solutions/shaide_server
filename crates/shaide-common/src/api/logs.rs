use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

#[derive(Serialize, Deserialize, ToSchema, IntoParams)]
pub struct LogsQuery {
    start_date: String,
    end_date: String,
}

#[derive(Serialize, Deserialize, ToSchema)]
pub struct LogFileContent {
    file_name: String,
    content: String,
}

#[derive(Serialize, Deserialize, ToSchema)]
pub struct LogFilesResponse {
    log_files: Vec<LogFileContent>,
}
