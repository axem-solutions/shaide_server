use axum::{Json, Router, extract::State, routing};
use serde::{Deserialize, Serialize};
use shaide_common::api::error::OpenAiErrorResponse;
use shaide_db::{
    DbConn, ModelDAO,
    models::{FimModeDao, NativeFimModeDao},
};
use tracing::{debug, warn};

use crate::{
    error::ShaideError,
    middlewares::authorize_user::AuthUser,
    providers::{gcp::get_gcp_client, shaide::get_axem_client},
    services::completion::{
        Choice, CompletionError, CompletionRequest, CompletionResponse, render_fim_prompt,
    },
};

// TODO: this needs major refactoring. We basically want only inject the right service
#[utoipa::path(
    post,
    path = "/v1/completions",
    request_body = CompletionRequest,
    operation_id = "completion",
    tag = "completions",
    responses(
        (status = 200, description = "Success", body = CompletionResponse, content_type = "application/json"),
        (status = 400, description = "Bad request", body = OpenAiErrorResponse),
        (status = 401, description = "Authentication failed", body = OpenAiErrorResponse),
        (status = 403, description = "Model usage limit reached", body = OpenAiErrorResponse),
        (status = 413, description = "Request body too large", body = OpenAiErrorResponse),
        (status = 429, description = "Rate limited", body = OpenAiErrorResponse),
        (status = 500, description = "Internal server error", body = OpenAiErrorResponse),
        (status = 503, description = "Provider unavailable", body = OpenAiErrorResponse)
    ),
    security(("bearer_token" = []))
)]
pub async fn completions(
    auth: AuthUser,
    State(db): State<DbConn>,
    Json(request): Json<CompletionRequest>,
) -> Result<Json<CompletionResponse>, ShaideError> {
    debug!(
        user_id = auth.user.id,
        model = request.model,
        has_raw_prompt = request.has_raw_prompt(),
        "Handling completions request"
    );
    if request.has_raw_prompt() {
        warn!(
            model = request.model,
            "Raw prompt completions are unsupported for provider-backed models"
        );
        return Err(ShaideError::bad_request_with_message(
            "raw_prompt is unsupported for provider-backed completions".to_owned(),
        ));
    }
    let model_dao = db.get_model_by_name(&request.model).await?;
    let completion_url = model_dao.completions_endpoint.as_deref().ok_or_else(|| {
        warn!(
            model = model_dao.name,
            provider = ?model_dao.platform,
            "Native completions endpoint is not configured for model"
        );
        ShaideError::bad_request_with_message("unsupported-native-fim".to_owned())
    })?;
    let provider_payload = build_provider_payload(&request, &model_dao)?;
    let response = if model_dao.platform.as_deref() == Some("vertex") {
        let gcp_client = get_gcp_client().await?;
        gcp_client
            .post_native_completion(completion_url, &provider_payload)
            .await?
    } else {
        let axem_client = get_axem_client().await;
        axem_client
            .post_native_completion(completion_url, &provider_payload)
            .await?
    };
    let completion = map_provider_response(response)?;
    Ok(Json(completion))
}

fn map_completion_error(err: CompletionError) -> ShaideError {
    ShaideError::bad_request_with_message(err.to_string())
}

fn build_provider_payload(
    request: &CompletionRequest,
    model: &ModelDAO,
) -> Result<NativeCompletionPayload, ShaideError> {
    if request
        .temperature
        .is_some_and(|temperature| !temperature.is_finite())
    {
        return Err(ShaideError::bad_request_with_message(
            "invalid temperature value".to_owned(),
        ));
    }

    let (prefix, suffix) = request.fim_segments().map_err(map_completion_error)?;
    let (prompt, suffix) = match model.native_fim_mode {
        FimModeDao(Some(NativeFimModeDao::CompletionsSuffix)) => {
            (prefix.to_owned(), Some(suffix.to_owned()))
        }
        FimModeDao(Some(NativeFimModeDao::FimTokens)) => (
            render_fim_prompt(model.fim_prompt_template.as_deref(), prefix, suffix)
                .map_err(map_completion_error)?,
            None,
        ),
        FimModeDao(None) => {
            warn!(model = model.name, "Native FIM disabled for model");
            return Err(ShaideError::bad_request_with_message(
                "unsupported-native-fim".to_owned(),
            ));
        }
    };

    Ok(NativeCompletionPayload {
        model: model.variant.clone(),
        prompt,
        max_tokens: model.max_generated_tokens,
        stream: false,
        suffix,
        temperature: request.temperature,
        seed: request.seed,
    })
}

#[derive(Debug, Serialize)]
pub struct NativeCompletionPayload {
    model: String,
    prompt: String,
    max_tokens: i64,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    suffix: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    seed: Option<u64>,
}

fn extract_text_from_provider_response(response: &ProviderCompletionResponse) -> Option<String> {
    match &response.body {
        ProviderCompletionBody::Choices { choices } => {
            let choice = choices.first()?;
            match choice {
                ProviderChoice::Text { text } => Some(text.clone()),
                ProviderChoice::Message { message } => match &message.content {
                    ProviderMessageContent::Text(text) => Some(text.clone()),
                    ProviderMessageContent::Parts(parts) => {
                        let text = parts
                            .iter()
                            .filter_map(|part| part.text.as_deref())
                            .collect::<String>();
                        (!text.is_empty()).then_some(text)
                    }
                },
            }
        }
        ProviderCompletionBody::Content { content } => {
            let text = content
                .iter()
                .map(|part| part.text.as_str())
                .collect::<String>();
            (!text.is_empty()).then_some(text)
        }
    }
}

fn map_provider_response(
    response: ProviderCompletionResponse,
) -> Result<CompletionResponse, ShaideError> {
    let text = extract_text_from_provider_response(&response).ok_or_else(|| {
        ShaideError::internal_server_error(
            "provider completion response did not contain a text choice".to_owned(),
        )
    })?;
    let id = response
        .id
        .unwrap_or_else(|| format!("cmpl-{}", uuid::Uuid::new_v4()));
    Ok(CompletionResponse::new(id, vec![Choice::new(text)], None))
}

#[derive(Debug, Deserialize)]
pub struct ProviderCompletionResponse {
    #[serde(default)]
    id: Option<String>,
    #[serde(flatten)]
    body: ProviderCompletionBody,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum ProviderCompletionBody {
    Choices { choices: Vec<ProviderChoice> },
    Content { content: Vec<ProviderContentPart> },
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum ProviderChoice {
    Text { text: String },
    Message { message: ProviderMessage },
}

#[derive(Debug, Deserialize)]
struct ProviderMessage {
    content: ProviderMessageContent,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum ProviderMessageContent {
    Text(String),
    Parts(Vec<ProviderMessageContentPart>),
}

#[derive(Debug, Deserialize)]
struct ProviderMessageContentPart {
    #[serde(default)]
    text: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ProviderContentPart {
    text: String,
}

pub fn completion_router(db: DbConn) -> Router {
    Router::new()
        .route("/v1/completions", routing::post(completions))
        .with_state(db)
}
