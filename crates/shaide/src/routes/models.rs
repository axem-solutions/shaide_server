use axum::{Json, Router, extract::State, routing};
use shaide_common::api::{
    embedding_models::{
        DeleteEmbeddingModelRequest, DeleteEmbeddingModelResponse, InsertEmbeddingModelRequest,
        InsertEmbeddingModelResponse, ListEmbeddingModel, ListEmbeddingModelsResponse,
    },
    models::{
        CreateModelRequest, CreateModelResponse, DeleteModelRequest, ListModelsResponse,
        OpenAIListModel, SetModelLimitsRequest, validate_reasoning_effort_values,
    },
};
use shaide_db::{
    DbConn, InsertModelDAO, embedding_models::InsertEmbeddingModelDao, models::SetModelLimitsDao,
};

use crate::{
    error::ShaideError,
    middlewares::{Admin, Authenticated},
};

#[utoipa::path(
    get,
    path = "/v1/models",
    tag = "models",
    responses((status = 200, description = "Available models", body = ListModelsResponse)),
    security(("bearer_token" = []))
)]
pub async fn list_models(
    _authenticated: Authenticated,
    State(db): State<DbConn>,
) -> Result<Json<ListModelsResponse>, ShaideError> {
    let models = db.list_models().await?;
    let (models, data) = models
        .into_iter()
        .map(|model| {
            let openai_model = OpenAIListModel {
                id: model.name.clone(),
                object: "model".to_owned(),
                created: u32::try_from(model.created_at.timestamp()).unwrap_or_default(),
                owned_by: model
                    .platform
                    .clone()
                    .unwrap_or_else(|| "shaide".to_owned()),
            };
            (model.to_api_response(), openai_model)
        })
        .unzip();
    Ok(Json(ListModelsResponse {
        models,
        object: "list".to_owned(),
        data,
    }))
}

#[utoipa::path(
    post,
    path = "/v1/models",
    tag = "models",
    request_body = CreateModelRequest,
    responses(
        (status = 200, description = "Created model", body = CreateModelResponse),
        (status = 400, description = "Invalid model definition"),
    ),
    security(("bearer_token" = []))
)]
pub async fn create_models(
    _admin: Admin,
    State(db): State<DbConn>,
    Json(insert_model): Json<CreateModelRequest>,
) -> Result<Json<CreateModelResponse>, ShaideError> {
    validate_reasoning_effort_values(&insert_model.reasoning_effort_values)
        .map_err(|err| ShaideError::bad_request_with_message(err.to_string()))?;
    let db_model = InsertModelDAO::from_create_model_request(insert_model);
    let model_id = db.create_model(db_model).await?;
    Ok(Json(CreateModelResponse { model_id }))
}

#[utoipa::path(
    delete,
    path = "/v1/models",
    tag = "models",
    request_body = DeleteModelRequest,
    responses((status = 200, description = "Deleted model")),
    security(("bearer_token" = []))
)]
pub async fn delete_model(
    _admin: Admin,
    State(db): State<DbConn>,
    Json(DeleteModelRequest { model_id }): Json<DeleteModelRequest>,
) -> Result<(), ShaideError> {
    Ok(db.delete_model_by_id(model_id).await?)
}

#[utoipa::path(
    patch,
    path = "/v1/model-limits",
    tag = "models",
    request_body = SetModelLimitsRequest,
    responses((status = 200, description = "Updated model limits")),
    security(("bearer_token" = []))
)]
pub async fn set_model_limits(
    _admin: Admin,
    State(db): State<DbConn>,
    Json(SetModelLimitsRequest {
        name,
        daily_input_token_limit,
        daily_output_token_limit,
    }): Json<SetModelLimitsRequest>,
) -> Result<(), ShaideError> {
    Ok(db
        .set_model_limits(SetModelLimitsDao {
            name,
            daily_input_token_limit,
            daily_output_token_limit,
        })
        .await?)
}

#[utoipa::path(
    get,
    path = "/v1/embedding_models",
    tag = "embedding models",
    responses((status = 200, description = "Available embedding models", body = ListEmbeddingModelsResponse)),
    security(("bearer_token" = []))
)]
pub async fn list_embedding_models(
    _authenticated: Authenticated,
    State(db): State<DbConn>,
) -> Result<Json<ListEmbeddingModelsResponse>, ShaideError> {
    let models = db.list_embedding_models().await?;
    let models = models
        .into_iter()
        .map(|m| ListEmbeddingModel {
            id: m.id,
            name: m.name,
        })
        .collect::<Vec<_>>();
    Ok(Json(ListEmbeddingModelsResponse { models }))
}

