pub mod anthropic;
pub mod vertex;

use anyhow::Result;
use async_openai::{error::OpenAIError, types::chat::CreateChatCompletionRequest};
use google_cloud_auth::errors::CredentialsError;
use hyper::StatusCode;
use reqwest_eventsource::{Event, EventSource, RequestBuilderExt};
use serde::{Deserialize, Serialize};
use shaide_common::open_ai_types::{
    ShaideChatCompletionResponseStream, ShaideChatCompletionStreamEvent,
};
use shaide_db::{ModelDAO, models::ApiSchemaDao};
use thiserror::Error;
use tokio::sync::mpsc::UnboundedSender;
use tokio_stream::StreamExt;
use tracing::error;

use crate::{
    error::ShaideError,
    providers::gcp::{
        GcpClient, GcpError,
        chat::{
            anthropic::{anthropic_stream_handler, map_open_ai_request_to_anthropic},
            vertex::{map_open_ai_request_to_vertex, vertex_stream_handler},
        },
    },
    utils::openai_stream_handler::openai_stream_handler,
};

/// Maximum number of retries after the initial chat request is rate-limited.
///
/// Each retry observes the model's shared exponential-backoff deadline before
/// making another request.
const BACKOFF_ATTEMPT_LIMIT: usize = 2;

#[derive(Debug, Error)]
enum ShaideChatStreamCreationError {
    // Gcp returned a 429. In this case we try to backoff exponentially
    #[error("Too many requests were received. Need to backoff for a bit")]
    TooManyRequests,

    // Other error has been reached when extracting the first error
    #[error("Other event source error has been reached")]
    OtherEventSourceError(Box<reqwest_eventsource::Error>),

    // The credentials from GCP errors out
    #[error("Credentials error")]
    CredentialsError(#[from] CredentialsError),
}

impl From<reqwest_eventsource::Error> for ShaideChatStreamCreationError {
    fn from(value: reqwest_eventsource::Error) -> Self {
        Self::OtherEventSourceError(Box::new(value))
    }
}

type StreamEventType = Option<Result<Event, reqwest_eventsource::Error>>;
type StreamSenderType = UnboundedSender<Result<ShaideChatCompletionStreamEvent, OpenAIError>>;

const MAX_ERROR_BODY_CHARS: usize = 4000;

fn map_chat_request_mapping_error(err: ShaideError) -> GcpError {
    match err {
        ShaideError::BadRequest(reason) => GcpError::BadRequest(reason),
        ShaideError::BadRequestWithMessage(reason) => GcpError::BadRequest(reason),
        other => GcpError::BadRequest(other.to_string()),
    }
}

fn truncate_error_body(body: String) -> String {
    if body.chars().count() <= MAX_ERROR_BODY_CHARS {
        return body;
    }

    let truncated: String = body.chars().take(MAX_ERROR_BODY_CHARS).collect();
    format!("{truncated}...[truncated]")
}

async fn extract_response_error_body(response: reqwest::Response) -> String {
    match response.text().await {
        Ok(body) if body.trim().is_empty() => "<empty body>".to_owned(),
        Ok(body) => truncate_error_body(body),
        Err(err) => format!("<failed to read response body: {err}>"),
    }
}

#[derive(Deserialize)]
struct VertexErrorBody {
    error: VertexErrorDetail,
}

#[derive(Deserialize)]
struct VertexErrorDetail {
    message: String,
    status: String,
}

fn try_extract_vertex_bad_request(body: &str) -> Option<String> {
    let errors: Vec<VertexErrorBody> = serde_json::from_str(body).ok()?;
    let first = errors.into_iter().next()?;
    if first.error.status == "INVALID_ARGUMENT" {
        Some(first.error.message)
    } else {
        None
    }
}

impl GcpClient {
    async fn get_completion_stream<
        B: Serialize,
        H: AsyncFn(EventSource, StreamEventType, StreamSenderType),
    >(
        &self,
        body: &B,
        model: &ModelDAO,
        stream_handler: H,
    ) -> Result<ShaideChatCompletionResponseStream, ShaideChatStreamCreationError> {
        self.backoff_bucket.wait_model_backoff(&model.name).await;
        let api_key = self.access_token().await?;
        let mut event_source = self
            .client
            .post(&model.chat_completions_endpoint)
            .json(body)
            .bearer_auth(api_key)
            .eventsource()
            .unwrap();

        let first_event: Option<Result<Event, reqwest_eventsource::Error>> =
            event_source.next().await;

        // Check for specific error status codes first
        match first_event {
            // NOTE: We get a too many requests error, we should handle this or error out
            Some(Err(reqwest_eventsource::Error::InvalidStatusCode(
                StatusCode::TOO_MANY_REQUESTS,
                _,
            ))) => {
                self.backoff_bucket
                    .increase_model_wait_time(&model.name)
                    .await;
                Err(ShaideChatStreamCreationError::TooManyRequests)
            }
            Some(Err(error)) => {
                Err(error.into())
                // Unhandled errro has occured
            }
            _ => {
                let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
                self.backoff_bucket.clear_model_backoff(&model.name).await;
                stream_handler(event_source, first_event, tx).await;
                Ok(Box::pin(
                    tokio_stream::wrappers::UnboundedReceiverStream::new(rx),
                ))
            }
        }
    }

