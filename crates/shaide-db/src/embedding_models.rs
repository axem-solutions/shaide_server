use sqlx_turso::{query, query_as, query_scalar};

use crate::{DbConn, error::ShaideDBError};

#[derive(Debug, Clone)]
pub struct EmbeddingModelDao {
    pub id: i64,
    pub url: String,
    pub name: String,
    pub vector_size: i64,
    pub platform: Option<String>,
}

#[derive(Clone)]
pub struct InsertEmbeddingModelDao {
    pub url: String,
    pub name: String,
    pub vector_size: i64,
    pub platform: Option<String>,
    pub api_schema: Option<String>,
}

impl DbConn {
    pub async fn list_embedding_models(&self) -> Result<Vec<EmbeddingModelDao>, ShaideDBError> {
        let models = query_as!(
            EmbeddingModelDao,
            r#"SELECT
                id as "id!: i64",
                vector_size as "vector_size!: i64",
                name as "name!: String",
                url as "url!: String",
                platform as "platform?: String"
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
        let res = query_scalar!(
            r#"INSERT INTO embedding_models (url, name, vector_size, platform, api_schema) VALUES (?, ?, ?, ?, ?) RETURNING id as "id!: i64""#,
            embedding_model.url,
            embedding_model.name,
            embedding_model.vector_size,
            embedding_model.platform,
            embedding_model.api_schema
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(res)
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
                id as "id!: i64",
                vector_size as "vector_size!: i64",
                name as "name!: String",
                url as "url!: String",
                platform as "platform?: String"
            FROM embedding_models WHERE id= ?"#,
            embedding_model_id
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(model)
    }
}
