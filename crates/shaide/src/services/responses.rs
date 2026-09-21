use std::pin::Pin;

use async_openai::types::chat::CompletionUsage;
use futures::{Stream, StreamExt};
use reqwest::RequestBuilder;
use reqwest_eventsource::{Event as UpstreamEvent, RequestBuilderExt};
use serde_json::Value;
use shaide_common::open_ai_types::ShaideCreateResponse;
use shaide_db::ModelDAO;
use tracing::warn;

use crate::{
    error::ShaideError,
    providers::{
        azure::{AzureError, get_azure_client},
        gcp::{GcpError, get_gcp_client},
        shaide::{ShaideProviderError, get_axem_client},
    },
};

/// A Responses API stream event forwarded verbatim from the upstream provider.
///
/// Events are kept as raw JSON instead of typed structs so that event types and fields unknown
/// to `async-openai` (e.g. `end_turn`, provider specific events) reach clients such as Codex
/// unchanged.
#[derive(Debug, Clone)]
pub struct RawResponseEvent {
    pub event_type: String,
    pub data: Value,
}

impl RawResponseEvent {
    /// Token usage carried by terminal events (`response.completed`, `response.incomplete`,
    /// `response.failed`).
    pub fn usage(&self) -> Option<CompletionUsage> {
        self.data.get("response").and_then(response_usage)
    }
}

pub type RawResponseStream = Pin<Box<dyn Stream<Item = Result<RawResponseEvent, String>> + Send>>;

pub enum ProviderResponse {
    Json(Box<Value>),
    Stream(RawResponseStream),
}

pub async fn create_response(
    request: ShaideCreateResponse,
    model: &ModelDAO,
) -> Result<ProviderResponse, ShaideError> {
    let should_stream = request.stream.unwrap_or(false);
    let request_builder = response_request(&request, model).await?;
    if should_stream {
        Ok(ProviderResponse::Stream(
            response_stream(request_builder, model.platform.as_deref()).await?,
        ))
    } else {
        Ok(ProviderResponse::Json(Box::new(
            json_response(request_builder, model.platform.as_deref()).await?,
        )))
    }
}

/// Extracts token usage from a Responses API `response` object.
pub fn response_usage(response: &Value) -> Option<CompletionUsage> {
    let usage = response.get("usage")?;
    let prompt_tokens = u32::try_from(usage.get("input_tokens")?.as_u64()?).ok()?;
    let completion_tokens = u32::try_from(usage.get("output_tokens")?.as_u64()?).ok()?;
    Some(CompletionUsage {
        prompt_tokens,
        completion_tokens,
        total_tokens: prompt_tokens.saturating_add(completion_tokens),
        prompt_tokens_details: None,
        completion_tokens_details: None,
    })
}

async fn response_request(
    request: &ShaideCreateResponse,
    model: &ModelDAO,
) -> Result<RequestBuilder, ShaideError> {
    let endpoint = model.responses_endpoint.as_deref().ok_or_else(|| {
        ShaideError::bad_request(format!(
            "Model '{}' does not expose an OpenAI-compatible Responses endpoint",
            model.name
        ))
    })?;
    match model.platform.as_deref() {
        Some("vertex") => {
            let client = get_gcp_client().await?;
            let token = client.access_token().await.map_err(GcpError::Credentials)?;
            Ok(client
                .client()
                .post(endpoint)
                .bearer_auth(token)
                .json(request))
        }
        Some("foundry") => {
            let client = get_azure_client().await?;
            let token = client.access_token().await?;
            Ok(client
                .client()
                .post(endpoint)
                .bearer_auth(token)
                .json(request))
        }
        Some("axem") => Ok(get_axem_client()
            .await
            .client()
            .post(endpoint)
            .json(request)),
        Some(platform) => Err(ShaideError::unsupported_platform(platform.to_owned())),
        None => Err(ShaideError::unsupported_platform("none".to_owned())),
    }
}

