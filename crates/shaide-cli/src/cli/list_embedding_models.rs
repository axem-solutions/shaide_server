use anyhow::Result;
use clap::Parser;
use reqwest::Method;
use shaide_common::api::embedding_models::{ListEmbeddingModel, ListEmbeddingModelsResponse};

use crate::{cli::ExecuteServerCommand, shaide_client::ShaideClient};

#[derive(Parser)]
pub struct ListEmbeddingModelsArg;

impl ExecuteServerCommand for ListEmbeddingModelsArg {
    async fn execute_server_command(self, shaide_client: ShaideClient) -> Result<()> {
        let response = shaide_client
            .request("v1/embedding_models", Method::GET, None, vec![])
            .await?;
        let response_text = response.text().await?;
        let models_response: ListEmbeddingModelsResponse = serde_json::from_str(&response_text)?;
        for ListEmbeddingModel {
            id,
            name,
            daily_input_token_limit,
        } in models_response.models
        {
            let limit = daily_input_token_limit
                .map_or_else(|| "unlimited".to_owned(), |limit| limit.to_string());
            println!("Model id: {id} Model name: {name} Daily input token limit: {limit}")
        }
        Ok(())
    }
}
