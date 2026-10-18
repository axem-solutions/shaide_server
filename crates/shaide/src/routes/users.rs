use anyhow::Result;
use axum::{Json, Router, extract::State, http::StatusCode, routing};
use chrono::{Duration, TimeZone, Utc};
use shaide_common::api::users::{
    AccessTokenResponse, CreateUserRequest, CreateUserResponse, GenerateUsersRequest,
    GenerateUsersResponse, GeneratedUserResponse, ListUser, ListUsersResponse, LoginRequest,
    RefreshTokenRequest,
};
use shaide_db::{DbConn, UserDAO, UserRole, error::ShaideDBError};
use tracing::{debug, warn};

use crate::{
    error::ShaideError,
    middlewares::{AccessRequirement, Admin, ensure_access},
    services::auth::{get_auth_service, hash_refresh_token},
};

/// How long after a refresh token was rotated presenting it again counts as a benign race (two
/// tabs refreshing at once) rather than reuse of a stolen token, which revokes the session.
const REFRESH_TOKEN_REUSE_GRACE: Duration = Duration::seconds(30);

#[utoipa::path(
    get,
    path = "/v1/users",
    tag = "users",
    responses((status = 200, description = "Users", body = ListUsersResponse)),
    security(("bearer_token" = []))
)]
pub async fn get_users(
    _admin: Admin,
    State(db): State<DbConn>,
) -> Result<Json<ListUsersResponse>, ShaideError> {
    let users = db.list_users().await?;
    let users = users
        .into_iter()
        .map(|u| ListUser {
            id: u.id,
            username: u.username,
            expiry: u.expiry,
        })
        .collect();
    Ok(Json(ListUsersResponse { users }))
}

#[utoipa::path(
    post,
    path = "/v1/user",
    tag = "users",
    request_body = CreateUserRequest,
    responses((status = 200, description = "Created user", body = CreateUserResponse)),
    security(("bearer_token" = []))
)]
pub async fn create_user(
    _admin: Admin,
    State(db): State<DbConn>,
    Json(request): Json<CreateUserRequest>,
) -> Result<Json<CreateUserResponse>, ShaideError> {
    let CreateUserRequest {
        username,
        password,
        expiry,
    } = request;
    let password_hash = get_auth_service().hash_password(password).await?;
    let id = db
        .create_user(username.clone(), password_hash, expiry)
        .await?;
    Ok(Json(CreateUserResponse { id, username }))
}

#[utoipa::path(
    post,
    path = "/v1/generate-users",
    tag = "users",
    request_body = GenerateUsersRequest,
    responses((status = 200, description = "Generated users", body = GenerateUsersResponse)),
    security(("bearer_token" = []))
)]
pub async fn generate_users(
    _admin: Admin,
    State(db): State<DbConn>,
    Json(request): Json<GenerateUsersRequest>,
) -> Result<Json<GenerateUsersResponse>, ShaideError> {
    let mut new_users = vec![];
    // Generated users historically behaved as non-expiring accounts. Keep that behavior until
    // account expiry becomes nullable.
    let expiry = Utc
        .with_ymd_and_hms(9999, 12, 31, 23, 59, 59)
        .single()
        .expect("generated user expiry must be valid");
    for _ in 0..request.number_of_new_users {
        let username = uuid::Uuid::new_v4().to_string();
        let password = uuid::Uuid::new_v4().to_string();
        let password_hash = get_auth_service().hash_password(password.clone()).await?;
        let id = db
            .create_user(username.clone(), password_hash, expiry)
            .await?;
        new_users.push(GeneratedUserResponse {
            id,
            username,
            password,
        });
    }
    Ok(Json(GenerateUsersResponse { new_users }))
}

