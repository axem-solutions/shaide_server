use async_openai::types::chat::{
    ChatCompletionRequestMessage::User, ChatCompletionRequestUserMessage,
    ChatCompletionRequestUserMessageContent, ChatCompletionRequestUserMessageContentPart,
    ChatCompletionStreamOptions, CreateChatCompletionRequest,
};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, State},
    response::{
        IntoResponse, Response, Sse,
        sse::{Event, KeepAlive},
    },
    routing,
};
use base64::{Engine, engine::general_purpose::STANDARD as BASE64_STANDARD};
use futures::StreamExt;
use shaide_common::{
    api::error::{OpenAiErrorBody, OpenAiErrorResponse},
    open_ai_types::ShaideChatCompletionStreamEvent,
};
use shaide_db::{DbConn, ModelDAO};
use tracing::{debug, error};

use crate::{
    error::ShaideError,
    middlewares::authorize_user::AuthUser,
    providers::gcp::{check_user_model_usage, try_update_model_usage},
    services::chat::{collect_chat_completion, completion_stream},
};

// Keep below Vertex's 30 MB request limit; this is a Shaide transport guard.
const MAX_CHAT_COMPLETIONS_BODY_BYTES: usize = 28 * 1024 * 1024;

fn does_user_message_have_images(request: &CreateChatCompletionRequest) -> bool {
    user_message_image_urls(request).next().is_some()
}

fn count_user_message_images(request: &CreateChatCompletionRequest) -> usize {
    user_message_image_urls(request).count()
}

fn user_message_image_urls(
    request: &CreateChatCompletionRequest,
) -> impl Iterator<Item = &str> + '_ {
    request.messages.iter().flat_map(|message| {
        let User(ChatCompletionRequestUserMessage { content, .. }) = message else {
            return Vec::new();
        };
        user_message_content_image_urls(content)
    })
}

fn user_message_content_image_urls(content: &ChatCompletionRequestUserMessageContent) -> Vec<&str> {
    let ChatCompletionRequestUserMessageContent::Array(message_content) = content else {
        return Vec::new();
    };
    message_content
        .iter()
        .filter_map(|msg| {
            let ChatCompletionRequestUserMessageContentPart::ImageUrl(image) = msg else {
                return None;
            };
            Some(image.image_url.url.as_str())
        })
        .collect()
}

fn validate_user_message_vision_limits(
    request: &CreateChatCompletionRequest,
    model: &ModelDAO,
) -> Result<(), ShaideError> {
    let image_count = count_user_message_images(request);

    if let Some(max_images) = model.max_images_per_request
        && image_count as i64 > max_images
    {
        return Err(ShaideError::bad_request_with_message(format!(
            "This request contains too many images for model '{}'. The model accepts up to {max_images} images per request.",
            model.name
        )));
    }

    for image_url in user_message_image_urls(request) {
        validate_image_url_vision_limits(image_url, model)?;
    }

    Ok(())
}

fn validate_image_url_vision_limits(image_url: &str, model: &ModelDAO) -> Result<(), ShaideError> {
    let Some(bytes) = decode_image_data_url(image_url)? else {
        return Ok(());
    };

    if let Some(max_bytes) = model.max_image_bytes
        && bytes.len() as i64 > max_bytes
    {
        return Err(ShaideError::bad_request_with_message(format!(
            "An image in this request is too large for model '{}'. The model accepts images up to {max_bytes} bytes.",
            model.name
        )));
    }

    let needs_dimensions =
        model.max_image_width_px.is_some() || model.max_image_height_px.is_some();
    if !needs_dimensions {
        return Ok(());
    }

    let Some((width, height)) = image_dimensions(&bytes) else {
        return Err(ShaideError::bad_request_with_message(
            "Unable to determine image dimensions for inline image payload".to_owned(),
        ));
    };

    if let Some(max_width) = model.max_image_width_px
        && width as i64 > max_width
    {
        return Err(ShaideError::bad_request_with_message(format!(
            "An image in this request is too wide for model '{}'. The model accepts images up to {max_width}px wide.",
            model.name
        )));
    }

    if let Some(max_height) = model.max_image_height_px
        && height as i64 > max_height
    {
        return Err(ShaideError::bad_request_with_message(format!(
            "An image in this request is too tall for model '{}'. The model accepts images up to {max_height}px high.",
            model.name
        )));
    }

    Ok(())
}

