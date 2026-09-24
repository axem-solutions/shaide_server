use async_openai::types::embeddings::{
    Base64EmbeddingVector, CreateEmbeddingRequest, EmbeddingInput, EncodingFormat,
};
use axum::{
    Json, Router,
    extract::State,
    response::{IntoResponse, Response},
    routing,
};
use base64::{Engine, engine::general_purpose::STANDARD};
use hyper::StatusCode;
use serde::Serialize;
use shaide_common::api::embedding::{
    EmbedCodeResponse, RemoteIndexRequest, RemoveCodeFilesRequest, RemoveCodeFilesResponse,
};
use shaide_common::api::error::OpenAiErrorResponse;
use shaide_db::{DbConn, embedding_models::EmbeddingModelDao};
use tracing::debug;

use crate::{
    error::ShaideError,
    middlewares::AuthUser,
    providers::{
        azure::get_azure_client, gcp::get_gcp_client, shaide::get_axem_client,
        vector_db::get_vector_db,
    },
    services::embedding::embed,
};

/// OpenAI's per-request cap on the number of inputs.
const MAX_EMBEDDING_INPUTS: usize = 2048;

#[utoipa::path(
    post,
    path = "/v1/index",
    tag = "embeddings",
    request_body = RemoteIndexRequest,
    responses((status = 200, description = "Indexed code snippets", body = EmbedCodeResponse)),
    security(("bearer_token" = []))
)]
pub async fn embed_code(
    auth: AuthUser,
    State(db): State<DbConn>,
    Json(request): Json<RemoteIndexRequest>,
) -> Result<Json<EmbedCodeResponse>, ShaideError> {
    let RemoteIndexRequest {
        inputs,
        embedding_model_id,
        workspace_id,
    } = request;
    debug!(
        user_id = auth.user.id,
        embedding_model_id = embedding_model_id,
        workspace_id = workspace_id,
        input_count = inputs.len(),
        "Handling code embedding request"
    );
    let embedding_model = db.get_embedding_model(embedding_model_id).await?;
    let embedded_snippets = match embedding_model.platform.as_deref() {
        Some("vertex") => {
            let gcp_client = get_gcp_client().await?;
            gcp_client.embed_snippets(embedding_model, inputs).await?
        }
        Some("axem") => {
            let axem_client = get_axem_client().await;
            axem_client.embed_snippets(embedding_model, inputs).await?
        }
        Some("foundry") => {
            let azure_client = get_azure_client().await?;
            azure_client.embed_snippets(embedding_model, inputs).await?
        }
        Some(platform) => return Err(ShaideError::unsupported_platform(platform.to_owned())),
        None => return Err(ShaideError::unsupported_platform("none".to_owned())),
    };
    let vector_db = get_vector_db().await;
    let collection_name = format!("{}/{}", auth.user.id, workspace_id);
    vector_db
        .upsert_embedded_snippets(collection_name, embedded_snippets)
        .await?;
    Ok(Json(EmbedCodeResponse {}))
}

#[utoipa::path(
    delete,
    path = "/v1/delete-vectors",
    tag = "embeddings",
    request_body = RemoveCodeFilesRequest,
    responses((status = 200, description = "Deleted vectors", body = RemoveCodeFilesResponse)),
    security(("bearer_token" = []))
)]
pub async fn delete_vectors(
    auth: AuthUser,
    Json(request): Json<RemoveCodeFilesRequest>,
) -> Result<Json<RemoveCodeFilesResponse>, ShaideError> {
    let vector_db = get_vector_db().await;
    let RemoveCodeFilesRequest {
        workspace_id,
        file_paths,
    } = request;
    let collection = format!("{}/{}", auth.user.id, workspace_id);
    vector_db
        .delete_embedded_code_snippets(collection, &file_paths)
        .await?;
    Ok(Json(RemoveCodeFilesResponse {}))
}

#[utoipa::path(
    post,
    path = "/v1/embeddings",
    tag = "embeddings",
    request_body(content = serde_json::Value, description = "OpenAI-compatible embedding request"),
    responses(
        (status = 200, description = "OpenAI-compatible embedding response", body = serde_json::Value),
        (status = 400, description = "Invalid request", body = OpenAiErrorResponse),
        (status = 401, description = "Authentication failed", body = OpenAiErrorResponse),
        (status = 404, description = "Embedding model not found", body = OpenAiErrorResponse),
        (status = 500, description = "Internal server error", body = OpenAiErrorResponse),
        (status = 503, description = "Provider unavailable", body = OpenAiErrorResponse)
    ),
    security(("bearer_token" = []))
)]
pub async fn create_embeddings(
    auth: AuthUser,
    State(db): State<DbConn>,
    Json(request): Json<CreateEmbeddingRequest>,
) -> Result<Response, ShaideError> {
    let texts = embedding_texts(request.input)?;
    let embedding_model = find_embedding_model(&db, &request.model).await?;
    check_dimensions(request.dimensions, &embedding_model)?;
    debug!(
        user_id = auth.user.id,
        embedding_model = %embedding_model.name,
        input_count = texts.len(),
        "Handling OpenAI-compatible embedding request"
    );

    let embeddings = embed(&embedding_model, texts).await?;
    Ok(embedding_response(
        request.model,
        embeddings,
        request.encoding_format.unwrap_or_default(),
    )
    .into_response())
}

