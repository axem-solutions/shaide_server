use serde::{Deserialize, Serialize};
use shaide_common::api::embedding::CodeBlock;
use shaide_db::embedding_models::EmbeddingModelDao;
use tracing::debug;

use crate::{
    providers::shaide::{AxemClient, ShaideProviderError},
    services::embedding::{EmbeddedSnippet, EmbeddedSnippets},
};

#[derive(Deserialize)]
struct OpenAIEmbeddingResponse {
    data: Vec<OpenAIEmbeddingData>,
    model: String,
}

#[derive(Serialize)]
struct OpenAIEmbeddingRequest {
    input: Vec<String>,
    model: String,
}

#[derive(Deserialize)]
struct OpenAIEmbeddingData {
    index: usize,
    embedding: Vec<f32>,
}

impl AxemClient {
    pub async fn embed(
        &self,
        embedding_model: &EmbeddingModelDao,
        texts: Vec<String>,
    ) -> Result<Vec<Vec<f32>>, ShaideProviderError> {
        let request = OpenAIEmbeddingRequest {
            input: texts,
            model: embedding_model.name.to_string(),
        };
        let response = self
            .client
            .post(embedding_model.url.to_string())
            .json(&request)
            .send()
            .await?;
        let status_code = response.status();
        let response_body = response.text().await?;
        debug!(
            endpoint = %embedding_model.url,
            model = %embedding_model.name,
            status_code = %status_code,
            input_count = request.input.len(),
            response_body_len = response_body.len(),
            "Received embedding response from shaide provider"
        );
        if !status_code.is_success() {
            Err(ShaideProviderError::HttpError {
                status_code,
                response_body,
            })
        } else {
            let mut openai_response: OpenAIEmbeddingResponse =
                serde_json::from_str(&response_body)?;
            debug!(
                endpoint = %embedding_model.url,
                model = %openai_response.model,
                embedding_count = openai_response.data.len(),
                "Parsed embedding response from shaide provider"
            );
            openai_response.data.sort_unstable_by_key(|item| item.index);
            assert!(
                openai_response
                    .data
                    .iter()
                    .enumerate()
                    .all(|(index, item)| item.index == index),
                "Shaide returned invalid embedding indices"
            );
            Ok(openai_response
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
    ) -> Result<EmbeddedSnippets, ShaideProviderError> {
        let texts: Vec<_> = snippets
            .iter()
            .map(|snippet| snippet.content.clone())
            .collect();
        let embeddings = self.embed(&embedding_model, texts).await?;
        assert_eq!(
            embeddings.len(),
            snippets.len(),
            "Shaide returned a different number of embeddings than requested"
        );
        let snippets: Vec<_> = embeddings
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
