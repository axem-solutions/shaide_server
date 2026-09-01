use axum::Router;
use utoipa::{
    Modify, OpenApi,
    openapi::security::{HttpAuthScheme, HttpBuilder, SecurityScheme},
};
use utoipa_swagger_ui::SwaggerUi;

#[derive(OpenApi)]
#[openapi(
    paths(
        crate::routes::health::health,
        crate::routes::trial::is_trial,
        crate::routes::users::login,
        crate::routes::users::get_users,
        crate::routes::users::create_user,
        crate::routes::users::generate_users,
        crate::routes::models::list_models,
        crate::routes::models::create_models,
        crate::routes::models::delete_model,
        crate::routes::models::set_model_limits,
        crate::routes::models::list_embedding_models,
        crate::routes::models::insert_embedding_model,
        crate::routes::models::delete_embedding_model,
        crate::routes::completions::completions,
        crate::routes::chat::chat_completions,
        crate::routes::responses::create_response,
        crate::routes::embedding::embed_code,
        crate::routes::embedding::delete_vectors,
        crate::routes::vector_db::remote_search,
        crate::routes::vector_db::create_user_collection,
        crate::routes::vector_db::delete_user_collection,
        crate::routes::statistics::model_daily_usage_statistics,
        crate::routes::statistics::api_usage_statistics,
        crate::routes::statistics::user_daily_usage,
        crate::routes::mcp::list_mcp_servers,
    ),
    tags(
        (name = "auth", description = "Authentication"),
        (name = "health", description = "Health checks"),
        (name = "trial", description = "Trial state"),
        (name = "users", description = "User management"),
        (name = "models", description = "Model management"),
        (name = "embedding models", description = "Embedding model management"),
        (name = "chat", description = "OpenAI-compatible chat completions"),
        (name = "responses", description = "OpenAI-compatible responses"),
        (name = "completions", description = "OpenAI-compatible text completions"),
        (name = "embeddings", description = "Embedding and indexing"),
        (name = "rag", description = "Vector search and RAG collections"),
        (name = "statistics", description = "Usage statistics"),
        (name = "mcp", description = "MCP server discovery"),
    ),
    modifiers(&SecurityAddon)
)]
pub struct ApiDoc;

struct SecurityAddon;

impl Modify for SecurityAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        let Some(components) = openapi.components.as_mut() else {
            return;
        };

        components.add_security_scheme(
            "bearer_token",
            SecurityScheme::Http(
                HttpBuilder::new()
                    .scheme(HttpAuthScheme::Bearer)
                    .bearer_format("JWT or admin token")
                    .description(Some(
                        "Bearer token. Use `Authorization: Bearer <token>`. Endpoints may require a user access token, the admin token, or either.",
                    ))
                    .build(),
            ),
        );
    }
}

pub fn openapi_router() -> Router {
    SwaggerUi::new("/swagger-ui")
        .url("/api-docs/openapi.json", ApiDoc::openapi())
        .into()
}