    pub async fn stream_chat_completion<
        B: Serialize,
        H: AsyncFn(EventSource, StreamEventType, StreamSenderType) + Clone,
    >(
        &self,
        body: B,
        model: &ModelDAO,
        stream_handler: H,
    ) -> Result<ShaideChatCompletionResponseStream, GcpError> {
        let mut backoff_attempts_count = 0;
        let mut stream = self
            .get_completion_stream(&body, model, stream_handler.clone())
            .await;
        loop {
            match stream {
                Ok(stream) => return Ok(stream),
                Err(ShaideChatStreamCreationError::TooManyRequests) => {
                    if backoff_attempts_count >= BACKOFF_ATTEMPT_LIMIT {
                        return Err(GcpError::MaxRetriesHaveBeenAchieved {
                            service: "chat completion".into(),
                        });
                    }
                    backoff_attempts_count += 1;
                }
                Err(ShaideChatStreamCreationError::OtherEventSourceError(error)) => match *error {
                    reqwest_eventsource::Error::InvalidContentType(header_value, response) => {
                        let status = response.status();
                        let url = response.url().to_string();
                        let body = extract_response_error_body(response).await;
                        error!(
                            content_type = ?header_value,
                            url = %url,
                            status_code = %status,
                            body = %body,
                            "Unexpected upstream Content-Type"
                        );
                        return Err(GcpError::UnexpectedResponse {
                            status_code: status,
                            response_body: format!(
                                "Invalid Content-Type: {:?}, URL: {}, Body: {}",
                                header_value, url, body
                            ),
                            service: "chat".into(),
                        });
                    }
                    reqwest_eventsource::Error::InvalidStatusCode(status_code, response) => {
                        let url = response.url().to_string();
                        let body = extract_response_error_body(response).await;
                        if status_code == StatusCode::BAD_REQUEST
                            && let Some(message) = try_extract_vertex_bad_request(&body)
                        {
                            return Err(GcpError::BadRequest(message));
                        }
                        error!(
                            status_code = %status_code,
                            url = %url,
                            body = %body,
                            "Upstream chat completion returned non-2xx status"
                        );
                        return Err(GcpError::UnexpectedResponse {
                            status_code,
                            response_body: format!(
                                "Status: {}, URL: {}, Body: {}",
                                status_code, url, body
                            ),
                            service: "chat".into(),
                        });
                    }
                    error => {
                        error!("Unhandled event source error while creating chat stream: {error}");
                        return Err(GcpError::UnexpectedResponse {
                            status_code: StatusCode::BAD_GATEWAY,
                            response_body: format!("Unhandled event source error: {error}"),
                            service: "chat".into(),
                        });
                    }
                },
                Err(ShaideChatStreamCreationError::CredentialsError(err)) => return Err(err.into()),
            }
            // reconstruct the stream and try again
            stream = self
                .get_completion_stream(&body, model, stream_handler.clone())
                .await
        }
    }

    pub async fn stream_gcp_completion_response(
        &self,
        request: CreateChatCompletionRequest,
        model: &ModelDAO,
    ) -> Result<ShaideChatCompletionResponseStream, GcpError> {
        match model.api_schema {
            ApiSchemaDao::Anthropic => {
                let max_tokens = model.max_generated_tokens as u32;
                let request = map_open_ai_request_to_anthropic(request, max_tokens, true)
                    .map_err(map_chat_request_mapping_error)?;
                let model_name = model.name.clone();
                let handler = async move |event_source, event, tx| {
                    { anthropic_stream_handler(event_source, event, tx, model_name.clone()) }.await
                };
                Ok(self.stream_chat_completion(request, model, handler).await?)
            }
            ApiSchemaDao::Vertex => {
                let max_tokens = model.max_generated_tokens as u32;
                let request = map_open_ai_request_to_vertex(request, max_tokens)
                    .map_err(map_chat_request_mapping_error)?;
                Ok(self
                    .stream_chat_completion(request, model, vertex_stream_handler)
                    .await?)
            }
            ApiSchemaDao::OpenAI => Ok(self
                .stream_chat_completion(request, model, openai_stream_handler)
                .await?),
        }
    }
}
