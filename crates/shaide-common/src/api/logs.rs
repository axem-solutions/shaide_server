use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

#[derive(Serialize, Deserialize, ToSchema, IntoParams)]
pub struct LogsQuery {
    pub start_date: String,
    pub end_date: String,
}

#[derive(Serialize, Deserialize, ToSchema)]
pub struct LogFileContent {
    pub file_name: String,
    pub content: String,
}

#[derive(Serialize, Deserialize, ToSchema)]
pub struct LogFilesResponse {
    pub log_files: Vec<LogFileContent>,
}
