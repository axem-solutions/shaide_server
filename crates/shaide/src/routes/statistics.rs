use std::collections::HashMap;

use axum::{
    Json, Router,
    extract::{Query, State},
    routing,
};
use chrono::Local;
use shaide_common::api::{
    statistics::{
        APIUsageStatisticsQuery, ApiRequests, ApiUsageStatistics, ApiUsageUserStatistics,
        ModelUsageStatistics, ModelUsageStatisticsQuery, ModelUsageUserDailyStatistics,
        ModelUsageUserUsageStatistics,
    },
    user_daily_limits::{ModelDailyLimit, UserDailyLimitsResponse},
};
use shaide_db::{DbConn, api_usage::ApiUsersUsage, daily_usage::DailyUsageDaoWithModelName};

use crate::{
    error::ShaideError,
    middlewares::{authorize_admin::Admin, authorize_user::AuthUser},
};

pub fn map_db_to_usage_statistics(
    db_statistics: Vec<DailyUsageDaoWithModelName>,
    start_date: String,
    end_date: String,
) -> ModelUsageStatistics {
    let mut hm: HashMap<i64, Vec<ModelUsageUserDailyStatistics>> = HashMap::new();
    for st in db_statistics {
        let user_id = st.user;
        let usage_statistics = ModelUsageUserDailyStatistics {
            date: st.date,
            total_input_token_count: st.total_input_token_count,
            total_output_token_count: st.total_output_token_count,
            model_id: st.model,
            model_name: st.model_name,
        };
        if let Some(user_stats) = hm.get_mut(&user_id) {
            user_stats.push(usage_statistics);
        } else {
            hm.insert(user_id, vec![usage_statistics]);
        }
    }
    let users_statistics = hm
        .into_iter()
        .map(
            |(user_id, daily_statistics)| ModelUsageUserUsageStatistics {
                user_id,
                model_usage_statistics: daily_statistics,
            },
        )
        .collect();
    ModelUsageStatistics {
        start_date,
        end_date,
        users_statistics,
    }
}

pub fn map_db_api_usage_statistics(
    api_statistics: ApiUsersUsage,
    start_date: String,
    end_date: String,
) -> ApiUsageStatistics {
    let mut hm: HashMap<i64, Vec<ApiRequests>> = HashMap::new();
    for st in api_statistics.api_usages {
        let user_id = st.user;
        let api_request = ApiRequests {
            route: st.route,
            request_made: st.request_made,
            model_name: st.model_name,
            input_token_count: st.input_token_count,
            output_token_count: st.output_token_count,
        };
        if let Some(user_stats) = hm.get_mut(&user_id) {
            user_stats.push(api_request);
        } else {
            hm.insert(user_id, vec![api_request]);
        }
    }
    let api_usage_statistics = hm
        .into_iter()
        .map(|(user_id, api_statistics)| ApiUsageUserStatistics {
            user_id,
            requests_made: api_statistics.len(),
            requests: api_statistics,
        })
        .collect();
    ApiUsageStatistics {
        start_date,
        end_date,
        api_usage_statistics,
    }
}

#[utoipa::path(
    get,
    path = "/v1/statistics/model-daily-usage",
    tag = "statistics",
    params(ModelUsageStatisticsQuery),
    responses((status = 200, description = "Model usage statistics", body = ModelUsageStatistics)),
    security(("bearer_token" = []))
)]
pub async fn model_daily_usage_statistics(
    _admin: Admin,
    Query(usage_statistics_query): Query<ModelUsageStatisticsQuery>,
    State(db): State<DbConn>,
) -> Result<Json<ModelUsageStatistics>, ShaideError> {
    let ModelUsageStatisticsQuery {
        start_date,
        end_date,
        limit,
        skip,
    } = usage_statistics_query;
    let db_statistics = db
        .get_daily_usages(&start_date, &end_date, limit, skip)
        .await?;
    let usage_satatistics = map_db_to_usage_statistics(db_statistics, start_date, end_date);
    Ok(Json(usage_satatistics))
}

#[utoipa::path(
    get,
    path = "/v1/statistics/api-usage-statistics",
    tag = "statistics",
    params(APIUsageStatisticsQuery),
    responses((status = 200, description = "API usage statistics", body = ApiUsageStatistics)),
    security(("bearer_token" = []))
)]
pub async fn api_usage_statistics(
    _admin: Admin,
    Query(api_usage_statistics_query): Query<APIUsageStatisticsQuery>,
    State(db): State<DbConn>,
) -> Result<Json<ApiUsageStatistics>, ShaideError> {
    let APIUsageStatisticsQuery {
        limit,
        skip,
        start_date,
        end_date,
    } = api_usage_statistics_query;
    let db_statistics = db
        .get_user_api_statistics(&start_date, &end_date, limit, skip)
        .await?;
    let api_statistics = map_db_api_usage_statistics(db_statistics, start_date, end_date);
    Ok(Json(api_statistics))
}

#[utoipa::path(
    get,
    path = "/v1/user-daily-limits",
    tag = "statistics",
    responses((status = 200, description = "Current user's daily limits", body = UserDailyLimitsResponse)),
    security(("bearer_token" = []))
)]
pub async fn user_daily_usage(
    auth: AuthUser,
    State(db): State<DbConn>,
) -> Result<Json<UserDailyLimitsResponse>, ShaideError> {
    let date = Local::now().format("%Y-%m-%d").to_string();
    let models = db.list_models().await?;
    let daily_usages = db.get_user_daily_usages(&date, auth.user.id).await?;

    let limits = models
        .into_iter()
        .map(|model| {
            if let Some(daily_usage) = daily_usages.iter().find(|du| du.model == model.id) {
                ModelDailyLimit {
                    id: model.id,
                    model_name: model.name,
                    daily_input_token_limit: model.daily_input_token_limit,
                    daily_output_token_limit: model.daily_output_token_limit,
                    total_input_token_count: daily_usage.total_input_token_count,
                    total_output_token_count: daily_usage.total_output_token_count,
                }
            } else {
                ModelDailyLimit {
                    id: model.id,
                    model_name: model.name,
                    daily_input_token_limit: model.daily_input_token_limit,
                    daily_output_token_limit: model.daily_output_token_limit,
                    total_input_token_count: 0,
                    total_output_token_count: 0,
                }
            }
        })
        .collect::<Vec<_>>();
    Ok(Json(UserDailyLimitsResponse { limits }))
}

pub fn statistics_router(db: DbConn) -> Router {
    Router::new()
        .route("/v1/user-daily-limits", routing::get(user_daily_usage))
        .route(
            "/v1/statistics/model-daily-usage",
            routing::get(model_daily_usage_statistics),
        )
        .route(
            "/v1/statistics/api-usage-statistics",
            routing::get(api_usage_statistics),
        )
        .with_state(db)
}
