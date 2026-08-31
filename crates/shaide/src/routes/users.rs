use anyhow::Result;
use axum::{Json, Router, extract::State, routing};
use chrono::Utc;
use shaide_common::api::users::{
    AccessTokenResponse, CreateUserRequest, CreateUserResponse, GenerateUsersRequest,
    GenerateUsersResponse, GeneratedUserResponse, ListUser, ListUsersResponse, LoginRequest,
};
use shaide_db::DbConn;

use crate::{
    error::ShaideError,
    middlewares::authorize_admin::Admin,
    services::auth::{AuthService, get_auth_service},
};

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
    for _ in 0..request.number_of_new_users {
        let username = uuid::Uuid::new_v4().to_string();
        let password = uuid::Uuid::new_v4().to_string();
        let password_hash = get_auth_service().hash_password(password.clone()).await?;
        let id = db
            .create_user(username.clone(), password_hash, Utc::now())
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
    let expiry = user.expiry;
    get_auth_service()
        .verify_password(request.password, user.password_hash)
        .await?;
    let access_token = get_auth_service().issue_access_token(user.id)?;
    Ok(Json(AccessTokenResponse {
        access_token,
        token_type: "Bearer".to_owned(),
        expires_in: AuthService::TOKEN_LIFETIME_SECONDS,
        expiry,
    }))
}

pub fn users_router(db: DbConn) -> Router {
    Router::new()
        .route("/v1/login", routing::post(login))
        .route("/v1/users", routing::get(get_users))
        .route("/v1/user", routing::post(create_user))
        .route("/v1/generate-users", routing::post(generate_users))
        .with_state(db)
}
