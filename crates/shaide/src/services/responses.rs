use async_openai::{
    error::{OpenAIError, StreamError},
    types::responses::{Response, ResponseStream, ResponseStreamEvent},
};
use futures::StreamExt;
use reqwest::RequestBuilder;
use reqwest_eventsource::{Event as UpstreamEvent, RequestBuilderExt};
use shaide_common::open_ai_types::ShaideCreateResponse;
use shaide_db::ModelDAO;

use crate::{
    error::ShaideError,
    providers::{
        azure::{AzureError, get_azure_client},
        gcp::{GcpError, get_gcp_client},
        shaide::{ShaideProviderError, get_axem_client},
    },
};

pub enum ProviderResponse {
    Json(Box<Response>),
    Stream(ResponseStream),
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
) -> Result<Response, ShaideError> {
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
    serde_json::from_str(&response_body)
        .map_err(|error| OpenAIError::JSONDeserialize(error, response_body).into())
}

async fn response_stream(
    request: RequestBuilder,
    platform: Option<&str>,
) -> Result<ResponseStream, ShaideError> {
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
                    yield serde_json::from_str::<ResponseStreamEvent>(&message.data)
                        .map_err(|error| OpenAIError::JSONDeserialize(error, message.data));
                }
                Err(reqwest_eventsource::Error::StreamEnded) => break,
                Err(error) => {
                    yield Err(OpenAIError::StreamError(Box::new(
                        StreamError::EventStream(error.to_string()),
                    )));
                }
            }
        }
        event_source.close();
    };
    Ok(Box::pin(stream))
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

fn provider_http_error(
    platform: Option<&str>,
    status_code: reqwest::StatusCode,
    response_body: String,
) -> ShaideError {
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
