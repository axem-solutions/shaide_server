use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

#[derive(Serialize, Deserialize, ToSchema, IntoParams)]
pub struct ModelUsageStatisticsQuery {
    pub limit: Option<i64>,
    pub skip: Option<i64>,
    pub start_date: String,
    pub end_date: String,
}

#[derive(Serialize, Deserialize, ToSchema)]
pub struct ModelUsageUserDailyStatistics {
    pub date: String,
    pub total_input_token_count: i64,
    pub total_output_token_count: i64,
    pub model_id: i64,
    pub model_name: String,
}

#[derive(Serialize, Deserialize, ToSchema)]
pub struct ModelUsageUserUsageStatistics {
    // NOTE: We do not have much else to show here for the user right now
    pub user_id: i64,
    pub model_usage_statistics: Vec<ModelUsageUserDailyStatistics>,
}

#[derive(Serialize, Deserialize, ToSchema)]
pub struct ModelUsageStatistics {
    // We may not need this!
    pub start_date: String,
    pub end_date: String,
    pub users_statistics: Vec<ModelUsageUserUsageStatistics>,
}

#[derive(Serialize, Deserialize, ToSchema, IntoParams)]
pub struct APIUsageStatisticsQuery {
    pub limit: Option<i64>,
    pub skip: Option<i64>,
    pub start_date: String,
    pub end_date: String,
}

#[derive(Serialize, Deserialize, ToSchema)]
pub struct ApiRequests {
    pub route: String,
    #[schema(value_type = String, format = DateTime)]
    pub request_made: DateTime<Utc>,
    pub model_name: Option<String>,
    pub input_token_count: Option<i64>,
    pub output_token_count: Option<i64>,
}

#[derive(Serialize, Deserialize, ToSchema)]
pub struct ApiUsageUserStatistics {
    pub user_id: i64,
    pub requests_made: usize,
    pub requests: Vec<ApiRequests>,
}

#[derive(Serialize, Deserialize, ToSchema)]
pub struct ApiUsageStatistics {
    pub start_date: String,
    pub end_date: String,
    pub api_usage_statistics: Vec<ApiUsageUserStatistics>,
}