fn decode_image_data_url(image_url: &str) -> Result<Option<Vec<u8>>, ShaideError> {
    // URI schemes are case-insensitive per RFC 3986 §3.1.
    let Some((scheme, rest)) = image_url.split_once(':') else {
        return Ok(None);
    };
    if !scheme.eq_ignore_ascii_case("data") {
        return Ok(None);
    }
    let Some((metadata, encoded)) = rest.split_once(',') else {
        return Err(ShaideError::bad_request_with_message(
            "Invalid image data URL".to_owned(),
        ));
    };
    if !metadata.to_ascii_lowercase().starts_with("image/") {
        return Ok(None);
    }
    if !metadata
        .split(';')
        .any(|segment| segment.eq_ignore_ascii_case("base64"))
    {
        return Err(ShaideError::bad_request_with_message(
            "Inline image data URLs must be base64 encoded".to_owned(),
        ));
    }
    BASE64_STANDARD
        .decode(encoded)
        .map(Some)
        .map_err(|_| ShaideError::bad_request_with_message("Invalid base64 image data".to_owned()))
}

fn image_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    let size = imagesize::blob_size(bytes).ok()?;
    Some((size.width as u32, size.height as u32))
}

#[utoipa::path(
    post,
    path = "/v1/chat/completions",
    tag = "chat",
    request_body(content = serde_json::Value, description = "OpenAI-compatible chat completion request"),
    responses(
        (status = 200, description = "JSON chat completion, or a server-sent event stream when stream=true", content_type = "text/event-stream", body = String),
        (status = 400, description = "Invalid request", body = OpenAiErrorResponse),
        (status = 401, description = "Authentication failed", body = OpenAiErrorResponse),
        (status = 403, description = "Model usage limit reached", body = OpenAiErrorResponse),
        (status = 413, description = "Request body too large", body = OpenAiErrorResponse),
        (status = 429, description = "Rate limited", body = OpenAiErrorResponse),
        (status = 500, description = "Internal server error", body = OpenAiErrorResponse),
        (status = 503, description = "Provider unavailable", body = OpenAiErrorResponse)
    ),
    security(("bearer_token" = []))
)]
pub async fn chat_completions(
    auth: AuthUser,
    State(db): State<DbConn>,
    Json(mut request): Json<CreateChatCompletionRequest>,
) -> Result<Response, ShaideError> {
    let should_stream = request.stream.unwrap_or_default();
    debug!(
        user_id = auth.user.id,
        model = request.model,
        message_count = request.messages.len(),
        stream = should_stream,
        "Handling chat request"
    );
    let request_has_image = does_user_message_have_images(&request);
    let model_dao = db.get_model_by_name(&request.model).await?;

    if request_has_image && !model_dao.supports_images {
        return Err(ShaideError::bad_request(format!(
            "Model '{}' does not support image input",
            request.model
        )));
    }
    validate_user_message_vision_limits(&request, &model_dao)?;

    // NOTE: this checks whether the user is able to use the model. In the future, we might opt for
    // a more middleware heavy implementation. Maybe.
    check_user_model_usage(db.clone(), auth.user.id, &model_dao).await?;

    // Providers use one streaming path internally. The route decides whether that stream is
    // forwarded as SSE or collected into a regular chat completion response.
    request.stream = Some(true);
    if !should_stream {
        request.stream_options = Some(ChatCompletionStreamOptions {
            include_usage: Some(true),
            include_obfuscation: None,
        });
    }

    let requested_model = request.model.clone();
    let mut provider_stream = completion_stream(request, &model_dao).await?;
    if !should_stream {
        let response = collect_chat_completion(provider_stream, requested_model).await?;
        if let Err(err) = try_update_model_usage(
            db,
            auth.user.id,
            auth.request_id,
            model_dao.id,
            response.usage.as_ref(),
        )
        .await
        {
            error!(error = %err, "Could not update model usage statistics");
        }
        return Ok(Json(response).into_response());
    }

    let s = async_stream::stream! {
        let mut terminated = false;
        while let Some(event) = provider_stream.next().await {
            match event {
                Ok(ShaideChatCompletionStreamEvent::Chunk(event)) => {

                    // If the event has usage, we will try to update the usage statistics
                    if let Err(err) = try_update_model_usage(db.clone(), auth.user.id, auth.request_id, model_dao.id, event.usage.as_ref()).await {
                        error!("Could not update model usage statistics, error: {err}");
                    }
                    yield Ok::<Event, anyhow::Error>(Event::default().json_data(event)?);
                }
                Ok(ShaideChatCompletionStreamEvent::Done) => {
                    terminated = true;
                    yield Ok::<Event, anyhow::Error>(Event::default().data("[DONE]"));
                    break;
                }
                Err(err) => {
                    terminated = true;
                    error!(error = %err, "Chat completion stream failed, terminating stream");
                    let error = OpenAiErrorResponse::new(OpenAiErrorBody::new(
                        err.to_string(),
                        "server_error",
                        None,
                        Some("provider_stream_error".to_owned()),
                    ));
                    yield Ok(Event::default().json_data(error)?);
                    yield Ok(Event::default().data("[DONE]"));
                    break;
                }
            }
        }
        if !terminated {
            error!("Chat completion stream closed before its completion marker");
            yield Ok::<Event, anyhow::Error>(Event::default().json_data(serde_json::json!({
                "error": {
                    "message": "Provider chat stream closed before its completion marker",
                    "type": "provider_error"
                }
            }))?);
            yield Ok::<Event, anyhow::Error>(Event::default().data("[DONE]"));
        }
    };

    Ok(Sse::new(s).keep_alive(KeepAlive::default()).into_response())
}