async fn json_response(
    request: RequestBuilder,
    platform: Option<&str>,
) -> Result<Value, ShaideError> {
    let response = request
        .send()
        .await
        .map_err(|error| provider_request_error(platform, error))?;
    let status_code = response.status();
    let response_body = response
        .text()
        .await
        .map_err(|error| provider_request_error(platform, error))?;
    if !status_code.is_success() {
        return Err(provider_http_error(platform, status_code, response_body));
    }
    serde_json::from_str(&response_body).map_err(|error| {
        ShaideError::internal_server_error(format!(
            "Could not parse Responses API response: {error}"
        ))
    })
}

async fn response_stream(
    request: RequestBuilder,
    platform: Option<&str>,
) -> Result<RawResponseStream, ShaideError> {
    let mut event_source = request.eventsource().map_err(|error| {
        ShaideError::internal_server_error(format!(
            "Could not create Responses event stream: {error}"
        ))
    })?;
    let first_event = match event_source.next().await {
        Some(Err(reqwest_eventsource::Error::InvalidStatusCode(status_code, response))) => {
            let response_body = response
                .text()
                .await
                .map_err(|error| provider_request_error(platform, error))?;
            return Err(provider_http_error(platform, status_code, response_body));
        }
        event => event,
    };

    let stream = async_stream::stream! {
        let mut pending_event = first_event;
        loop {
            let event = match pending_event.take() {
                Some(event) => Some(event),
                None => event_source.next().await,
            };
            let Some(event) = event else {
                break;
            };
            match event {
                Ok(UpstreamEvent::Open) => {}
                Ok(UpstreamEvent::Message(message)) if message.event == "keepalive" => {}
                Ok(UpstreamEvent::Message(message)) if message.data == "[DONE]" => break,
                Ok(UpstreamEvent::Message(message)) => {
                    yield parse_upstream_event(&message.event, &message.data);
                }
                Err(reqwest_eventsource::Error::StreamEnded) => break,
                Err(error) => {
                    yield Err(error.to_string());
                    break;
                }
            }
        }
        event_source.close();
    };
    Ok(Box::pin(stream))
}

/// Parses an upstream SSE message. The JSON `type` field is authoritative, the SSE `event`
/// name is only a fallback for providers that omit `type` from the payload.
fn parse_upstream_event(sse_event: &str, data: &str) -> Result<RawResponseEvent, String> {
    let data: Value = serde_json::from_str(data)
        .map_err(|error| format!("Could not parse Responses API event: {error}"))?;
    let event_type = data
        .get("type")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| (!sse_event.is_empty() && sse_event != "message").then(|| sse_event.to_owned()))
        .ok_or_else(|| "Responses API event has no type".to_owned())?;
    Ok(RawResponseEvent { event_type, data })
}

fn provider_request_error(platform: Option<&str>, error: reqwest::Error) -> ShaideError {
    match platform {
        Some("vertex") => GcpError::Request(error).into(),
        Some("foundry") => AzureError::Request(error).into(),
        Some("axem") => ShaideProviderError::Request(error).into(),
        Some(platform) => ShaideError::unsupported_platform(platform.to_owned()),
        None => ShaideError::unsupported_platform("none".to_owned()),
    }
}

const INVALID_ENCRYPTED_CONTENT: &str = "invalid_encrypted_content";

fn provider_http_error(
    platform: Option<&str>,
    status_code: reqwest::StatusCode,
    response_body: String,
) -> ShaideError {
    if let Some(error) = invalid_encrypted_content_error(status_code, &response_body) {
        return error;
    }
    match platform {
        Some("vertex") => GcpError::UnexpectedResponse {
            status_code,
            response_body,
            service: "responses".to_owned(),
        }
        .into(),
        Some("foundry") => AzureError::HttpError {
            status_code,
            response_body,
        }
        .into(),
        Some("axem") => ShaideProviderError::HttpError {
            status_code,
            response_body,
        }
        .into(),
        Some(platform) => ShaideError::unsupported_platform(platform.to_owned()),
        None => ShaideError::unsupported_platform("none".to_owned()),
    }
}

