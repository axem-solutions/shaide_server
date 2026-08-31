use sqlx::{query, query_as};

use crate::{DbConn, error::ShaideDBError};

#[derive(Debug, Clone)]
pub struct EmbeddingModelDao {
    pub id: i64,
    pub url: String,
    pub name: String,
    pub vector_size: i64,
    pub platform: Option<String>,
    pub max_embedding_model_text_len: Option<i64>,
}

#[derive(Clone)]
pub struct InsertEmbeddingModelDao {
    pub url: String,
    pub name: String,
    pub vector_size: i64,
    pub platform: Option<String>,
    pub api_schema: Option<String>,
    pub max_embedding_model_text_len: Option<i64>,
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
                max_embedding_model_text_len
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
            "INSERT INTO embedding_models (url, name, vector_size, platform, api_schema, max_embedding_model_text_len) VALUES (?, ?, ?, ?, ?, ?)",
            embedding_model.url,
            embedding_model.name,
            embedding_model.vector_size,
            embedding_model.platform,
            embedding_model.api_schema,
            embedding_model.max_embedding_model_text_len
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
                max_embedding_model_text_len
            FROM embedding_models WHERE id= ?"#,
            embedding_model_id
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(model)
    }
}
