use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Serialize, Deserialize, ToSchema, Clone, Debug)]
pub struct ListUser {
    pub id: i64,
    pub username: String,
    #[schema(value_type = String, format = DateTime)]
    pub expiry: DateTime<Utc>,
}

#[derive(Serialize, Deserialize, ToSchema, Clone, Debug)]
pub struct ListUsersResponse {
    pub users: Vec<ListUser>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct CreateUserRequest {
    pub username: String,
    pub password: String,
    #[schema(value_type = String, format = DateTime)]
    pub expiry: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct CreateUserResponse {
    pub id: i64,
    pub username: String,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct GenerateUsersRequest {
    pub number_of_new_users: usize,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct GenerateUsersResponse {
    pub new_users: Vec<GeneratedUserResponse>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct GeneratedUserResponse {
    pub id: i64,
    pub username: String,
    pub password: String,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct LoginStatusResponse {
    pub user_id: Option<i64>,
    pub is_admin: bool,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct AccessTokenResponse {
    pub access_token: String,
    pub token_type: String,
    pub expires_in: u64,
    #[schema(value_type = String, format = DateTime)]
    pub expiry: DateTime<Utc>,
}
