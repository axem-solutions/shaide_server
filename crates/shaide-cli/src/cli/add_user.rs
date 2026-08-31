use anyhow::Result;
use chrono::{DateTime, Utc};
use clap::Parser;
use reqwest::Method;
use shaide_common::api::users::{CreateUserRequest, CreateUserResponse};

use crate::{cli::ExecuteServerCommand, shaide_client::ShaideClient};

#[derive(Parser)]
pub struct AddUserArg {
    pub username: String,
    pub password: String,
    #[arg(long)]
    pub expiry: DateTime<Utc>,
}

impl ExecuteServerCommand for AddUserArg {
    async fn execute_server_command(self, shaide_client: ShaideClient) -> Result<()> {
        let AddUserArg {
            username,
            password,
            expiry,
        } = self;
        let body = CreateUserRequest {
            username,
            password,
            expiry,
        };
        let body = serde_json::to_string(&body)?;
        let response = shaide_client
            .request("v1/user", Method::POST, Some(body.into()), vec![])
            .await?;
        let response_text = response.text().await?;
        let CreateUserResponse { id, username } = serde_json::from_str(&response_text)?;
        println!("User created with id: {id}, username: {username}");
        Ok(())
    }
}
