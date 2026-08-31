use anyhow::Result;
use clap::Parser;
use reqwest::Method;
use shaide_common::api::embedding_models::DeleteEmbeddingModelRequest;

use crate::{cli::ExecuteServerCommand, shaide_client::ShaideClient};

#[derive(Parser)]
pub struct DeleteEmbeddingModelArgs {
    #[arg(long)]
    pub id: i64,
}

impl ExecuteServerCommand for DeleteEmbeddingModelArgs {
    async fn execute_server_command(self, shaide_client: ShaideClient) -> Result<()> {
        let id = self.id;
        let body = DeleteEmbeddingModelRequest { id };
        let body = serde_json::to_string(&body)?;
        shaide_client
            .request(
                "v1/embedding_model",
                Method::DELETE,
                Some(body.into()),
                vec![],
            )
            .await?;
        println!("embedding model with id {id} deleted");
        Ok(())
    }
}
