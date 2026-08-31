pub mod authorize_admin;
pub mod authorize_user;
pub mod error_response;

use axum::{
    body::{Body, to_bytes},
    extract::{FromRequestParts, Request},
    http::HeaderValue,
    middleware::Next,
    response::Response,
};
use http::{HeaderMap, header::AUTHORIZATION, request::Parts};
use shaide_db::{DbConn, Role, api_usage::InsertApiUsageDao};
use tracing::{debug, error};

use crate::{error::ShaideError, services::auth::get_auth_service};

// TODO: no idea why we need this tbh
pub async fn forward_headers_middleware(mut request: Request<Body>, next: Next) -> Response<Body> {
    // Preserve the original host and protocol for proxied requests
    // This ensures the control panel can determine the public-facing URL

    // Detect the actual protocol from the incoming request before borrowing headers mutably
    let proto = if request.uri().scheme_str() == Some("https") {
        HeaderValue::from_static("https")
    } else {
        HeaderValue::from_static("http")
    };

    let headers = request.headers_mut();

    // Only add X-Forwarded-Host if not already present (preserve existing if set by ingress)
    if !headers.contains_key("x-forwarded-host")
        && let Some(host) = headers.get("host").cloned()
    {
        headers.insert("x-forwarded-host", host);
    }

    // Only add X-Forwarded-Proto if not already present
    if !headers.contains_key("x-forwarded-proto") {
        headers.insert("x-forwarded-proto", proto);
    }

    next.run(request).await
}

pub async fn logging_middleware(request: Request<Body>, next: Next) -> Response<Body> {
    let method = request.method().clone();
    let uri = request.uri().clone();

    let (parts, body) = request.into_parts();
    let bytes = to_bytes(body, usize::MAX).await.unwrap();
    if uri != "/" {
        if let Ok(body) = String::from_utf8(bytes.to_vec()) {
            debug!(
                uri = uri.to_string(),
                method = method.to_string(),
                body = body,
                "request received"
            );
        } else {
            debug!(
                uri = uri.to_string(),
                method = method.to_string(),
                "request received"
            );
        }
    }
    // Reconstruct the body here
    let body = Body::from(bytes);
    let request = Request::from_parts(parts, body);
    let res = next.run(request).await;
    if uri != "/" {
        debug!(
            method = method.to_string(),
            uri = uri.to_string(),
            "request served"
        );
    }
    res
}

fn get_bearer_value(headers: &HeaderMap<HeaderValue>) -> Option<String> {
    let header_value = headers.get(AUTHORIZATION)?.to_str().ok()?;
    let (schema, token) = header_value.split_once(' ')?;
    if schema.eq_ignore_ascii_case("bearer") {
        Some(token.to_owned())
    } else {
        None
    }
}

pub(crate) fn get_bearer_value_or_error(parts: &Parts) -> Result<String, ShaideError> {
    let bearer_token = get_bearer_value(&parts.headers);
    if let Some(bearer_token) = bearer_token {
        Ok(bearer_token)
    } else {
        error!(
            method = %parts.method,
            uri = %parts.uri,
            "Request rejected with no bearer token"
        );
        Err(ShaideError::unauthorized(
            "Bearer token not set".to_string(),
        ))
    }
}

#[derive(Debug, Clone)]
pub enum Principal {
    Admin,
    User,
}

impl FromRequestParts<DbConn> for Principal {
    type Rejection = ShaideError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &DbConn,
    ) -> Result<Self, Self::Rejection> {
        let bearer_token = get_bearer_value_or_error(parts)?;
        let user_id = get_auth_service().validate_access_token(&bearer_token)?;
        let user = state.get_user_by_id(user_id).await?;
        match user.role {
            Role::Admin => {
                debug!(
                    user_id,
                    method = %parts.method,
                    uri = %parts.uri,
                    "Admin made request"
                );
                Ok(Principal::Admin)
            }
            Role::User => {
                debug!(
                    user_id,
                    method = %parts.method,
                    uri = %parts.uri,
                    "User made request"
                );
                state
                    .insert_api_usage(InsertApiUsageDao {
                        route: parts.uri.to_string(),
                        user: user_id,
                    })
                    .await?;
                Ok(Principal::User)
            }
        }
    }
}
