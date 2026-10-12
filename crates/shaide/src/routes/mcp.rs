use crate::{
    error::ShaideError,
    middlewares::{AccessRequirement, authorize_request},
    services::{
        collection_context::{self as context, ContextTargets},
        mcp::{ResolvedMcpTarget, get_mcp_service},
    },
};
use axum::{
    Json, Router,
    body::Body,
    extract::{Path, Request, State},
    middleware::{Next, from_fn_with_state},
    response::{IntoResponse, Response},
    routing,
};
use axum_reverse_proxy::{ProxyRouterExt, TargetResolver};
use http::StatusCode;
use shaide_common::api::mcp::McpServerListResponse;
use shaide_db::{DbConn, UserRole};
use std::collections::HashMap;

#[derive(Clone)]
pub(crate) struct GatewayState {
    db: DbConn,
    targets: ContextTargets,
    client: reqwest::Client,
    discovery: Option<crate::services::mcp::McpService>,
}
impl GatewayState {
    async fn services(&self) -> Result<crate::services::mcp::McpService, ShaideError> {
        if let Some(service) = &self.discovery {
            return Ok(service.clone());
        }
        Ok(get_mcp_service().await?.clone())
    }
}
async fn resolve_mcp_target(
    State(state): State<GatewayState>,
    Path(params): Path<HashMap<String, String>>,
    req: Request,
    next: Next,
) -> Result<Response, ShaideError> {
    let server_id = params
        .get("server_id")
        .ok_or_else(|| context::failure(StatusCode::NOT_FOUND, "not_found"))?;
    let target = state.targets.0.get(server_id);
    let (mut parts, body) = req.into_parts();
    if target.is_some() && parts.headers.get_all("authorization").iter().count() != 1 {
        return Err(context::failure(StatusCode::UNAUTHORIZED, "unauthorized"));
    }
    // Central authorization and request accounting exactly once.
    let auth = authorize_request(
        &parts,
        &state.db,
        if target.is_some() {
            AccessRequirement::Any
        } else {
            AccessRequirement::User
        },
    )
    .await?;
    let service_url = state
        .services()
        .await?
        .get_service_url(server_id)
        .ok_or_else(|| context::failure(StatusCode::NOT_FOUND, "not_found"))?;
    if let Some(target) = target {
        context::exact_route(&parts.uri, &parts.method, server_id)?;
        if context::origin(&service_url, true).ok().as_deref() != Some(&target.origin) {
            return Err(context::failure(
                StatusCode::SERVICE_UNAVAILABLE,
                "collection_context_target_mismatch",
            ));
        }
        if context::header_bytes(&parts.headers) > context::HEADER_LIMIT {
            return Err(context::failure(
                StatusCode::PAYLOAD_TOO_LARGE,
                "collection_context_too_large",
            ));
        }
        let (body, catalog) = context::clean_body(body, &parts.method).await?;
        let snapshot = state
            .db
            .collection_context_snapshot(auth.user.id, catalog)
            .await
            .map_err(|error| match error {
                shaide_db::error::ShaideDBError::NotFound(shaide_db::error::Resource::User) => {
                    context::failure(StatusCode::UNAUTHORIZED, "unauthorized")
                }
                _ => context::failure(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "collection_context_metadata_invalid",
                ),
            })?;
        let claims = target.claims(
            snapshot,
            auth.request_id,
            catalog,
            chrono::Utc::now().timestamp(),
        )?;
        let compact = target.sign(&claims)?;
        context::clean_headers(&mut parts.headers, true);
        parts.headers.remove("content-length");
        target.inject(&mut parts.headers, &compact)?;
        let bytes = axum::body::to_bytes(body, context::BODY_LIMIT)
            .await
            .map_err(|_| {
                context::failure(
                    StatusCode::PAYLOAD_TOO_LARGE,
                    "collection_context_too_large",
                )
            })?;
        return forward_participating(
            &state.client,
            &target.origin,
            parts.method,
            parts.headers,
            bytes,
            claims.exp,
        )
        .await;
    }
    // Ordinary bodies / downstream credentials preserve the existing behavior.
    context::clean_headers(&mut parts.headers, false);
    if let Some(value) = parts.headers.remove("x-mcp-authorization") {
        parts.headers.insert("authorization", value);
    } else {
        parts.headers.remove("authorization");
    }
    parts.extensions.insert(ResolvedMcpTarget(service_url));
    Ok(next.run(Request::from_parts(parts, body)).await)
}

