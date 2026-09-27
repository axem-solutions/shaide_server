use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Serialize, Deserialize, ToSchema, Clone, Debug, Default)]
pub struct ListEmbeddingModel {
    pub id: i64,
    pub name: String,
    /// Daily input tokens each user may embed with this model; `null` is unlimited.
    #[serde(default)]
    pub daily_input_token_limit: Option<i64>,
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
    /// Daily input tokens each user may embed with this model; omitted or
    /// `null` is unlimited.
    #[serde(default)]
    pub daily_input_token_limit: Option<i64>,
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

/// Sets or removes the daily input token limit of an embedding model.
#[derive(Serialize, Deserialize, ToSchema, Clone, Debug, Default)]
pub struct SetEmbeddingModelLimitRequest {
    pub name: String,
    /// The new limit. `null` removes it, making the model unlimited.
    pub daily_input_token_limit: Option<i64>,
}