pub fn chat_router(db: DbConn) -> Router {
    Router::new()
        .route(
            "/v1/chat/completions",
            routing::post(chat_completions)
                .layer(DefaultBodyLimit::max(MAX_CHAT_COMPLETIONS_BODY_BYTES)),
        )
        .with_state(db)
}

#[cfg(test)]
mod tests {
    use async_openai::types::chat::{
        ChatCompletionRequestMessage, ChatCompletionRequestMessageContentPartImage,
        ChatCompletionRequestMessageContentPartText, ChatCompletionRequestUserMessage,
        ChatCompletionRequestUserMessageContent, ChatCompletionRequestUserMessageContentPart,
        CreateChatCompletionRequest, ImageUrl,
    };
    use axum::{body::to_bytes, response::IntoResponse};
    use chrono::Utc;
    use hyper::StatusCode;
    use shaide_common::api::error::OpenAiErrorResponse;
    use shaide_db::{
        ModelDAO,
        models::{ApiSchemaDao, FimModeDao, ReasoningEffortValuesDao},
    };

    use super::{
        does_user_message_have_images, image_dimensions, validate_user_message_vision_limits,
    };
    use crate::error::ShaideError;

    const ONE_BY_ONE_PNG_DATA_URL: &str = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+/p9sAAAAASUVORK5CYII=";

    fn model_with_vision_limits(
        name: &str,
        max_images_per_request: Option<i64>,
        max_image_bytes: Option<i64>,
        max_image_width_px: Option<i64>,
        max_image_height_px: Option<i64>,
    ) -> ModelDAO {
        ModelDAO {
            created_at: Utc::now(),
            updated_at: Utc::now(),
            id: 1,
            name: name.to_owned(),
            variant: name.to_owned(),
            chat_completions_endpoint: "https://example.com/v1/chat/completions".to_owned(),
            completions_endpoint: None,
            responses_endpoint: None,
            api_schema: ApiSchemaDao::OpenAI,
            daily_input_token_limit: None,
            daily_output_token_limit: None,
            supports_images: true,
            reasoning_effort_values: ReasoningEffortValuesDao::default(),
            max_images_per_request,
            max_image_bytes,
            max_image_width_px,
            max_image_height_px,
            max_generated_tokens: 1024,
            context_size: 8192,
            platform: None,
            native_fim_mode: FimModeDao(None),
            fim_prompt_template: None,
        }
    }

    fn request_with_images(model: &str, images: &[&str]) -> CreateChatCompletionRequest {
        CreateChatCompletionRequest {
            model: model.to_owned(),
            messages: vec![ChatCompletionRequestMessage::User(
                ChatCompletionRequestUserMessage {
                    content: ChatCompletionRequestUserMessageContent::Array(
                        images
                            .iter()
                            .map(|url| {
                                ChatCompletionRequestUserMessageContentPart::ImageUrl(
                                    ChatCompletionRequestMessageContentPartImage {
                                        image_url: ImageUrl {
                                            url: (*url).to_owned(),
                                            detail: None,
                                        },
                                    },
                                )
                            })
                            .collect(),
                    ),
                    name: None,
                },
            )],
            ..Default::default()
        }
    }