#[utoipa::path(
    post,
    path = "/v1/login",
    tag = "auth",
    request_body = LoginRequest,
    responses(
        (status = 200, description = "User access token", body = AccessTokenResponse),
        (status = 401, description = "Invalid credentials")
    )
)]
pub async fn login(
    State(db): State<DbConn>,
    Json(request): Json<LoginRequest>,
) -> Result<Json<AccessTokenResponse>, ShaideError> {
    let user = db.get_user_by_username(&request.username).await?;
    get_auth_service()
        .verify_password(request.password, user.password_hash.clone())
        .await?;
    ensure_access(&user, AccessRequirement::Any)?;

    // Housekeeping only: a failure must not block the sign-in.
    match db.delete_expired_refresh_tokens().await {
        Ok(deleted) if deleted > 0 => debug!(deleted, "Deleted expired refresh tokens"),
        Ok(_) => {}
        Err(error) => warn!(%error, "Could not delete expired refresh tokens"),
    }

    let refresh_token = get_auth_service().issue_refresh_token(account_expiry(&user))?;
    let family_id = uuid::Uuid::new_v4().to_string();
    db.insert_refresh_token(
        user.id,
        &family_id,
        &refresh_token.hash,
        refresh_token.expires_at,
    )
    .await?;
    Ok(Json(token_response(
        &user,
        refresh_token.token,
        refresh_token.expires_in,
    )?))
}

#[utoipa::path(
    post,
    path = "/v1/token/refresh",
    tag = "auth",
    request_body = RefreshTokenRequest,
    responses(
        (status = 200, description = "New access token and rotated refresh token", body = AccessTokenResponse),
        (status = 401, description = "Refresh token is invalid, expired, used or revoked")
    )
)]
pub async fn refresh_token(
    State(db): State<DbConn>,
    Json(request): Json<RefreshTokenRequest>,
) -> Result<Json<AccessTokenResponse>, ShaideError> {
    let Some(stored) = db
        .get_refresh_token_by_hash(&hash_refresh_token(&request.refresh_token))
        .await?
    else {
        return Err(invalid_refresh_token());
    };

    if stored.revoked_at.is_some() {
        return Err(invalid_refresh_token());
    }
    if let Some(used_at) = stored.used_at {
        if Utc::now() - used_at > REFRESH_TOKEN_REUSE_GRACE {
            warn!(
                user_id = stored.user_id,
                family_id = stored.family_id,
                "Rotated refresh token presented again, revoking its session"
            );
            db.revoke_refresh_token_family(&stored.family_id).await?;
        }
        return Err(invalid_refresh_token());
    }
    if stored.expires_at <= Utc::now() {
        return Err(invalid_refresh_token());
    }

    let user = match db.get_user_by_id(stored.user_id).await {
        Ok(user) => user,
        Err(ShaideDBError::NotFound(_)) => return Err(invalid_refresh_token()),
        Err(error) => return Err(error.into()),
    };
    if let Err(error) = ensure_access(&user, AccessRequirement::Any) {
        db.revoke_refresh_token_family(&stored.family_id).await?;
        return Err(error);
    }

    let successor = get_auth_service().issue_refresh_token(account_expiry(&user))?;
    if !db
        .rotate_refresh_token(stored.id, &successor.hash, successor.expires_at)
        .await?
    {
        // A concurrent request rotated this token first.
        return Err(invalid_refresh_token());
    }
    Ok(Json(token_response(
        &user,
        successor.token,
        successor.expires_in,
    )?))
}

#[utoipa::path(
    post,
    path = "/v1/logout",
    tag = "auth",
    request_body = RefreshTokenRequest,
    responses((status = 204, description = "The refresh token's session is revoked"))
)]
pub async fn logout(
    State(db): State<DbConn>,
    Json(request): Json<RefreshTokenRequest>,
) -> Result<StatusCode, ShaideError> {
    // Answers the same for unknown tokens, so the endpoint does not reveal which tokens exist.
    if let Some(stored) = db
        .get_refresh_token_by_hash(&hash_refresh_token(&request.refresh_token))
        .await?
    {
        db.revoke_refresh_token_family(&stored.family_id).await?;
    }
    Ok(StatusCode::NO_CONTENT)
}

/// Admin accounts do not expire, so only a user's expiry caps the tokens issued to them.
fn account_expiry(user: &UserDAO) -> Option<chrono::DateTime<Utc>> {
    (user.role == UserRole::User).then_some(user.expiry)
}

