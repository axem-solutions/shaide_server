use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Deserialize, ToSchema)]
pub struct EmbedRequest {
    pub model_id: i64,
    pub text: String,
    pub collection_name: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CodeBlock {
    pub file_path: String,
    pub start_line: i64,
    pub end_line: i64,
    pub content: String,
    pub r#type: String,
    pub identifier: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct RemoteIndexRequest {
    pub inputs: Vec<CodeBlock>,
    pub embedding_model_id: i64,
    pub workspace_id: String,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct EmbedCodeResponse {}

#[derive(Debug, Deserialize, ToSchema)]
pub struct RemoveCodeFilesRequest {
    pub workspace_id: String,
    pub file_paths: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct RemoveCodeFilesResponse {}
