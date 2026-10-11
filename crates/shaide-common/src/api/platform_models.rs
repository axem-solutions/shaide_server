use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::{embedding_models::InsertEmbeddingModelRequest, models::CreateModelRequest};

/// Registration-ready settings discovered from the platform. Discovery never writes to the DB.
#[derive(Serialize, Deserialize, ToSchema, Clone, Debug)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PlatformModel {
    Generative(Box<CreateModelRequest>),
    Embedding(InsertEmbeddingModelRequest),
}

impl PlatformModel {
    pub fn key(&self) -> (&'static str, &str) {
        match self {
            Self::Generative(model) => ("generative", &model.name),
            Self::Embedding(model) => ("embedding", &model.name),
        }
    }
}

#[derive(Serialize, Deserialize, ToSchema, Clone, Debug)]
pub struct PlatformModelWarning {
    /// Kubernetes namespace/Service, or a served name for a duplicate-name conflict.
    pub source: String,
    pub message: String,
}

#[derive(Serialize, Deserialize, ToSchema, Clone, Debug, Default)]
pub struct PlatformModelsResponse {
    pub models: Vec<PlatformModel>,
    pub warnings: Vec<PlatformModelWarning>,
}
