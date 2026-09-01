pub mod error_response;

use axum::{
    body::Body,
    extract::{FromRequestParts, Request},
    http::HeaderValue,
    middleware::Next,
    response::Response,
};
use chrono::Utc;
use http::{HeaderMap, header::AUTHORIZATION, request::Parts};
use shaide_db::{DbConn, UserDAO, UserRole, api_usage::InsertApiUsageDao};
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

#[derive(Clone, Copy)]
pub(crate) enum AccessRequirement {
    User,
    Admin,
    Any,
}

pub(crate) struct AuthorizedRequest {
    pub user: UserDAO,
    pub request_id: i64,
}

pub(crate) fn ensure_access(
    user: &UserDAO,
    requirement: AccessRequirement,
) -> Result<(), ShaideError> {
    let role_allowed = match requirement {
        AccessRequirement::User => user.role == UserRole::User,
        AccessRequirement::Admin => user.role == UserRole::Admin,
        AccessRequirement::Any => true,
    };
    if !role_allowed {
        return Err(ShaideError::unauthorized(
            "Authenticated user does not have the required role".to_owned(),
        ));
    }
    if user.role == UserRole::User && user.expiry <= Utc::now() {
        return Err(ShaideError::unauthorized("Account has expired".to_owned()));
    }
    Ok(())
}

pub(crate) async fn authorize_request(
    parts: &Parts,
    db: &DbConn,
    requirement: AccessRequirement,
) -> Result<AuthorizedRequest, ShaideError> {
    let bearer_token = get_bearer_value_or_error(parts)?;
    let user_id = get_auth_service().validate_access_token(&bearer_token)?;
    let user = db.get_user_by_id(user_id).await?;
    ensure_access(&user, requirement)?;

    let request_id = db
        .insert_api_usage(InsertApiUsageDao {
            route: parts.uri.to_string(),
            user: user.id,
        })
        .await?;
    debug!(
        request_id,
        user_id = user.id,
        role = ?user.role,
        method = %parts.method,
        uri = %parts.uri,
        "Authenticated request"
    );
    Ok(AuthorizedRequest { user, request_id })
}

#[derive(Debug, Clone, Copy)]
pub struct Admin;

impl FromRequestParts<DbConn> for Admin {
    type Rejection = ShaideError;

    async fn from_request_parts(parts: &mut Parts, db: &DbConn) -> Result<Self, Self::Rejection> {
        authorize_request(parts, db, AccessRequirement::Admin).await?;
        Ok(Admin)
    }
}

#[derive(Debug, Clone)]
pub struct AuthUser {
    pub user: UserDAO,
    pub request_id: i64,
}

impl FromRequestParts<DbConn> for AuthUser {
    type Rejection = ShaideError;

    async fn from_request_parts(parts: &mut Parts, db: &DbConn) -> Result<Self, Self::Rejection> {
        let authorized = authorize_request(parts, db, AccessRequirement::User).await?;
        Ok(AuthUser {
            user: authorized.user,
            request_id: authorized.request_id,
        })
    }
}

#[derive(Debug, Clone)]
pub enum Authenticated {
    Admin,
    User,
}

impl FromRequestParts<DbConn> for Authenticated {
    type Rejection = ShaideError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &DbConn,
    ) -> Result<Self, Self::Rejection> {
        let authorized = authorize_request(parts, state, AccessRequirement::Any).await?;
        let user = authorized.user;
        match user.role {
            UserRole::Admin => Ok(Authenticated::Admin),
            UserRole::User => Ok(Authenticated::User),
        }
    }
}
