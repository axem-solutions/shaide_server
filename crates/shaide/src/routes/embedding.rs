use axum::{Json, Router, extract::State, routing};
use shaide_common::api::embedding::{
    EmbedCodeResponse, RemoteIndexRequest, RemoveCodeFilesRequest, RemoveCodeFilesResponse,
};
use shaide_db::DbConn;
use tracing::debug;

use crate::{
    error::ShaideError,
    middlewares::AuthUser,
    providers::{
        azure::get_azure_client, gcp::get_gcp_client, shaide::get_axem_client,
        vector_db::get_vector_db,
    },
};

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

pub fn embedding_router(db: DbConn) -> Router {
    Router::new()
        .route("/v1/index", routing::post(embed_code))
        .route("/v1/delete-vectors", routing::delete(delete_vectors))
        .with_state(db)
}
