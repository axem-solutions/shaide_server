use chrono::{DateTime, Utc};
use sqlx::FromRow;
use sqlx_turso::{query_as, query_scalar};

use super::DbConn;
use crate::error::{Resource, ShaideDBError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserRole {
    User,
    Admin,
}

impl From<String> for UserRole {
    fn from(value: String) -> Self {
        match value.as_str() {
            "user" => Self::User,
            "admin" => Self::Admin,
            _ => unreachable!(),
        }
    }
}

impl From<UserRole> for String {
    fn from(val: UserRole) -> Self {
        match val {
            UserRole::User => "user".into(),
            UserRole::Admin => "admin".into(),
        }
    }
}

#[derive(FromRow, Debug, Clone)]
pub struct UserDAO {
    pub id: i64,
    pub username: String,
    pub password_hash: String,
    pub role: UserRole,
    pub expiry: DateTime<Utc>,
}

/// db read/write operations for `users` table
impl DbConn {
    pub async fn create_user(
        &self,
        username: String,
        password_hash: String,
        expiry: DateTime<Utc>,
    ) -> Result<i64, ShaideDBError> {
        let mut transaction = self.pool.begin().await?;
        let res = query_scalar!(
            r#"INSERT INTO users (username, password_hash, expiry) VALUES (?, ?, ?) RETURNING id as "id!: i64""#,
            username,
            password_hash,
            expiry
        )
        .fetch_one(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(res)
    }

    pub async fn create_admin(
        &self,
        username: String,
        password_hash: String,
    ) -> Result<i64, ShaideDBError> {
        let res = query_scalar!(
            r#"INSERT INTO users (username, password_hash, role) VALUES (?, ?, 'admin') RETURNING id as "id!: i64""#,
            username,
            password_hash
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(res)
    }

    pub async fn get_user_by_username(
        &self,
        username: &str,
    ) -> std::result::Result<UserDAO, ShaideDBError> {
        let username = String::from(username);
        let user = query_as!(
            UserDAO,
            r#"
            SELECT
                id as "id!: i64",
                username as "username!: String",
                password_hash as "password_hash!: String",
                role as "role!: String",
                expiry as "expiry!: DateTime<Utc>"
            FROM users WHERE username = ?"#,
            &username
        )
        .fetch_optional(&self.pool)
        .await?;
        if let Some(user) = user {
            Ok(user)
        } else {
            Err(ShaideDBError::NotFound(Resource::User))
        }
    }

    pub async fn get_user_by_id(&self, user_id: i64) -> Result<UserDAO, ShaideDBError> {
        query_as!(
            UserDAO,
            r#"
                SELECT
                    id as "id!: i64",
                    username as "username!: String",
                    password_hash as "password_hash!: String",
                    role as "role!: String",
                    expiry as "expiry!: DateTime<Utc>"
                FROM users WHERE id = ?"#,
            user_id
        )
        .fetch_optional(&self.pool)
        .await?
        .ok_or(ShaideDBError::NotFound(Resource::User))
    }

    pub async fn list_users(&self) -> Result<Vec<UserDAO>, ShaideDBError> {
        let users = query_as!(
            UserDAO,
            r#"
                SELECT
                    id as "id!: i64",
                    username as "username!: String",
                    password_hash as "password_hash!: String",
                    role as "role!: String",
                    expiry as "expiry!: DateTime<Utc>"
                FROM users"#
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(users)
    }
}
