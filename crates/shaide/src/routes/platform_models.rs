use axum::{Extension, Json, Router, routing};
use shaide_common::api::platform_models::PlatformModelsResponse;
use shaide_db::DbConn;
use std::sync::Arc;

use crate::{error::ShaideError, middlewares::Admin, services::platform_models::PlatformDiscovery};

#[utoipa::path(
    get,
    path = "/v1/platform-models",
    tag = "models",
    responses(
        (status = 200, description = "Discovered platform models and per-model warnings; does not register models", body = PlatformModelsResponse),
        (status = 401, description = "Administrator authentication required"),
        (status = 503, description = "Platform discovery unavailable")
    ),
    security(("bearer_token" = []))
)]
pub async fn list_platform_models(
    _admin: Admin,
    Extension(discovery): Extension<Arc<PlatformDiscovery>>,
) -> Result<Json<PlatformModelsResponse>, ShaideError> {
    discovery.list().await.map(Json).map_err(|message| {
        ShaideError::request_rejection(
            hyper::StatusCode::SERVICE_UNAVAILABLE,
            message.to_owned(),
            "platform_discovery_unavailable",
        )
    })
}

pub fn platform_models_router(db: DbConn) -> Router {
    Router::new()
        .route("/v1/platform-models", routing::get(list_platform_models))
        .layer(Extension(Arc::new(PlatformDiscovery::default())))
        .with_state(db)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn anonymous_discovery_is_rejected_before_kubernetes_access() {
        let dir = temp_testdir::TempDir::default();
        let db = DbConn::new(&dir.join("discovery.sqlite")).await.unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            axum::serve(listener, platform_models_router(db))
                .await
                .unwrap()
        });
        let response = reqwest::get(format!("http://{address}/v1/platform-models"))
            .await
            .unwrap();
        assert_eq!(response.status(), hyper::StatusCode::UNAUTHORIZED);
        task.abort();
    }
}