    #[test]
    fn gemma_model_limit_allows_four_images() {
        let model =
            model_with_vision_limits("google/gemma-4-26b-a4b-it-maas", Some(4), None, None, None);
        let images = [
            ONE_BY_ONE_PNG_DATA_URL,
            ONE_BY_ONE_PNG_DATA_URL,
            ONE_BY_ONE_PNG_DATA_URL,
            ONE_BY_ONE_PNG_DATA_URL,
        ];
        let request = request_with_images(&model.name, &images);

        validate_user_message_vision_limits(&request, &model)
            .expect("four images should be accepted");
    }

    #[tokio::test]
    async fn gemma_model_limit_rejects_more_than_four_images() {
        let model =
            model_with_vision_limits("google/gemma-4-26b-a4b-it-maas", Some(4), None, None, None);
        let images = [
            ONE_BY_ONE_PNG_DATA_URL,
            ONE_BY_ONE_PNG_DATA_URL,
            ONE_BY_ONE_PNG_DATA_URL,
            ONE_BY_ONE_PNG_DATA_URL,
            ONE_BY_ONE_PNG_DATA_URL,
        ];
        let request = request_with_images(&model.name, &images);

        let response = validate_user_message_vision_limits(&request, &model)
            .expect_err("more than four images should be rejected")
            .into_response();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body: OpenAiErrorResponse = serde_json::from_slice(&body).unwrap();

        assert_eq!(
            body.error.message,
            "This request contains too many images for model 'google/gemma-4-26b-a4b-it-maas'. The model accepts up to 4 images per request."
        );
        assert_eq!(body.error.r#type, "invalid_request_error");
    }

    #[test]
    fn mistral_style_limit_rejects_oversized_inline_image_bytes() {
        let model = model_with_vision_limits("mistral-small-2506", None, Some(8), None, None);
        let request = request_with_images(&model.name, &[ONE_BY_ONE_PNG_DATA_URL]);
        let err = validate_user_message_vision_limits(&request, &model)
            .expect_err("image bytes should be rejected");

        assert!(
            matches!(err, ShaideError::BadRequestWithMessage(message) if message.contains("too large"))
        );
    }

    #[test]
    fn anthropic_style_limit_rejects_oversized_dimensions() {
        let model = model_with_vision_limits(
            "claude-opus-4-1",
            Some(100),
            Some(5 * 1024 * 1024),
            Some(0),
            Some(8000),
        );
        let request = request_with_images(&model.name, &[ONE_BY_ONE_PNG_DATA_URL]);
        let err = validate_user_message_vision_limits(&request, &model)
            .expect_err("image width should be rejected");

        assert!(
            matches!(err, ShaideError::BadRequestWithMessage(message) if message.contains("too wide"))
        );
    }

    #[test]
    fn image_dimension_parser_reads_png_dimensions() {
        let bytes = super::decode_image_data_url(ONE_BY_ONE_PNG_DATA_URL)
            .expect("data url should parse")
            .expect("data url should decode");

        assert_eq!(image_dimensions(&bytes), Some((1, 1)));
    }

    #[test]
    fn decode_image_data_url_passes_through_remote_url() {
        let result = super::decode_image_data_url("https://example.com/photo.png")
            .expect("remote url should not error");
        assert!(result.is_none());
    }

    #[test]
    fn decode_image_data_url_passes_through_non_image_data_url() {
        let result = super::decode_image_data_url("data:text/plain;base64,aGVsbG8=")
            .expect("non-image data url should not error");
        assert!(result.is_none());
    }

    #[test]
    fn decode_image_data_url_accepts_uppercase_scheme() {
        // URI schemes are case-insensitive per RFC 3986 §3.1; non-canonical input must still parse.
        let upper = ONE_BY_ONE_PNG_DATA_URL.replacen("data:", "DATA:", 1);
        let lower = super::decode_image_data_url(ONE_BY_ONE_PNG_DATA_URL)
            .expect("lowercase data url should parse")
            .expect("lowercase data url should decode");
        let result = super::decode_image_data_url(&upper)
            .expect("uppercase scheme should parse")
            .expect("uppercase scheme should decode");
        assert_eq!(result, lower);
    }

    #[test]
    fn decode_image_data_url_rejects_missing_comma() {
        let err = super::decode_image_data_url("data:image/png;base64")
            .expect_err("malformed data url should be rejected");
        assert!(
            matches!(err, ShaideError::BadRequestWithMessage(message) if message.contains("Invalid image data URL"))
        );
    }

    #[test]
    fn decode_image_data_url_rejects_missing_base64_marker() {
        let err = super::decode_image_data_url("data:image/png,iVBORw0KGgo")
            .expect_err("data url without base64 marker should be rejected");
        assert!(
            matches!(err, ShaideError::BadRequestWithMessage(message) if message.contains("base64"))
        );
    }

    #[test]
    fn decode_image_data_url_rejects_invalid_base64() {
        let err = super::decode_image_data_url("data:image/png;base64,!!!notbase64!!!")
            .expect_err("invalid base64 payload should be rejected");
        assert!(
            matches!(err, ShaideError::BadRequestWithMessage(message) if message.contains("Invalid base64 image data"))
        );
    }

    #[test]
    fn anthropic_style_limit_rejects_oversized_height() {
        let model = model_with_vision_limits(
            "claude-opus-4-1",
            Some(100),
            Some(5 * 1024 * 1024),
            Some(8000),
            Some(0),
        );
        let request = request_with_images(&model.name, &[ONE_BY_ONE_PNG_DATA_URL]);
        let err = validate_user_message_vision_limits(&request, &model)
            .expect_err("image height should be rejected");

        assert!(
            matches!(err, ShaideError::BadRequestWithMessage(message) if message.contains("too tall"))
        );
    }

    #[test]
    fn bytes_limit_accepts_image_within_budget() {
        let model = model_with_vision_limits("mistral-small-2506", None, Some(10_000), None, None);
        let request = request_with_images(&model.name, &[ONE_BY_ONE_PNG_DATA_URL]);

        validate_user_message_vision_limits(&request, &model)
            .expect("image well under the byte budget should be accepted");
    }

    #[test]
    fn dimension_check_rejects_unparseable_image_bytes() {
        // Valid base64 payload that is not a recognized image format. When the model has
        // dimension limits configured, we cannot validate dimensions and must reject.
        let model = model_with_vision_limits("claude-opus-4-1", None, None, Some(100), None);
        let request = request_with_images(&model.name, &["data:image/png;base64,YWJjZA=="]);
        let err = validate_user_message_vision_limits(&request, &model)
            .expect_err("unparseable bytes should fail dimension validation");

        assert!(
            matches!(err, ShaideError::BadRequestWithMessage(message) if message.contains("Unable to determine image dimensions"))
        );
    }

    #[test]
    fn remote_image_url_bypasses_byte_and_dimension_checks() {
        // Documenting a known limitation: byte and dimension limits only fire for inline
        // data: URLs, since validating a remote URL would require fetching it.
        let model =
            model_with_vision_limits("claude-opus-4-1", Some(10), Some(1), Some(1), Some(1));
        let request = request_with_images(&model.name, &["https://example.com/huge.png"]);

        validate_user_message_vision_limits(&request, &model)
            .expect("remote URLs should pass through byte/dimension validation");
    }

    #[test]
    fn multi_image_request_rejects_when_any_image_exceeds_byte_limit() {
        let model = model_with_vision_limits("mistral-small-2506", None, Some(50), None, None);
        let request = request_with_images(
            &model.name,
            // First image decodes to 3 bytes (under the 50-byte limit); the 1x1 PNG
            // decodes to ~67 bytes and trips the limit.
            &["data:image/png;base64,YWJj", ONE_BY_ONE_PNG_DATA_URL],
        );
        let err = validate_user_message_vision_limits(&request, &model)
            .expect_err("any oversized image should reject the whole request");

        assert!(
            matches!(err, ShaideError::BadRequestWithMessage(message) if message.contains("too large"))
        );
    }

    #[test]
    fn vision_limits_pass_when_request_has_no_images() {
        let model = model_with_vision_limits("claude-opus-4-1", Some(1), Some(1), Some(1), Some(1));
        let request = CreateChatCompletionRequest {
            model: model.name.clone(),
            messages: vec![ChatCompletionRequestMessage::User(
                ChatCompletionRequestUserMessage {
                    content: ChatCompletionRequestUserMessageContent::Text(
                        "no images here".to_owned(),
                    ),
                    name: None,
                },
            )],
            ..Default::default()
        };

        validate_user_message_vision_limits(&request, &model)
            .expect("request with no images should never trip vision limits");
    }

    #[test]
    fn image_in_older_user_turn_is_detected() {
        let request = CreateChatCompletionRequest {
            model: "mistral-small-2503".to_owned(),
            messages: vec![
                ChatCompletionRequestMessage::User(ChatCompletionRequestUserMessage {
                    content: ChatCompletionRequestUserMessageContent::Array(vec![
                        ChatCompletionRequestUserMessageContentPart::ImageUrl(
                            ChatCompletionRequestMessageContentPartImage {
                                image_url: ImageUrl {
                                    url: "https://picsum.photos/id/237/200/300".to_owned(),
                                    detail: None,
                                },
                            },
                        ),
                    ]),
                    name: None,
                }),
                ChatCompletionRequestMessage::User(ChatCompletionRequestUserMessage {
                    content: ChatCompletionRequestUserMessageContent::Text(
                        "latest prompt text".to_owned(),
                    ),
                    name: None,
                }),
            ],
            ..Default::default()
        };

        let has_images = does_user_message_have_images(&request);
        assert!(has_images);
    }

    #[test]
    fn image_and_text_in_same_user_turn_are_detected_and_keep_text() {
        let request = CreateChatCompletionRequest {
            model: "mistral-small-2503".to_owned(),
            messages: vec![ChatCompletionRequestMessage::User(
                ChatCompletionRequestUserMessage {
                    content: ChatCompletionRequestUserMessageContent::Array(vec![
                        ChatCompletionRequestUserMessageContentPart::Text(
                            ChatCompletionRequestMessageContentPartText {
                                text: "describe this image".to_owned(),
                            },
                        ),
                        ChatCompletionRequestUserMessageContentPart::ImageUrl(
                            ChatCompletionRequestMessageContentPartImage {
                                image_url: ImageUrl {
                                    url: "https://picsum.photos/id/237/200/300".to_owned(),
                                    detail: None,
                                },
                            },
                        ),
                    ]),
                    name: None,
                },
            )],
            ..Default::default()
        };

        let has_images = does_user_message_have_images(&request);
        assert!(has_images);
    }

    #[test]
    fn image_only_user_turn_is_allowed_and_returns_no_text() {
        let request = CreateChatCompletionRequest {
            model: "mistral-small-2503".to_owned(),
            messages: vec![ChatCompletionRequestMessage::User(
                ChatCompletionRequestUserMessage {
                    content: ChatCompletionRequestUserMessageContent::Array(vec![
                        ChatCompletionRequestUserMessageContentPart::ImageUrl(
                            ChatCompletionRequestMessageContentPartImage {
                                image_url: ImageUrl {
                                    url: "https://picsum.photos/id/237/200/300".to_owned(),
                                    detail: None,
                                },
                            },
                        ),
                    ]),
                    name: None,
                },
            )],
            ..Default::default()
        };

        let has_images = does_user_message_have_images(&request);

        assert!(has_images);
    }

    #[test]
    fn older_image_does_not_allow_empty_latest_user_turn() {
        let request = CreateChatCompletionRequest {
            model: "mistral-small-2503".to_owned(),
            messages: vec![
                ChatCompletionRequestMessage::User(ChatCompletionRequestUserMessage {
                    content: ChatCompletionRequestUserMessageContent::Array(vec![
                        ChatCompletionRequestUserMessageContentPart::ImageUrl(
                            ChatCompletionRequestMessageContentPartImage {
                                image_url: ImageUrl {
                                    url: "https://picsum.photos/id/237/200/300".to_owned(),
                                    detail: None,
                                },
                            },
                        ),
                    ]),
                    name: None,
                }),
                ChatCompletionRequestMessage::User(ChatCompletionRequestUserMessage {
                    content: ChatCompletionRequestUserMessageContent::Array(vec![]),
                    name: None,
                }),
            ],
            ..Default::default()
        };

        let has_images = does_user_message_have_images(&request);

        assert!(has_images);
    }
}