/// The providers embed text, so only string input is accepted. OpenAI's token
/// array forms are rejected rather than decoded with a tokenizer that may not
/// match the model.
fn embedding_texts(input: EmbeddingInput) -> Result<Vec<String>, ShaideError> {
    let texts = match input {
        EmbeddingInput::String(text) => vec![text],
        EmbeddingInput::StringArray(texts) => texts,
        EmbeddingInput::IntegerArray(_) | EmbeddingInput::ArrayOfIntegerArray(_) => {
            return Err(ShaideError::bad_request(
                "Token array input is not supported; send the input as a string or an array of strings"
                    .to_owned(),
            ));
        }
    };

    if texts.is_empty() {
        return Err(ShaideError::bad_request(
            "input must contain at least one string".to_owned(),
        ));
    }
    if texts.len() > MAX_EMBEDDING_INPUTS {
        return Err(ShaideError::bad_request(format!(
            "input must not contain more than {MAX_EMBEDDING_INPUTS} strings, got {}",
            texts.len()
        )));
    }
    if let Some(index) = texts.iter().position(|text| text.is_empty()) {
        return Err(ShaideError::bad_request(format!(
            "input[{index}] is empty, every input must be a non-empty string"
        )));
    }

    Ok(texts)
}

/// `model` names a registered embedding model, the same name that
/// `/v1/embedding_models` lists.
async fn find_embedding_model(db: &DbConn, name: &str) -> Result<EmbeddingModelDao, ShaideError> {
    db.list_embedding_models()
        .await?
        .into_iter()
        .find(|model| model.name == name)
        .ok_or_else(|| {
            ShaideError::request_rejection(
                StatusCode::NOT_FOUND,
                format!("The embedding model '{name}' does not exist"),
                "model_not_found".to_owned(),
            )
        })
}

