use chrono::{DateTime, Utc};
use sqlx::{query, query_as};

use crate::{DbConn, error::ShaideDBError};

pub struct ApiUsageDaoWithModelName {
    pub route: String,
    pub user: i64,
    pub request_made: DateTime<Utc>,
    pub input_token_count: Option<i64>,
    pub output_token_count: Option<i64>,
    pub model_name: Option<String>,
}

pub struct InsertApiUsageDao {
    pub route: String,
    pub user: i64,
}

#[derive(Debug)]
pub struct UpdateModelUsageDao {
    pub model: i64,
    pub input_token_count: Option<i64>,
    pub output_token_count: Option<i64>,
}

pub struct ApiUsersUsage {
    pub api_usages: Vec<ApiUsageDaoWithModelName>,
}

impl DbConn {
    pub async fn insert_api_usage(
        &self,
        insert_api_usage: InsertApiUsageDao,
    ) -> Result<i64, ShaideDBError> {
        let InsertApiUsageDao { route, user } = insert_api_usage;
        let mut transaction = self.pool.begin().await?;
        let res = query!(
            r#"
                INSERT INTO api_usage (route, user) 
                VALUES (?, ?);
            "#,
            route,
            user,
        )
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(res.last_insert_rowid())
    }

    pub async fn update_model_usage(
        &self,
        request_id: i64,
        update: UpdateModelUsageDao,
    ) -> Result<(), ShaideDBError> {
        let UpdateModelUsageDao {
            model,
            input_token_count,
            output_token_count,
        } = update;
        let mut transaction = self.pool.begin().await?;
        query!(
            r#"UPDATE api_usage SET model = ?, input_token_count = ?, output_token_count = ? where id = ?"#,
            model,
            input_token_count,
            output_token_count,
            request_id
        )
        .execute(transaction.as_mut())
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    pub async fn get_user_api_statistics(
        &self,
        start_date: &str,
        end_date: &str,
        limit: Option<i64>,
        skip: Option<i64>,
    ) -> Result<ApiUsersUsage, ShaideDBError> {
        let limit = limit.unwrap_or(-1);
        let skip = skip.unwrap_or(0);
        let api_usages = query_as!(
            ApiUsageDaoWithModelName,
            r#"
                SELECT
                    api_usage.route,
                    api_usage.user,
                    api_usage.request_made as "request_made!: DateTime<Utc>",
                    api_usage.input_token_count,
                    api_usage.output_token_count,
                    models.name as "model_name!: String"
                FROM api_usage 
                LEFT JOIN models on api_usage.model = models.id
                WHERE api_usage.request_made >= date(?) AND api_usage.request_made <= date(?)
                LIMIT ? OFFSET ?
            "#,
            start_date,
            end_date,
            limit,
            skip
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(ApiUsersUsage { api_usages })
    }
}
