use chrono::{DateTime, Utc};
use sqlx::{FromRow, query, query_as};

use super::DbConn;
use crate::error::{Resource, ShaideDBError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    User,
    Admin,
}

impl From<String> for Role {
    fn from(value: String) -> Self {
        match value.as_str() {
            "user" => Self::User,
            "admin" => Self::Admin,
            _ => unreachable!(),
        }
    }
}

#[derive(FromRow, Debug, Clone)]
pub struct UserDAO {
    pub id: i64,
    pub username: String,
    pub password_hash: String,
    pub role: Role,
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
        let res = query!(
            "INSERT INTO users (username, password_hash, expiry) VALUES (?, ?, ?)",
            username,
            password_hash,
            expiry
        )
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(res.last_insert_rowid())
    }

    pub async fn create_admin(
        &self,
        username: String,
        password_hash: String,
    ) -> Result<i64, ShaideDBError> {
        let res = query!(
            "INSERT INTO users (username, password_hash, role) VALUES (?, ?, 'admin')",
            username,
            password_hash
        )
        .execute(&self.pool)
        .await?;
        Ok(res.last_insert_rowid())
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
                id as "id!", 
                username,
                password_hash,
                role,
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
                    id as "id!",
                    username,
                    password_hash,
                    role,
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
                    id as "id!",
                    username,
                    password_hash,
                    role,
                    expiry as "expiry!: DateTime<Utc>"
                FROM users"#
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(users)
    }
}
