use anyhow::Result;
use clap::Parser;
use reqwest::Method;
use shaide_common::api::statistics::{ApiUsageStatistics, ModelUsageStatistics};

use crate::{cli::ExecuteServerCommand, shaide_client::ShaideClient, write_statistics};

#[derive(Parser)]
pub struct GenerateStatistics {
    #[arg(long)]
    pub start_date: String,

    #[arg(long)]
    pub end_date: String,
}

impl ExecuteServerCommand for GenerateStatistics {
    async fn execute_server_command(self, shaide_client: ShaideClient) -> Result<()> {
        let model_daily_usage_url = "v1/statistics/model-daily-usage".to_string();
        let madel_daily_usage = shaide_client
            .request(
                &model_daily_usage_url,
                Method::GET,
                None,
                vec![
                    ("start_date".to_string(), self.start_date.clone()),
                    ("end_date".to_string(), self.end_date.clone()),
                ],
            )
            .await?;
        let model_daily_usage_text = madel_daily_usage.text().await?;
        let model_usage_stats: ModelUsageStatistics =
            serde_json::from_str(&model_daily_usage_text).unwrap();

        let api_usage_url = "v1/statistics/api-usage-statistics".to_string();
        let api_usage_statistics = shaide_client
            .request(
                &api_usage_url,
                Method::GET,
                None,
                vec![
                    ("start_date".to_string(), self.start_date),
                    ("end_date".to_string(), self.end_date),
                ],
            )
            .await?;
        let api_usage_statistics = api_usage_statistics.text().await?;
        let api_usage_stats: ApiUsageStatistics =
            serde_json::from_str(&api_usage_statistics).unwrap();
        write_statistics(model_usage_stats, api_usage_stats);
        Ok(())
    }
}
