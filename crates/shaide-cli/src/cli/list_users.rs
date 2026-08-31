use anyhow::Result;
use clap::Parser;
use reqwest::Method;
use shaide_common::api::users::{ListUser, ListUsersResponse};

use crate::{cli::ExecuteServerCommand, shaide_client::ShaideClient};

#[derive(Parser)]
pub struct ListUsersArg;

impl ExecuteServerCommand for ListUsersArg {
    async fn execute_server_command(self, shaide_client: ShaideClient) -> Result<()> {
        let response = shaide_client
            .request("v1/users", Method::GET, None, vec![])
            .await?;
        let response_text = response.text().await?;
        let users_response: ListUsersResponse = serde_json::from_str(&response_text)?;
        let ListUsersResponse { users } = users_response;
        for ListUser {
            id,
            username,
            expiry,
        } in users
        {
            println!("User id: {id}, username: {username}, expiry: {expiry}");
        }
        Ok(())
    }
}