async fn forward_participating(
    client: &reqwest::Client,
    origin: &str,
    method: http::Method,
    headers: http::HeaderMap,
    body: axum::body::Bytes,
    exp: i64,
) -> Result<Response, ShaideError> {
    let remaining = exp * 1000 - chrono::Utc::now().timestamp_millis();
    if remaining <= 0 {
        return Err(context::failure(
            StatusCode::UNAUTHORIZED,
            "collection_context_expired",
        ));
    }
    // Bound upstream send/read to claims.exp, with no refresh/retry. The body
    // guard below checks transfer to Hyper, not pending socket writes: after a
    // frame is yielded, Hyper owns it and may stop polling under backpressure.
    // Finding 1 remains blocked on cancellable transport ownership; see docs.
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_millis(remaining as u64);
    let result = tokio::time::timeout_at(deadline, async {
        if chrono::Utc::now().timestamp() >= exp { return Err(context::failure(StatusCode::UNAUTHORIZED,"collection_context_expired")); }
        let mut upstream = client.request(method, format!("{origin}/mcp")).headers(headers).body(body).send().await.map_err(|_| context::failure(StatusCode::SERVICE_UNAVAILABLE,"collection_context_upstream_unavailable"))?;
        if upstream.status().is_redirection() { return Err(context::failure(StatusCode::SERVICE_UNAVAILABLE,"collection_context_redirect_rejected")); }
        let status = upstream.status();
        let mut safe_upstream = upstream.headers().clone();
        context::clean_headers(&mut safe_upstream, true);
        if status.is_client_error() || status.is_server_error() {
            let safe_status = match status {
                StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN | StatusCode::METHOD_NOT_ALLOWED | StatusCode::PAYLOAD_TOO_LARGE => status,
                _ => StatusCode::SERVICE_UNAVAILABLE,
            };
            // Never read/forward the upstream error body or credential headers.
            let mut response = context::failure(safe_status, "collection_context_upstream_rejected").into_response();
            response.headers_mut().insert("cache-control", http::HeaderValue::from_static("private, no-store"));
            if status == StatusCode::METHOD_NOT_ALLOWED
                && let Some(allow) = safe_allow(&safe_upstream) {
                response.headers_mut().insert("allow", allow);
            }
            return Ok(response);
        }
        // Check original declarations as well: stripping a nominated field must
        // not disguise duplicates, invalid media or SSE as an absent empty ack.
        let content_types: Vec<_> = upstream.headers().get_all("content-type").iter().collect();
        if content_types.len() > 1 || content_types.first().is_some_and(|v| !valid_json_content_type(v)) {
            return Err(context::failure(StatusCode::SERVICE_UNAVAILABLE, "collection_context_upstream_transport_unsupported"));
        }
        // Validate the effective media that can actually reach the client.
        let json_media = safe_upstream.get("content-type").is_some_and(valid_json_content_type);
        let mut headers = http::HeaderMap::new();
        // A finite protocol header allowlist; no cookies or arbitrary upstream
        // headers can cross the participating service boundary.
        for name in ["content-type", "mcp-session-id", "mcp-protocol-version"] {
            if safe_upstream.get_all(name).iter().count() == 1
                && let Some(value) = safe_upstream.get(name) {
                headers.insert(name, value.clone());
            }
        }
        headers.insert("cache-control", http::HeaderValue::from_static("private, no-store"));
        let mut bytes = Vec::new();
        while let Some(chunk) = upstream.chunk().await.map_err(|_| context::failure(StatusCode::SERVICE_UNAVAILABLE,"collection_context_upstream_unavailable"))? {
            if bytes.len() + chunk.len() > context::BODY_LIMIT { return Err(context::failure(StatusCode::PAYLOAD_TOO_LARGE,"collection_context_too_large")); }
            bytes.extend_from_slice(&chunk);
        }
        if bytes.is_empty() {
            if !matches!(status, StatusCode::ACCEPTED | StatusCode::NO_CONTENT) {
                return Err(context::failure(StatusCode::SERVICE_UNAVAILABLE, "collection_context_upstream_transport_unsupported"));
            }
        } else if !json_media || serde_json::from_slice::<serde_json::Value>(&bytes).is_err() {
            return Err(context::failure(StatusCode::SERVICE_UNAVAILABLE, "collection_context_upstream_transport_unsupported"));
        }
        if chrono::Utc::now().timestamp() >= exp { return Err(context::failure(StatusCode::UNAUTHORIZED,"collection_context_expired")); }
        let guarded = async_stream::stream! {
            if chrono::Utc::now().timestamp() >= exp {
                yield Err::<axum::body::Bytes, std::io::Error>(std::io::Error::other("collection context expired"));
                return;
            }
            yield Ok::<axum::body::Bytes, std::io::Error>(axum::body::Bytes::from(bytes));
        };
        let mut response = Response::new(Body::from_stream(guarded));
        *response.status_mut() = status;
        *response.headers_mut() = headers;
        Ok(response)
    }).await;
    match result {
        Ok(Ok(response)) => Ok(response),
        Ok(Err(error)) => Ok(private_error(error)),
        Err(_) => Ok(private_error(context::failure(
            StatusCode::UNAUTHORIZED,
            "collection_context_expired",
        ))),
    }
}

