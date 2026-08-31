use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Serialize, Deserialize, ToSchema, Clone, Debug, Default)]
pub struct ListEmbeddingModel {
    pub id: i64,
    pub name: String,
}

#[derive(Serialize, Deserialize, ToSchema, Clone, Debug, Default)]
pub struct ListEmbeddingModelsResponse {
    pub models: Vec<ListEmbeddingModel>,
}

#[derive(Serialize, Deserialize, ToSchema, Clone, Debug, Default)]
pub struct InsertEmbeddingModelRequest {
    pub url: String,
    pub name: String,
    pub vector_size: i64,
    pub platform: Option<String>,
    pub api_schema: Option<String>,
    pub max_embedding_model_text_len: Option<i64>,
}

#[derive(Serialize, Deserialize, ToSchema, Clone, Debug, Default)]
pub struct InsertEmbeddingModelResponse {
    pub id: i64,
}

#[derive(Serialize, Deserialize, ToSchema, Clone, Debug, Default)]
pub struct DeleteEmbeddingModelRequest {
    pub id: i64,
}

#[derive(Serialize, Deserialize, ToSchema, Clone, Debug, Default)]
pub struct DeleteEmbeddingModelResponse {}
