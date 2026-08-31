use axum::extract::FromRequestParts;
use http::request::Parts;
use shaide_db::{DbConn, Role};
use tracing::debug;

use crate::{
    error::ShaideError,
    middlewares::get_bearer_value_or_error,
    services::auth::{AuthService, get_auth_service},
};

#[derive(Debug, Clone, Copy)]
pub struct Admin;

async fn authorize_admin(
    db: &DbConn,
    bearer_token: &str,
    auth_service: &AuthService,
) -> Result<i64, ShaideError> {
    let user_id = auth_service.validate_access_token(bearer_token)?;
    let user = db.get_user_by_id(user_id).await?;
    if user.role == Role::Admin {
        Ok(user_id)
    } else {
        Err(ShaideError::unauthorized(
            "Authenticated user is not an admin".to_string(),
        ))
    }
}

impl FromRequestParts<DbConn> for Admin {
    type Rejection = ShaideError;

    async fn from_request_parts(parts: &mut Parts, db: &DbConn) -> Result<Self, Self::Rejection> {
        let bearer_token = get_bearer_value_or_error(parts)?;
        let user_id = authorize_admin(db, &bearer_token, get_auth_service()).await?;
        debug!(
            user_id,
            method = %parts.method,
            uri = %parts.uri,
            "Admin made request"
        );
        Ok(Admin)
    }
}
