use anyhow::Result;
use clap::Parser;
use reqwest::Method;
use shaide_common::api::models::SetModelLimitsRequest;

use crate::{cli::ExecuteServerCommand, shaide_client::ShaideClient};

#[derive(Parser)]
pub struct SetModelDailyLimitArgs {
    #[arg(long)]
    pub model_name: String,

    #[arg(long = "input", short = 'i')]
    pub daily_input_token_limit: Option<i64>,

    #[arg(long = "output", short = 'o')]
    pub daily_output_token_limit: Option<i64>,
}

impl ExecuteServerCommand for SetModelDailyLimitArgs {
    async fn execute_server_command(self, shaide_client: ShaideClient) -> Result<()> {
        let SetModelDailyLimitArgs {
            model_name,
            daily_input_token_limit,
            daily_output_token_limit,
        } = self;
        let body = SetModelLimitsRequest {
            name: model_name,
            daily_input_token_limit,
            daily_output_token_limit,
        };
        let body = serde_json::to_string(&body)?;
        let _response = shaide_client
            .request("v1/model-limits", Method::PATCH, Some(body.into()), vec![])
            .await?;
        Ok(())
    }
}
