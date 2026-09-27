use sqlx::{query, query_as};

use crate::{DbConn, error::ShaideDBError};

#[derive(Debug, Clone)]
pub struct EmbeddingModelDao {
    pub id: i64,
    pub url: String,
    pub name: String,
    pub vector_size: i64,
    pub platform: Option<String>,
    /// Daily input tokens each user may embed with this model. `None` is unlimited.
    pub daily_input_token_limit: Option<i64>,
}

#[derive(Clone)]
pub struct InsertEmbeddingModelDao {
    pub url: String,
    pub name: String,
    pub vector_size: i64,
    pub platform: Option<String>,
    pub api_schema: Option<String>,
    pub daily_input_token_limit: Option<i64>,
}

impl DbConn {
    pub async fn list_embedding_models(&self) -> Result<Vec<EmbeddingModelDao>, ShaideDBError> {
        let models = query_as!(
            EmbeddingModelDao,
            r#"SELECT
                id as "id!",
                vector_size,
                name as "name: String",
                url as "url: String",
                platform,
                daily_input_token_limit
            FROM embedding_models"#,
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(models)
    }

    pub async fn insert_embedding_model(
        &self,
        embedding_model: InsertEmbeddingModelDao,
    ) -> Result<i64, ShaideDBError> {
        let res = query!(
            "INSERT INTO embedding_models (url, name, vector_size, platform, api_schema, daily_input_token_limit) VALUES (?, ?, ?, ?, ?, ?)",
            embedding_model.url,
            embedding_model.name,
            embedding_model.vector_size,
            embedding_model.platform,
            embedding_model.api_schema,
            embedding_model.daily_input_token_limit
        )
        .execute(&self.pool)
        .await?;
        Ok(res.last_insert_rowid())
    }

    pub async fn delete_embedding_model(
        &self,
        embedding_model_id: i64,
    ) -> Result<(), ShaideDBError> {
        query!(
            "DELETE FROM embedding_models where id = ?",
            embedding_model_id
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn get_embedding_model(
        &self,
        embedding_model_id: i64,
    ) -> Result<EmbeddingModelDao, ShaideDBError> {
        let model = query_as!(
            EmbeddingModelDao,
            r#"SELECT
                id as "id!",
                vector_size,
                name as "name: String",
                url as "url: String",
                platform,
                daily_input_token_limit
            FROM embedding_models WHERE id= ?"#,
            embedding_model_id
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(model)
    }

    /// Sets the daily input token limit of the embedding model called `name`;
    /// `None` removes it. Returns whether a model with that name exists.
    pub async fn set_embedding_model_limit(
        &self,
        name: &str,
        daily_input_token_limit: Option<i64>,
    ) -> Result<bool, ShaideDBError> {
        let result = query!(
            "UPDATE embedding_models SET daily_input_token_limit = ?, updated_at = DATETIME('now') WHERE name = ?",
            daily_input_token_limit,
            name
        )
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }
}
