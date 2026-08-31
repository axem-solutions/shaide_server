use serde::{Deserialize, Serialize};
use shaide_common::api::embedding::CodeBlock;
use shaide_db::embedding_models::EmbeddingModelDao;
use tracing::debug;

use crate::{
    providers::azure::{AzureClient, AzureError},
    services::embedding::{EmbeddedSnippet, EmbeddedSnippets},
};

#[derive(Serialize)]
struct AzureEmbeddingRequest {
    input: Vec<String>,
    model: String,
    encoding_format: &'static str,
}

#[derive(Deserialize)]
struct AzureEmbeddingResponse {
    data: Vec<AzureEmbeddingData>,
}

#[derive(Deserialize)]
struct AzureEmbeddingData {
    index: usize,
    embedding: Vec<f32>,
}

impl AzureClient {
    pub async fn embed(
        &self,
        embedding_model: &EmbeddingModelDao,
        texts: Vec<String>,
    ) -> Result<Vec<Vec<f32>>, AzureError> {
        let input_count = texts.len();
        let request = AzureEmbeddingRequest {
            input: texts,
            model: embedding_model.name.clone(),
            encoding_format: "float",
        };
        let token = self.inference_access_token().await?;
        let response = self
            .client
            .post(&embedding_model.url)
            .bearer_auth(token)
            .json(&request)
            .send()
            .await
            .map_err(AzureError::Request)?;
        let status_code = response.status();
        let response_body = response.text().await.map_err(AzureError::Request)?;
        debug!(
            endpoint = %embedding_model.url,
            model = %embedding_model.name,
            status_code = %status_code,
            input_count,
            response_body_len = response_body.len(),
            "Received embedding response from Azure provider"
        );
        if !status_code.is_success() {
            Err(AzureError::HttpError {
                status_code,
                response_body,
            })
        } else {
            let mut response: AzureEmbeddingResponse =
                serde_json::from_str(&response_body).map_err(AzureError::Deserialization)?;
            response.data.sort_unstable_by_key(|item| item.index);
            assert!(
                response
                    .data
                    .iter()
                    .enumerate()
                    .all(|(index, item)| item.index == index),
                "Azure returned invalid embedding indices"
            );
            Ok(response
                .data
                .into_iter()
                .map(|item| item.embedding)
                .collect())
        }
    }

    pub async fn embed_snippets(
        &self,
        embedding_model: EmbeddingModelDao,
        snippets: Vec<CodeBlock>,
    ) -> Result<EmbeddedSnippets, AzureError> {
        let texts = snippets
            .iter()
            .map(|snippet| snippet.content.clone())
            .collect();
        let predictions = self.embed(&embedding_model, texts).await?;
        assert_eq!(
            predictions.len(),
            snippets.len(),
            "Azure returned a different number of embeddings than requested"
        );
        let snippets = predictions
            .into_iter()
            .zip(snippets)
            .map(|(prediction, snippet)| EmbeddedSnippet::new(snippet, prediction))
            .collect();
        Ok(EmbeddedSnippets {
            snippets,
            vector_size: embedding_model.vector_size as u64,
        })
    }
}
