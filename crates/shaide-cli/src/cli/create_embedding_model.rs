use anyhow::Result;
use clap::Parser;
use reqwest::Method;
use shaide_common::api::embedding_models::{
    InsertEmbeddingModelRequest, InsertEmbeddingModelResponse,
};

use crate::{cli::ExecuteServerCommand, shaide_client::ShaideClient};

#[derive(Parser)]
pub struct CreateEmbeddingModelArgs {
    #[arg(long)]
    pub url: String,

    #[arg(long)]
    pub name: String,

    #[arg(long)]
    pub vector_size: i64,

    #[arg(long)]
    pub platform: String,

    #[arg(long)]
    pub api_schema: Option<String>,
}

impl CreateEmbeddingModelArgs {
    pub fn into_api_request(self) -> InsertEmbeddingModelRequest {
        InsertEmbeddingModelRequest {
            url: self.url,
            name: self.name,
            vector_size: self.vector_size,
            platform: Some(self.platform),
            api_schema: self.api_schema,
        }
    }
}

impl ExecuteServerCommand for CreateEmbeddingModelArgs {
    async fn execute_server_command(self, shaide_client: ShaideClient) -> Result<()> {
        let body = self.into_api_request();
        let body = serde_json::to_string(&body)?;
        let response = shaide_client
            .request(
                "v1/embedding_model",
                Method::POST,
                Some(body.into()),
                vec![],
            )
            .await?;
        let response_text = response.text().await?;
        let InsertEmbeddingModelResponse { id: model_id } = serde_json::from_str(&response_text)?;
        println!("Embedding model with id {model_id} created");
        Ok(())
    }
}
