use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Serialize, Deserialize, ToSchema)]
pub struct ModelDailyLimit {
    pub id: i64,
    pub model_name: String,
    pub daily_input_token_limit: Option<i64>,
    pub daily_output_token_limit: Option<i64>,
    pub total_input_token_count: i64,
    pub total_output_token_count: i64,
}

#[derive(Serialize, Deserialize, ToSchema)]
pub struct UserDailyLimitsResponse {
    pub limits: Vec<ModelDailyLimit>,
}
