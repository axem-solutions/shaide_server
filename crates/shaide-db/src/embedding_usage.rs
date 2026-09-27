use sqlx::query_scalar;

use crate::{DbConn, error::ShaideDBError};

impl DbConn {
    /// Input tokens `user` embedded with `embedding_model` on `date`.
    pub async fn get_embedding_daily_usage(
        &self,
        date: &str,
        user: i64,
        embedding_model: i64,
    ) -> Result<i64, ShaideDBError> {
        let total = query_scalar!(
            r#"SELECT total_input_token_count as "total!: i64"
            FROM daily_usage_embedding_token
            WHERE date = ? AND user = ? AND embedding_model = ?"#,
            date,
            user,
            embedding_model
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(total.unwrap_or(0))
    }

    /// Adds `input_tokens` to the day's usage in one statement, so concurrent
    /// requests cannot lose an update.
    pub async fn add_embedding_daily_usage(
        &self,
        date: &str,
        user: i64,
        embedding_model: i64,
        input_tokens: i64,
    ) -> Result<(), ShaideDBError> {
        sqlx::query!(
            r#"INSERT INTO daily_usage_embedding_token (date, user, embedding_model, total_input_token_count)
            VALUES (?, ?, ?, ?)
            ON CONFLICT (date, user, embedding_model) DO UPDATE SET
                total_input_token_count = total_input_token_count + excluded.total_input_token_count,
                updated_at = DATETIME('now')"#,
            date,
            user,
            embedding_model,
            input_tokens
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}
