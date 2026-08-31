use axum::{Json, Router, extract::State, routing};
use shaide_common::api::rag::{
    RagCollectionParameterRequest, RagDeleteCollectionRequest, RemoteSearchRequest,
    RemoteSearchResponse,
};
use shaide_db::DbConn;

use crate::{
    error::ShaideError,
    middlewares::authorize_user::AuthUser,
    providers::vector_db::{RagCollectionParameters, get_vector_db},
    services::embedding::embed,
};

#[utoipa::path(
    post,
    path = "/v1/rag/user-collection",
    tag = "rag",
    request_body = RagCollectionParameterRequest,
    responses((status = 200, description = "Created user collection")),
    security(("bearer_token" = []))
)]
pub async fn create_user_collection(
    auth: AuthUser,
    State(db): State<DbConn>,
    Json(collection_parameter_request): Json<RagCollectionParameterRequest>,
) -> Result<(), ShaideError> {
    let RagCollectionParameterRequest {
        collection_name,
        model_id,
    } = collection_parameter_request;
    let collection_name = format!("{}_{}", auth.user.id, collection_name);
    let model = db.get_embedding_model(model_id).await?;
    let vector_size = model.vector_size as u64;
    let rag = get_vector_db().await;
    rag.create_collection(RagCollectionParameters::new(collection_name, vector_size))
        .await?;
    Ok(())
}

#[utoipa::path(
    delete,
    path = "/v1/rag/user-collection",
    tag = "rag",
    request_body = RagDeleteCollectionRequest,
    responses((status = 200, description = "Deleted user collection")),
    security(("bearer_token" = []))
)]
pub async fn delete_user_collection(
    auth: AuthUser,
    Json(delete_collection_request): Json<RagDeleteCollectionRequest>,
) -> Result<(), ShaideError> {
    let RagDeleteCollectionRequest { collection_name } = delete_collection_request;
    let collection_name = format!("{}_{}", auth.user.id, collection_name);
    let rag = get_vector_db().await;
    rag.delete_collection(&collection_name).await?;
    Ok(())
}

#[utoipa::path(
    post,
    path = "/v1/search",
    tag = "rag",
    request_body = RemoteSearchRequest,
    responses((status = 200, description = "Search results", body = RemoteSearchResponse)),
    security(("bearer_token" = []))
)]
pub async fn remote_search(
    auth: AuthUser,
    State(db): State<DbConn>,
    Json(request): Json<RemoteSearchRequest>,
) -> Result<Json<RemoteSearchResponse>, ShaideError> {
    let RemoteSearchRequest {
        query,
        min_score,
        max_results,
        workspace_id,
        identifier,
        r#type,
        embedding_model_id,
    } = request;
    let embedding_model = db.get_embedding_model(embedding_model_id).await?;
    let embedding = embed(&embedding_model, vec![query])
        .await?
        .into_iter()
        .next()
        .ok_or_else(|| {
            ShaideError::internal_server_error(
                "Embedding provider returned no embedding for the search query".to_owned(),
            )
        })?;
    let vector_db = get_vector_db().await;
    let collection_name = format!("{}/{}", auth.user.id, workspace_id);
    let results = vector_db
        .query_code(
            collection_name,
            embedding,
            max_results,
            min_score,
            identifier,
            r#type,
        )
        .await?;
    Ok(Json(RemoteSearchResponse { results }))
}

pub fn vector_db_router(db: DbConn) -> Router {
    Router::new()
        .route("/v1/search", routing::post(remote_search))
        .route(
            "/v1/rag/user-collection",
            routing::post(create_user_collection).delete(delete_user_collection),
        )
        .with_state(db)
}
