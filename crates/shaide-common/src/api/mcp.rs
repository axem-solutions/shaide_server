use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub enum McpServerStatusResponse {
    Starting,
    Running,
    Restarting,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct McpServerResponse {
    pub name: String,
    pub status: McpServerStatusResponse,
    pub url: String,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct McpServerListResponse {
    pub servers: Vec<McpServerResponse>,
}
