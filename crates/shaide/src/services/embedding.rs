use std::hash::Hasher;

use fnv::FnvHasher;
use qdrant_client::qdrant::PointStruct;
use shaide_common::api::embedding::CodeBlock;
use shaide_db::embedding_models::EmbeddingModelDao;

use crate::{
    error::ShaideError,
    providers::{azure::get_azure_client, gcp::get_gcp_client, shaide::get_axem_client},
};

pub struct EmbeddedSnippet {
    pub snippet: CodeBlock,
    pub prediction: Vec<f32>,
}

pub struct EmbeddedSnippets {
    pub snippets: Vec<EmbeddedSnippet>,
    pub vector_size: u64,
}

pub fn hash_content(content: &str) -> u64 {
    let mut h = FnvHasher::default(); // FNV-1a
    h.write(content.as_bytes());
    h.finish()
}

impl EmbeddedSnippet {
    pub fn new(snippet: CodeBlock, prediction: Vec<f32>) -> Self {
        Self {
            snippet,
            prediction,
        }
    }

    pub fn into_point_struct(self) -> PointStruct {
        let EmbeddedSnippet {
            snippet,
            prediction,
        } = self;
        let CodeBlock {
            file_path,
            start_line,
            end_line,
            content,
            identifier,
            r#type,
        } = snippet;
        let hash = hash_content(&content);
        let payload = [
            ("text", content.into()),
            ("file_path", file_path.into()),
            ("start_line", start_line.into()),
            ("end_line", end_line.into()),
            ("identifier", identifier.into()),
            ("type", r#type.into()),
        ];
        PointStruct::new(hash, prediction, payload)
    }
}

pub async fn embed(
    embedding_model: &EmbeddingModelDao,
    texts: Vec<String>,
) -> Result<Vec<Vec<f32>>, ShaideError> {
    let embeddings = match embedding_model.platform.as_deref() {
        Some("vertex") => {
            let gcp_client = get_gcp_client().await?;
            gcp_client.embed(embedding_model, texts).await?
        }
        Some("axem") => {
            let axem_client = get_axem_client().await;
            axem_client.embed(embedding_model, texts).await?
        }
        Some("foundry") => {
            let azure_client = get_azure_client().await?;
            azure_client.embed(embedding_model, texts).await?
        }
        Some(platform) => return Err(ShaideError::unsupported_platform(platform.to_owned())),
        None => return Err(ShaideError::unsupported_platform("none".to_owned())),
    };
    Ok(embeddings)
}
