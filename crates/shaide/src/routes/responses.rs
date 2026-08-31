use async_openai::traits::EventType;
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, State},
    response::{
        IntoResponse, Response,
        sse::{Event, KeepAlive, Sse},
    },
    routing,
};
use futures::StreamExt;
use shaide_common::open_ai_types::ShaideCreateResponse;
use shaide_db::{DbConn, models::ApiSchemaDao};
use tracing::{debug, error};

use crate::{
    error::ShaideError,
    middlewares::authorize_user::AuthUser,
    providers::gcp::check_user_model_usage,
    services::responses::{ProviderResponse, create_response as create_provider_response},
};

const MAX_RESPONSES_BODY_BYTES: usize = 28 * 1024 * 1024;

#[utoipa::path(
    post,
    path = "/v1/responses",
    tag = "responses",
    request_body(content = serde_json::Value, description = "OpenAI-compatible Responses API request"),
    responses((status = 200, description = "JSON response, or a server-sent event stream when stream=true")),
    security(("bearer_token" = []))
)]
pub async fn create_response(
    auth: AuthUser,
    State(db): State<DbConn>,
    Json(request): Json<ShaideCreateResponse>,
) -> Result<Response, ShaideError> {
    let model_name = request
        .model
        .as_deref()
        .ok_or_else(|| ShaideError::bad_request("model is required".to_owned()))?;
    debug!(
        user_id = auth.user.id,
        model = model_name,
        stream = request.stream.unwrap_or(false),
        "Handling Responses API request"
    );

    let model = db.get_model_by_name(model_name).await?;
    if !matches!(&model.api_schema, ApiSchemaDao::OpenAI) {
        return Err(ShaideError::bad_request(format!(
            "Model '{}' does not expose an OpenAI-compatible Responses API",
            model.name
        )));
    }
    check_user_model_usage(db, auth.user.id, &model).await?;

    match create_provider_response(request, &model).await? {
        ProviderResponse::Json(response) => Ok(Json(response).into_response()),
        ProviderResponse::Stream(mut stream) => {
            let events = async_stream::stream! {
                while let Some(event) = stream.next().await {
                    match event {
                        Ok(event) => {
                            let event_type = event.event_type();
                            yield Event::default().event(event_type).json_data(event)
                                .map_err(anyhow::Error::from);
                        }
                        Err(error) => {
                            error!(error = %error, "Responses API stream failed");
                            yield Err(anyhow::Error::from(error));
                            break;
                        }
                    }
                }
            };
            Ok(Sse::new(events)
                .keep_alive(KeepAlive::default())
                .into_response())
        }
    }
}

pub fn responses_router(db: DbConn) -> Router {
    Router::new()
        .route(
            "/v1/responses",
            routing::post(create_response).layer(DefaultBodyLimit::max(MAX_RESPONSES_BODY_BYTES)),
        )
        .with_state(db)
}
