use sqlx::{Sqlite, Transaction, query, query_as};

use crate::{DbConn, error::ShaideDBError};

pub struct DailyUsageDao {
    id: i64,
    pub total_input_token_count: i64,
    pub total_output_token_count: i64,
}

#[derive(Debug)]
pub struct UpsertDailyUsageDao {
    pub user: i64,
    pub model: i64,
    pub date: String,
    pub input_token_count: i64,
    pub output_token_count: i64,
}

#[derive(Debug)]
pub struct DailyUsageDaoWithModelName {
    pub date: String,
    pub user: i64,
    pub model: i64,
    pub total_input_token_count: i64,
    pub total_output_token_count: i64,
    pub model_name: String,
}

impl DbConn {
    pub async fn upsert_daily_usage_token_count(
        &self,
        upsert: UpsertDailyUsageDao,
    ) -> Result<(), ShaideDBError> {
        let mut transaction = self.pool.begin().await?;
        let id = self.ensure_daily_usage(&mut transaction, &upsert).await?;
        query!(
            "
                UPDATE daily_usage_token
                SET
                    total_input_token_count = total_input_token_count + ?,
                    total_output_token_count = total_output_token_count + ?
                WHERE id = ?
            ",
            upsert.input_token_count,
            upsert.output_token_count,
            id
        )
        .execute(transaction.as_mut())
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    async fn ensure_daily_usage(
        &self,
        transaction: &mut Transaction<'_, Sqlite>,
        insert_daily_usage: &UpsertDailyUsageDao,
    ) -> Result<i64, ShaideDBError> {
        let daily_usage = self
            .get_daily_usage_by_trx(
                transaction,
                &insert_daily_usage.date,
                insert_daily_usage.user,
                insert_daily_usage.model,
            )
            .await?;
        if let Some(daily_usage) = daily_usage {
            Ok(daily_usage.id)
        } else {
            struct DailyUsageId {
                id: i64,
            }
            let query_result = query_as!(
                DailyUsageId,
                r#"
                    INSERT INTO daily_usage_token (date, user, model) 
                    VALUES (?, ?, ?)
                    RETURNING id as "id!";
                "#,
                insert_daily_usage.date,
                insert_daily_usage.user,
                insert_daily_usage.model
            )
            .fetch_one(transaction.as_mut())
            .await?;
            Ok(query_result.id)
        }
    }

    pub async fn get_daily_usage(
        &self,
        date: &str,
        user: i64,
        model: i64,
    ) -> Result<Option<DailyUsageDao>, ShaideDBError> {
        let mut transaction = self.pool.begin().await?;
        let daily_usage = self
            .get_daily_usage_by_trx(&mut transaction, date, user, model)
            .await;
        transaction.commit().await?;
        daily_usage
    }

    async fn get_daily_usage_by_trx(
        &self,
        transaction: &mut Transaction<'_, Sqlite>,
        date: &str,
        user: i64,
        model: i64,
    ) -> Result<Option<DailyUsageDao>, ShaideDBError> {
        let daily_usage_dao = query_as!(
            DailyUsageDao,
            r#"
                SELECT 
                    id as "id!",
                    total_input_token_count,
                    total_output_token_count
                FROM daily_usage_token WHERE
                    user = ? AND
                    model = ? AND
                    date = ?;
            "#,
            user,
            model,
            date
        )
        .fetch_optional(transaction.as_mut())
        .await?;
        Ok(daily_usage_dao)
    }

    // TODO: we need to make sure that the start_date and the end_date should be parsed
    // correctly. The current implementation is a bit flimsy, but since this is currently not user
    // facing, it's not a huge issue. Still we need to harden this in the future
    pub async fn get_daily_usages(
        &self,
        start_date: &str,
        end_date: &str,
        limit: Option<i64>,
        skip: Option<i64>,
    ) -> Result<Vec<DailyUsageDaoWithModelName>, ShaideDBError> {
        let limit = limit.unwrap_or(-1);
        let skip = skip.unwrap_or(0);
        let daily_usage = query_as!(
            DailyUsageDaoWithModelName,
            r#"
                SELECT
                    daily_usage_token.date, 
                    daily_usage_token.user,
                    daily_usage_token.model,
                    daily_usage_token.total_input_token_count,
                    daily_usage_token.total_output_token_count,
                    models.name as "model_name!: String"
                FROM daily_usage_token 
                LEFT JOIN models on daily_usage_token.model = models.id
                WHERE date(daily_usage_token.date) >= date(?) AND date(daily_usage_token.date) <= date(?)
                LIMIT ? OFFSET ?
            "#,
            start_date,
            end_date,
            limit,
            skip
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(daily_usage)
    }

    pub async fn get_user_daily_usages(
        &self,
        date: &str,
        user_id: i64,
    ) -> Result<Vec<DailyUsageDaoWithModelName>, ShaideDBError> {
        let daily_usage = query_as!(
            DailyUsageDaoWithModelName,
            r#"
                SELECT
                    daily_usage_token.date, 
                    daily_usage_token.user,
                    daily_usage_token.model,
                    daily_usage_token.total_input_token_count,
                    daily_usage_token.total_output_token_count,
                    models.name as "model_name!: String"
                FROM daily_usage_token 
                LEFT JOIN models on daily_usage_token.model = models.id
                WHERE daily_usage_token.date = ? and daily_usage_token.user = ?
            "#,
            date,
            user_id
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(daily_usage)
    }
}