/// Encrypted reasoning and compaction items can only be decrypted by the provider that created
/// them. This error almost always means that a conversation started against another provider
/// (e.g. Codex with OpenAI) was continued through shaide, so explain that instead of forwarding
/// the provider's opaque message alone.
fn invalid_encrypted_content_error(
    status_code: reqwest::StatusCode,
    response_body: &str,
) -> Option<ShaideError> {
    if status_code != reqwest::StatusCode::BAD_REQUEST {
        return None;
    }
    let body: Value = serde_json::from_str(response_body).ok()?;
    let error = body.get("error")?;
    if error.get("code").and_then(Value::as_str) != Some(INVALID_ENCRYPTED_CONTENT) {
        return None;
    }
    let upstream_message = error
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or_default();
    warn!(
        upstream_message,
        "Responses request contains encrypted content from another provider"
    );
    Some(ShaideError::RequestRejection {
        status_code,
        message: format!(
            "This conversation contains encrypted reasoning or compaction items created by a \
             different model provider, which the model behind shaide cannot decrypt. This \
             happens when a conversation started with another provider (for example Codex with \
             OpenAI) is continued through shaide. Start a new conversation instead. Provider \
             message: {upstream_message}"
        ),
        code: INVALID_ENCRYPTED_CONTENT.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn upstream_event_is_forwarded_verbatim() {
        let data = json!({
            "type": "response.completed",
            "sequence_number": 7,
            "response": {
                "id": "resp_1",
                "end_turn": true,
                "usage": {"input_tokens": 120, "output_tokens": 30, "total_tokens": 150}
            }
        });
        let event = parse_upstream_event("response.completed", &data.to_string()).unwrap();
        assert_eq!(event.event_type, "response.completed");
        assert_eq!(event.data, data);
        let usage = event.usage().unwrap();
        assert_eq!(usage.prompt_tokens, 120);
        assert_eq!(usage.completion_tokens, 30);
        assert_eq!(usage.total_tokens, 150);
    }

    #[test]
    fn unknown_event_types_are_not_rejected() {
        let data = json!({"type": "response.some_future_event.delta", "delta": "x"});
        let event = parse_upstream_event("message", &data.to_string()).unwrap();
        assert_eq!(event.event_type, "response.some_future_event.delta");
        assert!(event.usage().is_none());
    }

    #[test]
    fn sse_event_name_is_used_when_type_is_missing() {
        let event = parse_upstream_event("response.in_progress", "{}").unwrap();
        assert_eq!(event.event_type, "response.in_progress");
        assert!(parse_upstream_event("message", "{}").is_err());
    }

    #[test]
    fn invalid_encrypted_content_gets_an_actionable_message() {
        let body = json!({"error": {
            "code": "invalid_encrypted_content",
            "message": "The encrypted content for item cmp_1 could not be verified.",
            "type": "invalid_request_error",
            "param": null
        }})
        .to_string();
        let Some(ShaideError::RequestRejection {
            status_code,
            message,
            code,
        }) = invalid_encrypted_content_error(reqwest::StatusCode::BAD_REQUEST, &body)
        else {
            panic!("expected a request rejection");
        };
        assert_eq!(status_code, reqwest::StatusCode::BAD_REQUEST);
        assert_eq!(code, INVALID_ENCRYPTED_CONTENT);
        assert!(message.contains("Start a new conversation"));
        assert!(message.contains("cmp_1 could not be verified"));
    }

    #[test]
    fn other_provider_errors_are_left_alone() {
        let body = json!({"error": {"code": "context_length_exceeded", "message": "too long"}});
        assert!(
            invalid_encrypted_content_error(reqwest::StatusCode::BAD_REQUEST, &body.to_string())
                .is_none()
        );
        assert!(
            invalid_encrypted_content_error(reqwest::StatusCode::BAD_REQUEST, "not json").is_none()
        );
    }

    #[test]
    fn usage_is_missing_without_token_counts() {
        assert!(response_usage(&json!({"usage": null})).is_none());
        assert!(response_usage(&json!({})).is_none());
    }
}