/// A model returns vectors of one size; shortening them is not supported, so
/// `dimensions` is accepted only when it matches.
fn check_dimensions(
    dimensions: Option<u32>,
    embedding_model: &EmbeddingModelDao,
) -> Result<(), ShaideError> {
    match dimensions {
        Some(requested) if i64::from(requested) != embedding_model.vector_size => {
            Err(ShaideError::bad_request(format!(
                "Embedding model '{}' returns {} dimensions; dimensions={requested} is not supported",
                embedding_model.name, embedding_model.vector_size
            )))
        }
        _ => Ok(()),
    }
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
enum EmbeddingVector {
    Float(Vec<f32>),
    Base64(Base64EmbeddingVector),
}

#[derive(Debug, Serialize)]
struct EmbeddingObject {
    object: &'static str,
    index: u32,
    embedding: EmbeddingVector,
}

/// The providers do not report token counts, so usage is reported as zero.
#[derive(Debug, Serialize)]
struct EmbeddingUsage {
    prompt_tokens: u32,
    total_tokens: u32,
}

#[derive(Debug, Serialize)]
struct EmbeddingResponse {
    object: &'static str,
    model: String,
    data: Vec<EmbeddingObject>,
    usage: EmbeddingUsage,
}

impl IntoResponse for EmbeddingResponse {
    fn into_response(self) -> Response {
        Json(self).into_response()
    }
}

fn embedding_response(
    model: String,
    embeddings: Vec<Vec<f32>>,
    encoding_format: EncodingFormat,
) -> EmbeddingResponse {
    let data = embeddings
        .into_iter()
        .enumerate()
        .map(|(index, vector)| EmbeddingObject {
            object: "embedding",
            index: index as u32,
            embedding: match encoding_format {
                EncodingFormat::Float => EmbeddingVector::Float(vector),
                EncodingFormat::Base64 => EmbeddingVector::Base64(base64_vector(&vector)),
            },
        })
        .collect();

    EmbeddingResponse {
        object: "list",
        model,
        data,
        usage: EmbeddingUsage {
            prompt_tokens: 0,
            total_tokens: 0,
        },
    }
}

/// OpenAI's base64 encoding: the vector's float32 values, little-endian.
fn base64_vector(vector: &[f32]) -> Base64EmbeddingVector {
    let bytes: Vec<u8> = vector
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect();
    Base64EmbeddingVector(STANDARD.encode(bytes))
}

pub fn embedding_router(db: DbConn) -> Router {
    Router::new()
        .route("/v1/embeddings", routing::post(create_embeddings))
        .route("/v1/index", routing::post(embed_code))
        .route("/v1/delete-vectors", routing::delete(delete_vectors))
        .with_state(db)
}

#[cfg(test)]
mod tests {
    use async_openai::types::embeddings::{EmbeddingInput, EncodingFormat};
    use axum::{body::to_bytes, response::IntoResponse};
    use base64::{Engine, engine::general_purpose::STANDARD};
    use hyper::StatusCode;
    use shaide_common::api::error::OpenAiErrorResponse;
    use shaide_db::{
        DbConn,
        embedding_models::{EmbeddingModelDao, InsertEmbeddingModelDao},
    };
    use temp_testdir::TempDir;

    use super::{
        MAX_EMBEDDING_INPUTS, check_dimensions, embedding_response, embedding_texts,
        find_embedding_model,
    };
    use crate::error::ShaideError;

    async fn error_parts(error: ShaideError) -> (StatusCode, OpenAiErrorResponse) {
        let response = error.into_response();
        let status = response.status();
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        (status, serde_json::from_slice(&body).unwrap())
    }

    fn model(vector_size: i64) -> EmbeddingModelDao {
        EmbeddingModelDao {
            id: 1,
            url: "https://example.com/embeddings".to_owned(),
            name: "text-embedding-3-large".to_owned(),
            vector_size,
            platform: Some("foundry".to_owned()),
        }
    }

    #[test]
    fn string_and_string_array_inputs_are_accepted() {
        assert_eq!(
            embedding_texts(EmbeddingInput::String("hello".to_owned())).unwrap(),
            vec!["hello"]
        );
        assert_eq!(
            embedding_texts(EmbeddingInput::StringArray(vec![
                "a".to_owned(),
                "b".to_owned()
            ]))
            .unwrap(),
            vec!["a", "b"]
        );
    }

    #[tokio::test]
    async fn invalid_inputs_are_rejected() {
        let cases = [
            (
                EmbeddingInput::IntegerArray(vec![1, 2]),
                "Token array input",
            ),
            (
                EmbeddingInput::ArrayOfIntegerArray(vec![vec![1]]),
                "Token array input",
            ),
            (EmbeddingInput::StringArray(vec![]), "at least one string"),
            (EmbeddingInput::String(String::new()), "input[0] is empty"),
            (
                EmbeddingInput::StringArray(vec!["a".to_owned(), String::new()]),
                "input[1] is empty",
            ),
            (
                EmbeddingInput::StringArray(vec!["a".to_owned(); MAX_EMBEDDING_INPUTS + 1]),
                "must not contain more than 2048",
            ),
        ];
        for (input, want) in cases {
            let (status, body) = error_parts(embedding_texts(input).unwrap_err()).await;
            assert_eq!(status, StatusCode::BAD_REQUEST);
            let message = body.message.unwrap();
            assert!(message.contains(want), "message {message:?} lacks {want:?}");
        }
    }

    #[tokio::test]
    async fn dimensions_must_match_the_model() {
        assert!(check_dimensions(None, &model(3072)).is_ok());
        assert!(check_dimensions(Some(3072), &model(3072)).is_ok());

        let (status, body) =
            error_parts(check_dimensions(Some(256), &model(3072)).unwrap_err()).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(body.message.unwrap().contains("returns 3072 dimensions"));
    }

    #[test]
    fn float_response_matches_the_openai_shape() {
        let response = embedding_response(
            "text-embedding-3-large".to_owned(),
            vec![vec![0.5, -1.0], vec![2.0, 0.25]],
            EncodingFormat::Float,
        );
        let json = serde_json::to_value(response).unwrap();

        assert_eq!(
            json,
            serde_json::json!({
                "object": "list",
                "model": "text-embedding-3-large",
                "data": [
                    {"object": "embedding", "index": 0, "embedding": [0.5, -1.0]},
                    {"object": "embedding", "index": 1, "embedding": [2.0, 0.25]}
                ],
                "usage": {"prompt_tokens": 0, "total_tokens": 0}
            })
        );
    }

    // OpenAI clients decode base64 embeddings as little-endian float32.
    #[test]
    fn base64_response_decodes_to_the_same_floats() {
        let vector = vec![0.5_f32, -1.0, 3.25];
        let response =
            embedding_response("m".to_owned(), vec![vector.clone()], EncodingFormat::Base64);
        let json = serde_json::to_value(response).unwrap();

        let encoded = json["data"][0]["embedding"]
            .as_str()
            .expect("base64 string");
        let decoded: Vec<f32> = STANDARD
            .decode(encoded)
            .unwrap()
            .chunks_exact(4)
            .map(|bytes| f32::from_le_bytes(bytes.try_into().unwrap()))
            .collect();
        assert_eq!(decoded, vector);
    }

    #[tokio::test]
    async fn embedding_model_is_found_by_name() {
        let temp_dir = TempDir::default();
        let db = DbConn::new(&temp_dir.join("shaide-test.sqlite"))
            .await
            .unwrap();
        db.insert_embedding_model(InsertEmbeddingModelDao {
            url: "https://example.com/embeddings".to_owned(),
            name: "text-embedding-3-large".to_owned(),
            vector_size: 3072,
            platform: Some("foundry".to_owned()),
            api_schema: Some("open_ai".to_owned()),
        })
        .await
        .unwrap();

        let found = find_embedding_model(&db, "text-embedding-3-large")
            .await
            .unwrap();
        assert_eq!(found.vector_size, 3072);

        let (status, body) =
            error_parts(find_embedding_model(&db, "missing").await.unwrap_err()).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body.error.code.as_deref(), Some("model_not_found"));
    }
}