#[utoipa::path(
    post,
    path = "/v1/embedding_model",
    tag = "embedding models",
    request_body = InsertEmbeddingModelRequest,
    responses((status = 200, description = "Created embedding model", body = InsertEmbeddingModelResponse)),
    security(("bearer_token" = []))
)]
pub async fn insert_embedding_model(
    _admin: Admin,
    State(db): State<DbConn>,
    Json(body): Json<InsertEmbeddingModelRequest>,
) -> Result<Json<InsertEmbeddingModelResponse>, ShaideError> {
    let InsertEmbeddingModelRequest {
        url,
        name,
        vector_size,
        platform,
        api_schema,
        max_embedding_model_text_len,
    } = body;
    let insert_embedding_model_dao = InsertEmbeddingModelDao {
        url,
        name,
        vector_size,
        platform,
        api_schema,
        max_embedding_model_text_len,
    };
    let id = db
        .insert_embedding_model(insert_embedding_model_dao)
        .await?;
    Ok(Json(InsertEmbeddingModelResponse { id }))
}

#[utoipa::path(
    delete,
    path = "/v1/embedding_model",
    tag = "embedding models",
    request_body = DeleteEmbeddingModelRequest,
    responses((status = 200, description = "Deleted embedding model", body = DeleteEmbeddingModelResponse)),
    security(("bearer_token" = []))
)]
pub async fn delete_embedding_model(
    _admin: Admin,
    State(db): State<DbConn>,
    Json(body): Json<DeleteEmbeddingModelRequest>,
) -> Result<Json<DeleteEmbeddingModelResponse>, ShaideError> {
    let DeleteEmbeddingModelRequest { id }: DeleteEmbeddingModelRequest = body;
    db.delete_embedding_model(id).await?;
    Ok(Json(DeleteEmbeddingModelResponse {}))
}

pub fn model_router(db: DbConn) -> Router {
    Router::new()
        .route(
            "/v1/models",
            routing::get(list_models)
                .post(create_models)
                .delete(delete_model),
        )
        .route("/v1/embedding_models", routing::get(list_embedding_models))
        .route("/v1/model-limits", routing::patch(set_model_limits))
        .route(
            "/v1/embedding_model",
            routing::post(insert_embedding_model).delete(delete_embedding_model),
        )
        .with_state(db)
}

#[cfg(test)]
mod tests {
    use axum::{Json, body::to_bytes, extract::State, response::IntoResponse};
    use hyper::StatusCode;
    use shaide_common::api::{error::OpenAiErrorResponse, models::CreateModelRequest};
    use shaide_db::DbConn;
    use temp_testdir::TempDir;

    use super::create_models;
    use crate::middlewares::Admin;

    fn create_model_request(name: &str, reasoning_effort_values: &[&str]) -> CreateModelRequest {
        CreateModelRequest {
            name: name.to_owned(),
            variant: name.to_owned(),
            chat_completions_endpoint: "https://example.com/v1/chat/completions".to_owned(),
            completions_endpoint: None,
            responses_endpoint: None,
            api_schema: "open_ai".to_owned(),
            daily_input_token_limit: None,
            daily_output_token_limit: None,
            supports_images: false,
            reasoning_effort_values: reasoning_effort_values
                .iter()
                .map(|value| (*value).to_owned())
                .collect(),
            max_images_per_request: None,
            max_image_bytes: None,
            max_image_width_px: None,
            max_image_height_px: None,
            max_generated_tokens: 512,
            context_size: 32768,
            platform: None,
            native_fim_mode: None,
            fim_prompt_template: None,
        }
    }

    async fn test_db(temp_dir: &TempDir) -> DbConn {
        DbConn::new(&temp_dir.join("shaide-test.sqlite"))
            .await
            .expect("test database should be created")
    }

    async fn bad_request_message(request: CreateModelRequest, db: DbConn) -> String {
        let Err(error) = create_models(Admin, State(db), Json(request)).await else {
            panic!("model should be rejected");
        };
        let response = error.into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body: OpenAiErrorResponse = serde_json::from_slice(&body).unwrap();
        body.message.expect("bad request should carry a message")
    }

    #[tokio::test]
    async fn invalid_reasoning_effort_values_are_rejected() {
        let temp_dir = TempDir::default();
        let db = test_db(&temp_dir).await;

        assert_eq!(
            bad_request_message(
                create_model_request("duplicate-model", &["low", "high", "low"]),
                db.clone(),
            )
            .await,
            r#"reasoning_effort_values[2] is a duplicate of an earlier value: "low""#
        );

        assert_eq!(
            bad_request_message(create_model_request("unknown-model", &["ultra"]), db).await,
            "reasoning_effort_values[0] is not a value chat completions can forward: \"ultra\", \
             expected one of none, minimal, low, medium, high, xhigh"
        );
    }
}