fn token_response(
    user: &UserDAO,
    refresh_token: String,
    refresh_token_expires_in: u64,
) -> Result<AccessTokenResponse, ShaideError> {
    let (access_token, token_expires_in) =
        get_auth_service().issue_access_token(user.id, account_expiry(user))?;
    Ok(AccessTokenResponse {
        access_token,
        token_type: "Bearer".to_owned(),
        token_expires_in,
        refresh_token,
        refresh_token_expires_in,
        account_expires_at: user.expiry,
        role: user.role.into(),
    })
}

fn invalid_refresh_token() -> ShaideError {
    ShaideError::unauthorized("Invalid or expired refresh token".to_owned())
}

pub fn users_router(db: DbConn) -> Router {
    Router::new()
        .route("/v1/login", routing::post(login))
        .route("/v1/token/refresh", routing::post(refresh_token))
        .route("/v1/logout", routing::post(logout))
        .route("/v1/users", routing::get(get_users))
        .route("/v1/user", routing::post(create_user))
        .route("/v1/generate-users", routing::post(generate_users))
        .with_state(db)
}

#[cfg(test)]
mod tests {
    use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};
    use chrono::{Duration, Utc};
    use shaide_common::api::users::{AccessTokenResponse, LoginRequest, RefreshTokenRequest};
    use shaide_db::DbConn;
    use temp_testdir::TempDir;

    use super::{login, logout, refresh_token};
    use crate::services::auth::{AuthService, init_test_auth_service};

    const PASSWORD: &str = "correct horse battery staple";

    async fn db_with_user(temp_dir: &TempDir, expiry: chrono::DateTime<Utc>) -> DbConn {
        let auth = init_test_auth_service();
        let db = DbConn::new(&temp_dir.join("shaide-test.sqlite"))
            .await
            .expect("test database should be created");
        let password_hash = auth.hash_password(PASSWORD.to_owned()).await.unwrap();
        db.create_user("ada".to_owned(), password_hash, expiry)
            .await
            .unwrap();
        db
    }

    async fn sign_in(db: &DbConn) -> AccessTokenResponse {
        let Json(response) = login(
            State(db.clone()),
            Json(LoginRequest {
                username: "ada".to_owned(),
                password: PASSWORD.to_owned(),
            }),
        )
        .await
        .expect("sign-in should succeed");
        response
    }

    async fn refresh(db: &DbConn, token: &str) -> Result<AccessTokenResponse, StatusCode> {
        refresh_token(
            State(db.clone()),
            Json(RefreshTokenRequest {
                refresh_token: token.to_owned(),
            }),
        )
        .await
        .map(|Json(response)| response)
        .map_err(|error| error.into_response().status())
    }

    /// Moves a token's rotation far enough into the past that presenting it again is reuse.
    async fn age_rotation(db: &DbConn, token: &str) {
        let used_at = Utc::now() - Duration::minutes(5);
        let token_hash = super::hash_refresh_token(token);
        sqlx::query("UPDATE refresh_tokens SET used_at = ? WHERE token_hash = ?")
            .bind(used_at)
            .bind(token_hash)
            .execute(&db.pool)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn a_refresh_token_is_exchanged_for_a_new_access_token_and_a_rotated_refresh_token() {
        let temp_dir = TempDir::default();
        let db = db_with_user(&temp_dir, Utc::now() + Duration::days(365)).await;
        let session = sign_in(&db).await;

        let refreshed = refresh(&db, &session.refresh_token).await.unwrap();

        assert_ne!(refreshed.refresh_token, session.refresh_token);
        assert_eq!(refreshed.token_type, "Bearer");
        let user_id = init_test_auth_service()
            .validate_access_token(&refreshed.access_token)
            .unwrap();
        assert_eq!(
            user_id,
            db.get_user_by_username("ada").await.unwrap().id,
            "the new access token should belong to the signed-in user"
        );
        assert!(refresh(&db, &refreshed.refresh_token).await.is_ok());
    }

    #[tokio::test]
    async fn a_token_presented_again_within_the_grace_period_is_rejected_without_ending_the_session()
     {
        let temp_dir = TempDir::default();
        let db = db_with_user(&temp_dir, Utc::now() + Duration::days(365)).await;
        let session = sign_in(&db).await;
        let refreshed = refresh(&db, &session.refresh_token).await.unwrap();

        assert_eq!(
            refresh(&db, &session.refresh_token).await.unwrap_err(),
            StatusCode::UNAUTHORIZED
        );
        assert!(
            refresh(&db, &refreshed.refresh_token).await.is_ok(),
            "the request that won the race should keep its session"
        );
    }

    #[tokio::test]
    async fn reusing_a_rotated_token_after_the_grace_period_revokes_the_whole_session() {
        let temp_dir = TempDir::default();
        let db = db_with_user(&temp_dir, Utc::now() + Duration::days(365)).await;
        let session = sign_in(&db).await;
        let refreshed = refresh(&db, &session.refresh_token).await.unwrap();
        age_rotation(&db, &session.refresh_token).await;

        assert_eq!(
            refresh(&db, &session.refresh_token).await.unwrap_err(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            refresh(&db, &refreshed.refresh_token).await.unwrap_err(),
            StatusCode::UNAUTHORIZED,
            "the successor of a reused token should be revoked too"
        );
    }

    #[tokio::test]
    async fn reuse_only_revokes_the_session_it_belongs_to() {
        let temp_dir = TempDir::default();
        let db = db_with_user(&temp_dir, Utc::now() + Duration::days(365)).await;
        let compromised = sign_in(&db).await;
        let other_device = sign_in(&db).await;
        refresh(&db, &compromised.refresh_token).await.unwrap();
        age_rotation(&db, &compromised.refresh_token).await;

        refresh(&db, &compromised.refresh_token).await.unwrap_err();

        assert!(refresh(&db, &other_device.refresh_token).await.is_ok());
    }

    #[tokio::test]
    async fn logout_revokes_the_session() {
        let temp_dir = TempDir::default();
        let db = db_with_user(&temp_dir, Utc::now() + Duration::days(365)).await;
        let session = sign_in(&db).await;

        let status = logout(
            State(db.clone()),
            Json(RefreshTokenRequest {
                refresh_token: session.refresh_token.clone(),
            }),
        )
        .await
        .unwrap();

        assert_eq!(status, StatusCode::NO_CONTENT);
        assert_eq!(
            refresh(&db, &session.refresh_token).await.unwrap_err(),
            StatusCode::UNAUTHORIZED
        );
    }

    #[tokio::test]
    async fn logout_answers_the_same_for_an_unknown_token() {
        let temp_dir = TempDir::default();
        let db = db_with_user(&temp_dir, Utc::now() + Duration::days(365)).await;

        let status = logout(
            State(db),
            Json(RefreshTokenRequest {
                refresh_token: "never-issued".to_owned(),
            }),
        )
        .await
        .unwrap();

        assert_eq!(status, StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn an_unknown_token_is_rejected() {
        let temp_dir = TempDir::default();
        let db = db_with_user(&temp_dir, Utc::now() + Duration::days(365)).await;

        assert_eq!(
            refresh(&db, "never-issued").await.unwrap_err(),
            StatusCode::UNAUTHORIZED
        );
    }

    #[tokio::test]
    async fn refreshing_fails_and_revokes_the_session_once_the_account_has_expired() {
        let temp_dir = TempDir::default();
        let db = db_with_user(&temp_dir, Utc::now() + Duration::days(365)).await;
        let session = sign_in(&db).await;
        sqlx::query("UPDATE users SET expiry = ? WHERE username = 'ada'")
            .bind(Utc::now() - Duration::seconds(1))
            .execute(&db.pool)
            .await
            .unwrap();

        assert_eq!(
            refresh(&db, &session.refresh_token).await.unwrap_err(),
            StatusCode::UNAUTHORIZED
        );
        let stored = db
            .get_refresh_token_by_hash(&super::hash_refresh_token(&session.refresh_token))
            .await
            .unwrap()
            .unwrap();
        assert!(stored.revoked_at.is_some());
    }

    #[tokio::test]
    async fn the_refresh_token_does_not_outlive_the_account() {
        let temp_dir = TempDir::default();
        let db = db_with_user(&temp_dir, Utc::now() + Duration::hours(2)).await;

        let session = sign_in(&db).await;

        assert!(session.refresh_token_expires_in <= 2 * 60 * 60);
        assert!(session.refresh_token_expires_in < AuthService::REFRESH_TOKEN_LIFETIME_SECONDS);
    }
}
