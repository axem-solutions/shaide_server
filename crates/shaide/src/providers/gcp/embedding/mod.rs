use google_cloud_auth::errors::CredentialsError;
use http::StatusCode;
use serde::{Deserialize, Serialize};
use shaide_common::api::embedding::CodeBlock;
use shaide_db::embedding_models::EmbeddingModelDao;
use thiserror::Error;
use tracing::error;

/// Maximum number of retries after the initial embedding request is
/// rate-limited.
///
/// Embedding retries are currently immediate; unlike chat retries, they do not
/// yet use the shared exponential-backoff bucket.
const BACKOFF_ATTEMPT_LIMIT: usize = 2;

use crate::{
    providers::gcp::{GcpClient, GcpError},
    services::embedding::{EmbeddedSnippet, EmbeddedSnippets},
};

#[derive(Serialize, Deserialize)]
struct GeminiEmbeddingContent {
    content: String,
}

#[derive(Serialize, Deserialize)]
struct GeminiParameters {
    #[serde(rename = "autoTruncate")]
    auto_truncate: bool,
}

#[derive(Serialize, Deserialize)]
struct GeminiEmbeddingRequest {
    instances: Vec<GeminiEmbeddingContent>,
    parameters: GeminiParameters,
}

#[derive(Serialize, Deserialize)]
pub struct EmbeddingStatistics {
    truncated: bool,
    token_count: u32,
}

#[derive(Serialize, Deserialize)]
pub struct Embedding {
    pub statistics: EmbeddingStatistics,
    pub values: Vec<f32>,
}
#[derive(Serialize, Deserialize)]
pub struct EmbeddingPrediction {
    pub embeddings: Embedding,
}

#[derive(Serialize, Deserialize, Default)]
pub struct EmbeddingResponse {
    pub predictions: Vec<EmbeddingPrediction>,
}

#[derive(Debug, Error)]
enum EmbeddingError {
    // Too many request are sent, probably recoverable
    #[error("Too many requests were received. Need to backoff for a bit")]
    TooManyRequests,

    #[error("Http error: {0}")]
    RequestError(#[from] reqwest::Error),

    #[error("Deserialization error: {0}")]
    DeserializationError(#[from] serde_json::Error),

    // The credentials from GCP errors out
    #[error("Credentials error")]
    CredentialsError(#[from] CredentialsError),

    #[error("Unexpected response")]
    UnexpectedResponse {
        status_code: hyper::StatusCode,
        response_body: String,
    },
}

impl GcpClient {
    // TODO: this does not do exponential backoffs right now
    async fn get_embedding(
        &self,
        model: &EmbeddingModelDao,
        body: &GeminiEmbeddingRequest,
    ) -> Result<EmbeddingResponse, EmbeddingError> {
        let api_key = self.access_token().await?;
        let request = self
            .client
            .post(&model.url)
            .bearer_auth(api_key)
            .json(body)
            .header("Content-Type", "application/json; charset=utf-8")
            .build()?;
        let response = self.client.execute(request).await;
        match response {
            Ok(response) => {
                let status_code = response.status();
                let response = response.text().await?;
                match status_code.as_u16() {
                    200..300 => {
                        let response: EmbeddingResponse = serde_json::from_str(&response)?;
                        Ok(response)
                    }
                    status_code => Err(EmbeddingError::UnexpectedResponse {
                        status_code: hyper::StatusCode::from_u16(status_code).unwrap(),
                        response_body: response,
                    }),
                }
            }
            Err(error) => {
                let Some(status_code) = error.status() else {
                    return Err(EmbeddingError::RequestError(error));
                };
                match status_code {
                    StatusCode::TOO_MANY_REQUESTS => Err(EmbeddingError::TooManyRequests),
                    _ => Err(EmbeddingError::RequestError(error)),
                }
            }
        }
    }

    pub async fn embed(
        &self,
        embedding_model: &EmbeddingModelDao,
        texts: Vec<String>,
    ) -> Result<Vec<Vec<f32>>, GcpError> {
        let instances = texts
            .into_iter()
            .map(|content| GeminiEmbeddingContent { content })
            .collect();
        let body = GeminiEmbeddingRequest {
            instances,
            parameters: GeminiParameters {
                auto_truncate: true,
            },
        };
        let mut backoff_attempts_count = 0;
        let mut response = self.get_embedding(embedding_model, &body).await;
        loop {
            match response {
                Ok(response) => {
                    return Ok(response
                        .predictions
                        .iter()
                        .map(|p| p.embeddings.values.clone())
                        .collect());
                }
                Err(EmbeddingError::CredentialsError(err)) => return Err(err.into()),
                Err(EmbeddingError::TooManyRequests) => {
                    if backoff_attempts_count >= BACKOFF_ATTEMPT_LIMIT {
                        return Err(GcpError::MaxRetriesHaveBeenAchieved {
                            service: "embedding".into(),
                        });
                    }
                    backoff_attempts_count += 1;
                }
                Err(EmbeddingError::RequestError(error)) => {
                    // TODO: I have no idea how this can happen?
                    error!(error = ?error, "GCP embedding request failed");
                    return Err(GcpError::Request(error));
                }
                Err(EmbeddingError::DeserializationError(error)) => {
                    error!(error = ?error, "Failed to deserialize GCP embedding response");
                    return Err(GcpError::DeserializationError(error));
                }
                Err(EmbeddingError::UnexpectedResponse {
                    status_code,
                    response_body,
                }) => {
                    return Err(GcpError::UnexpectedResponse {
                        status_code,
                        response_body,
                        service: "embedding".into(),
                    });
                }
            }
            response = self.get_embedding(embedding_model, &body).await;
        }
    }

    pub async fn embed_snippets(
        &self,
        embedding_model: EmbeddingModelDao,
        snippets: Vec<CodeBlock>,
    ) -> Result<EmbeddedSnippets, GcpError> {
        let texts = snippets
            .iter()
            .map(|snippet| snippet.content.clone())
            .collect();
        let predictions = self.embed(&embedding_model, texts).await?;
        // Vertex AI returns predictions in the same order as the request instances.
        assert_eq!(
            predictions.len(),
            snippets.len(),
            "GCP returned a different number of embeddings than requested"
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
