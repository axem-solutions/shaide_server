use chrono::{DateTime, Utc};
use sqlx::{FromRow, query, query_as};

use super::DbConn;
use crate::error::ShaideDBError;

#[derive(FromRow, Debug, Clone)]
pub struct RefreshTokenDAO {
    pub id: i64,
    pub user_id: i64,
    pub family_id: String,
    pub expires_at: DateTime<Utc>,
    pub used_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
}

/// db read/write operations for `refresh_tokens` table
impl DbConn {
    pub async fn insert_refresh_token(
        &self,
        user_id: i64,
        family_id: &str,
        token_hash: &str,
        expires_at: DateTime<Utc>,
    ) -> Result<i64, ShaideDBError> {
        let res = query!(
            "INSERT INTO refresh_tokens (user_id, family_id, token_hash, expires_at) VALUES (?, ?, ?, ?)",
            user_id,
            family_id,
            token_hash,
            expires_at
        )
        .execute(&self.pool)
        .await?;
        Ok(res.last_insert_rowid())
    }

    pub async fn get_refresh_token_by_hash(
        &self,
        token_hash: &str,
    ) -> Result<Option<RefreshTokenDAO>, ShaideDBError> {
        let token = query_as!(
            RefreshTokenDAO,
            r#"
                SELECT
                    id as "id!",
                    user_id,
                    family_id,
                    expires_at as "expires_at!: DateTime<Utc>",
                    used_at as "used_at: DateTime<Utc>",
                    revoked_at as "revoked_at: DateTime<Utc>"
                FROM refresh_tokens WHERE token_hash = ?"#,
            token_hash
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(token)
    }

    /// Marks `token_id` as used and stores its successor in the same family, atomically.
    ///
    /// Returns `false` without storing the successor when `token_id` was already used or revoked,
    /// which happens when two requests race to rotate the same token.
    pub async fn rotate_refresh_token(
        &self,
        token_id: i64,
        successor_hash: &str,
        successor_expires_at: DateTime<Utc>,
    ) -> Result<bool, ShaideDBError> {
        let now = Utc::now();
        let mut transaction = self.pool.begin().await?;
        let claimed = query!(
            "UPDATE refresh_tokens SET used_at = ? WHERE id = ? AND used_at IS NULL AND revoked_at IS NULL",
            now,
            token_id
        )
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        if claimed != 1 {
            return Ok(false);
        }
        query!(
            r#"
                INSERT INTO refresh_tokens (user_id, family_id, token_hash, expires_at)
                SELECT user_id, family_id, ?, ? FROM refresh_tokens WHERE id = ?"#,
            successor_hash,
            successor_expires_at,
            token_id
        )
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(true)
    }

    /// Revokes every token of a login session that is not revoked yet.
    pub async fn revoke_refresh_token_family(&self, family_id: &str) -> Result<(), ShaideDBError> {
        let now = Utc::now();
        query!(
            "UPDATE refresh_tokens SET revoked_at = ? WHERE family_id = ? AND revoked_at IS NULL",
            now,
            family_id
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Deletes tokens that can no longer be exchanged. Kept until expiry even once used or
    /// revoked, so that presenting them is still recognised as reuse of a known token.
    pub async fn delete_expired_refresh_tokens(&self) -> Result<u64, ShaideDBError> {
        let now = Utc::now();
        let res = query!("DELETE FROM refresh_tokens WHERE expires_at <= ?", now)
            .execute(&self.pool)
            .await?;
        Ok(res.rows_affected())
    }
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, Utc};
    use temp_testdir::TempDir;

    use crate::DbConn;

    async fn db_with_user(temp_dir: &TempDir) -> (DbConn, i64) {
        let db = DbConn::new(&temp_dir.join("shaide-test.sqlite"))
            .await
            .expect("test database should be created");
        let user_id = db
            .create_user(
                "ada".to_owned(),
                "hash".to_owned(),
                Utc::now() + Duration::days(1),
            )
            .await
            .unwrap();
        (db, user_id)
    }

    #[tokio::test]
    async fn rotation_marks_the_token_used_and_stores_the_successor_in_the_same_family() {
        let temp_dir = TempDir::default();
        let (db, user_id) = db_with_user(&temp_dir).await;
        let expires_at = Utc::now() + Duration::days(30);
        let token_id = db
            .insert_refresh_token(user_id, "family-1", "hash-1", expires_at)
            .await
            .unwrap();

        assert!(
            db.rotate_refresh_token(token_id, "hash-2", expires_at)
                .await
                .unwrap()
        );

        let rotated = db
            .get_refresh_token_by_hash("hash-1")
            .await
            .unwrap()
            .unwrap();
        assert!(rotated.used_at.is_some());
        let successor = db
            .get_refresh_token_by_hash("hash-2")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(successor.family_id, "family-1");
        assert_eq!(successor.user_id, user_id);
        assert!(successor.used_at.is_none());
    }

    #[tokio::test]
    async fn a_token_can_be_rotated_only_once() {
        let temp_dir = TempDir::default();
        let (db, user_id) = db_with_user(&temp_dir).await;
        let expires_at = Utc::now() + Duration::days(30);
        let token_id = db
            .insert_refresh_token(user_id, "family-1", "hash-1", expires_at)
            .await
            .unwrap();

        assert!(
            db.rotate_refresh_token(token_id, "hash-2", expires_at)
                .await
                .unwrap()
        );
        assert!(
            !db.rotate_refresh_token(token_id, "hash-3", expires_at)
                .await
                .unwrap()
        );
        assert!(
            db.get_refresh_token_by_hash("hash-3")
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn revoking_a_family_blocks_rotation_and_leaves_other_families_alone() {
        let temp_dir = TempDir::default();
        let (db, user_id) = db_with_user(&temp_dir).await;
        let expires_at = Utc::now() + Duration::days(30);
        let revoked_id = db
            .insert_refresh_token(user_id, "family-1", "hash-1", expires_at)
            .await
            .unwrap();
        db.insert_refresh_token(user_id, "family-2", "hash-2", expires_at)
            .await
            .unwrap();

        db.revoke_refresh_token_family("family-1").await.unwrap();

        assert!(
            !db.rotate_refresh_token(revoked_id, "hash-3", expires_at)
                .await
                .unwrap()
        );
        let other = db
            .get_refresh_token_by_hash("hash-2")
            .await
            .unwrap()
            .unwrap();
        assert!(other.revoked_at.is_none());
    }

    #[tokio::test]
    async fn only_expired_tokens_are_deleted() {
        let temp_dir = TempDir::default();
        let (db, user_id) = db_with_user(&temp_dir).await;
        db.insert_refresh_token(
            user_id,
            "family-1",
            "expired",
            Utc::now() - Duration::seconds(1),
        )
        .await
        .unwrap();
        db.insert_refresh_token(user_id, "family-2", "valid", Utc::now() + Duration::days(1))
            .await
            .unwrap();

        assert_eq!(db.delete_expired_refresh_tokens().await.unwrap(), 1);
        assert!(
            db.get_refresh_token_by_hash("expired")
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            db.get_refresh_token_by_hash("valid")
                .await
                .unwrap()
                .is_some()
        );
    }
}
