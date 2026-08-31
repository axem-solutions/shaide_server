use axum::extract::FromRequestParts;
use http::request::Parts;
use shaide_db::{DbConn, UserDAO, api_usage::InsertApiUsageDao};
use tracing::debug;

use crate::{
    error::ShaideError, middlewares::get_bearer_value_or_error, services::auth::get_auth_service,
};

#[derive(Debug, Clone)]
pub struct AuthUser {
    pub user: UserDAO,
    pub request_id: i64,
}

impl FromRequestParts<DbConn> for AuthUser {
    type Rejection = ShaideError;

    async fn from_request_parts(parts: &mut Parts, db: &DbConn) -> Result<Self, Self::Rejection> {
        let bearer_token = get_bearer_value_or_error(parts)?;
        let user_id = get_auth_service().validate_access_token(&bearer_token)?;
        let user = db.get_user_by_id(user_id).await?;
        debug!(
            user_id = user.id,
            method = %parts.method,
            uri = %parts.uri,
            "User made request"
        );
        let request_id = db
            .insert_api_usage(InsertApiUsageDao {
                route: parts.uri.to_string(),
                user: user.id,
            })
            .await?;
        Ok(AuthUser { user, request_id })
    }
}
