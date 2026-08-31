use axum::{Json, Router, routing};
use shaide_common::api::trial::TrialResponse;

use crate::{config::get_environment_config, error::ShaideError};

#[utoipa::path(
    get,
    path = "/v1/trial",
    tag = "trial",
    responses((status = 200, description = "Trial state", body = TrialResponse))
)]
pub async fn is_trial() -> Result<Json<TrialResponse>, ShaideError> {
    let is_trial = get_environment_config().is_trial;
    Ok(Json(TrialResponse { is_trial }))
}

pub fn trial_router() -> Router {
    Router::new().route("/v1/trial", routing::get(is_trial))
}
