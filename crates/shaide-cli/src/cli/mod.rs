mod add_user;
mod add_users;
mod create_embedding_model;
mod create_model;
mod delete_embedding_model;
mod delete_model;
mod generate_statistics;
mod list_embedding_models;
mod list_models;
mod list_users;
mod markdown_help;
mod set_model_daily_limit;

use add_user::AddUserArg;
use add_users::AddUsersArg;
use anyhow::Result;
use clap::{Args, Parser, Subcommand};
use delete_model::DeleteModelArgs;
use enum_dispatch::enum_dispatch;
use list_embedding_models::ListEmbeddingModelsArg;
use list_models::ListModelsArg;
use reqwest::Url;

use self::{
    create_embedding_model::CreateEmbeddingModelArgs, create_model::CreateModelArgs,
    delete_embedding_model::DeleteEmbeddingModelArgs, generate_statistics::GenerateStatistics,
    list_users::ListUsersArg, markdown_help::MarkdownHelp,
    set_model_daily_limit::SetModelDailyLimitArgs,
};
use crate::shaide_client::ShaideClient;

pub trait ExecuteServerCommand {
    async fn execute_server_command(self, shaide_client: ShaideClient) -> Result<()>;
}

#[enum_dispatch]
trait ExecuteCommand {
    async fn execute(self) -> Result<()>;
}

impl<T: ExecuteCommand> ExecuteCommand for Box<T> {
    async fn execute(self) -> Result<()> {
        (*self).execute().await
    }
}

#[derive(Debug, Clone, Args)]
pub struct AuthenticatedArg<T: Args + ExecuteServerCommand> {
    #[command(flatten)]
    pub arg: T,

    #[arg(long)]
    pub remote: String,

    /// Password for the built-in admin user.
    #[arg(long)]
    pub admin_password: String,
}

impl<T: Args + ExecuteServerCommand> ExecuteCommand for AuthenticatedArg<T> {
    async fn execute(self) -> Result<()> {
        let url = Url::parse(&self.remote)?;
        let shaide_client = ShaideClient::login(url, self.admin_password).await?;
        self.arg.execute_server_command(shaide_client).await
    }
}

/// Commands for managing users in the database.
#[derive(Subcommand)]
#[enum_dispatch(ExecuteCommand)]
pub enum CliCommands {
    /// Add new users to the database.
    AddUsers(AuthenticatedArg<AddUsersArg>),
    /// Add a single user to the database with the provided username
    AddUser(AuthenticatedArg<AddUserArg>),
    /// List all users in the database.
    ListUsers(AuthenticatedArg<ListUsersArg>),
    /// List the models in the DB
    ListModels(AuthenticatedArg<ListModelsArg>),
    /// List embedding models
    ListEmbeddingModels(AuthenticatedArg<ListEmbeddingModelsArg>),
    /// Deletes a model based on the ID
    DeleteModel(AuthenticatedArg<DeleteModelArgs>),
    /// Delete an embedding model based on the ID
    DeleteEmbeddingModel(AuthenticatedArg<DeleteEmbeddingModelArgs>),
    /// Create a model
    CreateModel(Box<AuthenticatedArg<CreateModelArgs>>),
    /// Create an embedding model
    CreateEmbeddingModel(AuthenticatedArg<CreateEmbeddingModelArgs>),
    /// Set a model daily limit
    SetModelDailyLimit(AuthenticatedArg<SetModelDailyLimitArgs>),
    /// Generates statistics
    GenerateStatistics(AuthenticatedArg<GenerateStatistics>),
    /// Prints the markdown help
    #[command(hide = true)]
    MarkdownHelp(MarkdownHelp),
}

#[derive(Parser)]
#[command(name = "shaide-cli", about = "Manage users in the database")]
pub struct Cli {
    #[command(subcommand)]
    pub command: CliCommands,
}

impl Cli {
    pub async fn execute(self) -> Result<()> {
        self.command.execute().await
    }
}
