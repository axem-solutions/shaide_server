mod cli;
mod shaide_client;

use std::{env, fs::File, io::Write};

use anyhow::Result;
use clap::Parser;
use serde::Serialize;
use shaide_common::api::statistics::{ApiUsageStatistics, ModelUsageStatistics};

use crate::cli::Cli;

fn write_statistics(model_usage_stats: ModelUsageStatistics, api_usage_stats: ApiUsageStatistics) {
    #[derive(Serialize)]
    struct Statistics {
        model_usage_stats: ModelUsageStatistics,
        api_usage_stats: ApiUsageStatistics,
    }
    let statistics = Statistics {
        model_usage_stats,
        api_usage_stats,
    };
    let content = serde_json::to_string(&statistics).unwrap();
    let mut statistics_path = env::current_dir().unwrap();
    statistics_path.push("statistics.json");
    if let Ok(mut file) = File::create(statistics_path) {
        file.write_all(content.as_bytes()).unwrap();
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    cli.execute().await
}
