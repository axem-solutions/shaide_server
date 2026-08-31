use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Serialize, Deserialize, ToSchema)]
pub struct RagDeleteCollectionRequest {
    pub collection_name: String,
}

#[derive(Serialize, Deserialize, ToSchema)]
pub struct RagCollectionParameterRequest {
    pub collection_name: String,
    pub model_id: i64,
}

#[derive(Serialize, Deserialize, ToSchema)]
pub struct RemoteSearchRequest {
    pub query: String,
    pub min_score: f32,
    pub max_results: u64,
    pub workspace_id: String,
    pub identifier: Option<String>,
    pub r#type: Option<String>,
    pub embedding_model_id: i64,
}

#[derive(Serialize, Deserialize, ToSchema)]
pub struct SearchResult {
    pub score: f32,
    pub file_path: String,
    pub code_chunk: String,
    pub start_line: u64,
    pub end_line: u64,
}

#[derive(Serialize, Deserialize, ToSchema)]
pub struct RemoteSearchResponse {
    pub results: Vec<SearchResult>,
}
