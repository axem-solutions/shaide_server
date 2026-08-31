use anyhow::Result;
use clap::Parser;
use reqwest::Method;
use shaide_common::api::users::{
    GenerateUsersRequest, GenerateUsersResponse, GeneratedUserResponse,
};

use crate::{cli::ExecuteServerCommand, shaide_client::ShaideClient};

/// Add one or more users to the database.
#[derive(Parser)]
pub struct AddUsersArg {
    /// The number of users to add.
    pub number_of_users: usize,
}

impl ExecuteServerCommand for AddUsersArg {
    async fn execute_server_command(self, shaide_client: ShaideClient) -> Result<()> {
        // let users_response = shaide_cli. } = serde_json::from_str(&users_response_text)?;
        let body = GenerateUsersRequest {
            number_of_new_users: self.number_of_users,
        };
        let body = serde_json::to_string(&body)?;
        let response = shaide_client
            .request("v1/generate-users", Method::POST, Some(body.into()), vec![])
            .await?;
        let response_text = response.text().await?;
        let GenerateUsersResponse { new_users } = serde_json::from_str(&response_text)?;
        for GeneratedUserResponse {
            id,
            username,
            password,
        } in &new_users
        {
            println!("User id: {id}, username: {username}, password: {password}");
        }
        Ok(())
    }
}
