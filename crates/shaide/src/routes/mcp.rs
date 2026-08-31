use std::collections::HashMap;

use axum::{
    Json, Router,
    extract::{Path, Request},
    middleware::{Next, from_fn_with_state},
    response::Response,
    routing,
};
use axum_reverse_proxy::{ProxyRouterExt, TargetResolver};
use shaide_common::api::mcp::McpServerListResponse;
use shaide_db::DbConn;

use crate::{
    error::ShaideError,
    middlewares::AuthUser,
    services::mcp::{ResolvedMcpTarget, get_mcp_service},
};

const X_MCP_AUTHORIZATION: &str = "x-mcp-authorization";

async fn resolve_mcp_target(
    Path(params): Path<HashMap<String, String>>,
    _auth: AuthUser,
    mut req: Request,
    next: Next,
) -> Result<Response, ShaideError> {
    if let Some(value) = req.headers().get(X_MCP_AUTHORIZATION).cloned() {
        req.headers_mut().insert("Authorization", value);
        req.headers_mut().remove(X_MCP_AUTHORIZATION);
    } else {
        req.headers_mut().remove("Authorization");
    }
    let server_id = params
        .get("server_id")
        .ok_or_else(|| ShaideError::bad_request("Server id not found".into()))?;
    let mcp_service = get_mcp_service().await?;
    let service_url = mcp_service
        .get_service_url(server_id)
        .ok_or_else(|| ShaideError::not_found_route("mcp route not found".into()))?;
    req.extensions_mut().insert(ResolvedMcpTarget(service_url));
    Ok(next.run(req).await)
}

#[utoipa::path(
    get,
    path = "/v1/list-mcps",
    tag = "mcp",
    responses((status = 200, description = "MCP servers", body = McpServerListResponse)),
    security(("bearer_token" = []))
)]
pub async fn list_mcp_servers(_auth: AuthUser) -> Result<Json<McpServerListResponse>, ShaideError> {
    let mcp_service = get_mcp_service().await?;
    let servers = mcp_service.get_services().await;
    let response = McpServerListResponse {
        servers: servers.into_iter().map(|srv| srv.into_response()).collect(),
    };
    Ok(Json(response))
}

#[derive(Clone)]
struct McpResolver;

impl TargetResolver for McpResolver {
    fn resolve(
        &self,
        req: &http::Request<axum::body::Body>,
        params: &[(String, String)],
    ) -> String {
        let target_base = req
            .extensions()
            .get::<ResolvedMcpTarget>()
            .unwrap()
            .0
            .trim_end_matches('/');
        let path = params
            .iter()
            .find(|(k, _)| k == "path")
            .map(|(_, v)| v.to_string())
            .unwrap_or_default();
        let query = req
            .uri()
            .query()
            .map(|q| format!("?{q}"))
            .unwrap_or_default();
        format!("{target_base}/{path}{query}")
    }
}

pub async fn mcp_user_routes(db: DbConn) -> Router {
    let list_mcps_route = Router::new().route("/v1/list-mcps", routing::get(list_mcp_servers));
    let reverse_proxy = Router::new()
        .proxy_route("/v1/mcp/proxy/{server_id}/{*path}", McpResolver)
        .route_layer(from_fn_with_state(db.clone(), resolve_mcp_target));
    reverse_proxy.merge(list_mcps_route).with_state(db)
}
