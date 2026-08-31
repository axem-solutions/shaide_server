use axum::{
    Json,
    body::{Body, to_bytes},
    extract::Request,
    http::{StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use serde_json::{Map, Value, json};
use shaide_common::api::error::{OpenAiErrorBody, OpenAiErrorResponse};
use tracing::error;

use crate::error::ShaideError;

pub async fn map_error_response(request: Request, next: Next) -> Response {
    let response = next.run(request).await;
    let status = response.status();
    if !status.is_client_error() && !status.is_server_error() {
        return response;
    }

    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok());
    if content_type.is_some_and(|content_type| {
        !content_type.starts_with("application/json") && !content_type.starts_with("text/plain")
    }) {
        return response;
    }

    let (mut parts, body) = response.into_parts();
    let bytes = match to_bytes(body, usize::MAX).await {
        Ok(bytes) => bytes,
        Err(error) => {
            error!(%error, "Failed to read error response body");
            let error = OpenAiErrorBody::new(
                "Internal server error",
                ShaideError::openai_error_type(StatusCode::INTERNAL_SERVER_ERROR),
                None,
                Some("internal_error".to_owned()),
            );
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(OpenAiErrorResponse::from_reason(error)),
            )
                .into_response();
        }
    };

    let mut body = serde_json::from_slice::<Value>(&bytes)
        .ok()
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default();
    if !body.contains_key("error") {
        add_openai_error(&mut body, status, &bytes);
    }

    parts.headers.remove(header::CONTENT_LENGTH);
    parts.headers.insert(
        header::CONTENT_TYPE,
        header::HeaderValue::from_static("application/json"),
    );
    let body = serde_json::to_vec(&body).expect("serde_json::Value always serializes");
    Response::from_parts(parts, Body::from(body))
}

fn add_openai_error(body: &mut Map<String, Value>, status: StatusCode, original_body: &[u8]) {
    let message = body
        .get("message")
        .or_else(|| body.get("reason"))
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| {
            let message = String::from_utf8_lossy(original_body).trim().to_owned();
            (!message.is_empty()).then_some(message)
        })
        .unwrap_or_else(|| {
            status
                .canonical_reason()
                .unwrap_or("Request failed")
                .to_owned()
        });

    if !body.contains_key("message") && !body.contains_key("reason") {
        let legacy_field = if status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error() {
            "reason"
        } else {
            "message"
        };
        body.insert(legacy_field.to_owned(), Value::String(message.clone()));
    }
    body.insert(
        "error".to_owned(),
        json!({
            "message": message,
            "type": ShaideError::openai_error_type(status),
            "param": null,
            "code": null,
        }),
    );
}
