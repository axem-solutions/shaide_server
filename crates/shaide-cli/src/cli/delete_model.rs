use anyhow::Result;
use clap::Parser;
use reqwest::Method;
use shaide_common::api::models::DeleteModelRequest;

use crate::{cli::ExecuteServerCommand, shaide_client::ShaideClient};

#[derive(Parser)]
pub struct DeleteModelArgs {
    #[arg(long)]
    pub id: i64,
}

impl ExecuteServerCommand for DeleteModelArgs {
    async fn execute_server_command(self, shaide_client: ShaideClient) -> Result<()> {
        let id = self.id;
        let body = DeleteModelRequest { model_id: id };
        let body = serde_json::to_string(&body)?;
        shaide_client
            .request("v1/models", Method::DELETE, Some(body.into()), vec![])
            .await?;
        println!("model with id {id} deleted");
        Ok(())
    }
}