// Finite v1 JSON only, with syntactically valid MIME parameters. In particular,
// comma-joined declarations and malformed/empty parameters cannot hide SSE.
fn valid_json_content_type(value: &http::HeaderValue) -> bool {
    fn token(byte: u8) -> bool {
        byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte)
    }
    let Ok(value) = value.to_str() else {
        return false;
    };
    let (media, mut parameters) = value.split_once(';').unwrap_or((value, ""));
    if !media.trim().eq_ignore_ascii_case("application/json") {
        return false;
    }
    if value.contains(';') && parameters.trim().is_empty() {
        return false;
    }
    while !parameters.is_empty() {
        parameters = parameters.trim_start_matches([' ', '\t']);
        let name_len = parameters.bytes().take_while(|b| token(*b)).count();
        if name_len == 0 || parameters.as_bytes().get(name_len) != Some(&b'=') {
            return false;
        }
        parameters = &parameters[name_len + 1..];
        let bytes = parameters.as_bytes();
        let end = if bytes.first() == Some(&b'"') {
            let mut i = 1;
            loop {
                match bytes.get(i) {
                    Some(b'"') => break i + 1,
                    Some(b'\\') => {
                        i += 1;
                        if !bytes
                            .get(i)
                            .is_some_and(|b| *b == b'\t' || (32..=126).contains(b))
                        {
                            return false;
                        }
                    }
                    Some(b) if *b == b'\t' || (32..=126).contains(b) => {}
                    _ => return false,
                }
                i += 1;
            }
        } else {
            let len = bytes.iter().take_while(|b| token(**b)).count();
            if len == 0 {
                return false;
            }
            len
        };
        parameters = parameters[end..].trim_start_matches([' ', '\t']);
        if parameters.is_empty() {
            return true;
        }
        let Some(rest) = parameters.strip_prefix(';') else {
            return false;
        };
        parameters = rest;
        if parameters.trim().is_empty() {
            return false;
        }
    }
    true
}

fn private_error(error: ShaideError) -> Response {
    let mut response = error.into_response();
    response.headers_mut().insert(
        "cache-control",
        http::HeaderValue::from_static("private, no-store"),
    );
    response
}

// Only standard HTTP methods; reject duplicates, extra fields and arbitrary
// text rather than reflecting upstream data in a transport-significant header.
fn safe_allow(headers: &http::HeaderMap) -> Option<http::HeaderValue> {
    if headers.get_all("allow").iter().count() != 1 {
        return None;
    }
    let value = headers.get("allow")?.to_str().ok()?;
    let mut methods = Vec::new();
    for method in value.split(',').map(str::trim) {
        if !matches!(
            method,
            "GET" | "HEAD" | "POST" | "PUT" | "DELETE" | "CONNECT" | "OPTIONS" | "TRACE" | "PATCH"
        ) || methods.contains(&method)
        {
            return None;
        }
        methods.push(method);
    }
    http::HeaderValue::from_str(&methods.join(", ")).ok()
}

#[utoipa::path(get, path = "/v1/list-mcps", tag = "mcp", responses((status = 200, description = "MCP servers", body = McpServerListResponse)), security(("bearer_token" = [])))]
pub(crate) async fn list_mcp_servers(
    State(state): State<GatewayState>,
    req: Request,
) -> Result<Json<McpServerListResponse>, ShaideError> {
    let (parts, _) = req.into_parts();
    let principal = authorize_request(&parts, &state.db, AccessRequirement::Any).await?;
    let servers = state.services().await?.get_services().await;
    Ok(Json(McpServerListResponse {
        servers: servers
            .into_iter()
            .map(|s| s.into_response())
            .filter(|s| {
                principal.user.role != UserRole::Admin || state.targets.0.contains_key(&s.name)
            })
            .collect(),
    }))
}
#[derive(Clone)]
struct McpResolver;
impl TargetResolver for McpResolver {
    fn resolve(&self, req: &http::Request<Body>, params: &[(String, String)]) -> String {
        let base = req
            .extensions()
            .get::<ResolvedMcpTarget>()
            .unwrap()
            .0
            .trim_end_matches('/');
        let path = params
            .iter()
            .find(|(k, _)| k == "path")
            .map(|(_, v)| v.as_str())
            .unwrap_or_default();
        let query = req
            .uri()
            .query()
            .map(|q| format!("?{q}"))
            .unwrap_or_default();
        format!("{base}/{path}{query}")
    }
}
fn gateway_routes(state: GatewayState) -> Router {
    let proxy = Router::new()
        .proxy_route("/v1/mcp/proxy/{server_id}/{*path}", McpResolver)
        .route_layer(from_fn_with_state(state.clone(), resolve_mcp_target));
    let list = Router::new()
        .route("/v1/list-mcps", routing::get(list_mcp_servers))
        .with_state(state);
    proxy.merge(list)
}
pub async fn mcp_user_routes(db: DbConn) -> Router {
    let targets = crate::config::get_environment_config()
        .mcp_collection_context_targets
        .clone();
    gateway_routes(GatewayState {
        db,
        targets,
        client: reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("MCP transport initialization failed"),
        discovery: None,
    })
}
#[cfg(test)]
mod collection_context_tests;
