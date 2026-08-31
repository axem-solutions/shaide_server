pub mod chat;
pub mod completions;
pub mod embedding;

use async_openai::types::chat::CompletionUsage;
use chrono::Local;
use google_cloud_auth::{
    credentials::{AccessTokenCredentials, Builder},
    errors::CredentialsError,
};
use reqwest::{Client, StatusCode};
use shaide_db::{
    DbConn, ModelDAO,
    api_usage::UpdateModelUsageDao,
    daily_usage::{DailyUsageDao, UpsertDailyUsageDao},
};
use thiserror::Error;
use tokio::sync::OnceCell;
use tracing::debug;

use crate::{error::ShaideError, utils::exponential_backoff::ExponentialBackoffBucket};

// NOTE: these are all not recovereble
#[derive(Debug, Error)]
pub enum GcpError {
    #[error("Could not build gcp client")]
    Build(#[from] google_cloud_auth::build_errors::Error),

    #[error("Credentials are insufficient")]
    Credentials(#[from] CredentialsError),

    #[error("Deserialization error: {0}")]
    DeserializationError(#[from] serde_json::Error),

    #[error("Bad request: {0}")]
    BadRequest(String),

    // NOTE: It is possible that we don't need this
    #[error("Unhandled http response")]
    UnexpectedResponse {
        status_code: StatusCode,
        response_body: String,
        service: String,
    },

    // NOTE: It is possible that we don't need this
    #[error("Request error")]
    Request(#[from] reqwest::Error),

    #[error("attemtted to create the completion stream too many times")]
    MaxRetriesHaveBeenAchieved { service: String },
}

pub struct GcpClient {
    client: Client,
    credentials: AccessTokenCredentials,
    backoff_bucket: ExponentialBackoffBucket,
}

impl GcpClient {
    fn create(client: Client) -> Result<Self, GcpError> {
        let credentials = Builder::default()
            .with_scopes(["https://www.googleapis.com/auth/cloud-platform"])
            .build_access_token_credentials()?;
        Ok(Self {
            client,
            credentials,
            backoff_bucket: ExponentialBackoffBucket::default(),
        })
    }

    pub async fn access_token(&self) -> Result<String, CredentialsError> {
        let access_token = self.credentials.access_token().await?;
        Ok(access_token.token)
    }

    pub fn client(&self) -> &Client {
        &self.client
    }
}

static GCP_CLIENT: OnceCell<GcpClient> = OnceCell::const_new();

pub async fn get_gcp_client() -> Result<&'static GcpClient, GcpError> {
    GCP_CLIENT.get_or_try_init(build_gcp_client).await
}

async fn build_gcp_client() -> Result<GcpClient, GcpError> {
    debug!("Initializing GCP client");
    let client = Client::new();
    GcpClient::create(client)
}

// TODO: should be somewhere else
pub async fn check_user_model_usage(
    db: DbConn,
    user_id: i64,
    model: &ModelDAO,
) -> Result<(), ShaideError> {
    let ModelDAO {
        id: model_id,
        daily_input_token_limit,
        daily_output_token_limit,
        name: model_name,
        ..
    } = model;
    let daily_input_token_limit = daily_input_token_limit.unwrap_or(i64::MAX);
    let daily_output_token_limit = daily_output_token_limit.unwrap_or(i64::MAX);
    let date = Local::now().format("%Y-%m-%d").to_string();
    let Some(DailyUsageDao {
        total_input_token_count,
        total_output_token_count,
        ..
    }) = db.get_daily_usage(&date, user_id, *model_id).await?
    else {
        return Ok(());
    };
    if daily_input_token_limit <= total_input_token_count
        || daily_output_token_limit <= total_output_token_count
    {
        Err(ShaideError::model_usage_limit_reached(format!(
            "Model usage reached on model: {}",
            model_name
        )))
    } else {
        Ok(())
    }
}

// TODO: should be somewhere else
pub async fn try_update_model_usage(
    db: DbConn,
    user_id: i64,
    request_id: i64,
    model_id: i64,
    usage: Option<&CompletionUsage>,
) -> Result<(), ShaideError> {
    if let Some(CompletionUsage {
        prompt_tokens,
        completion_tokens,
        ..
    }) = usage
    {
        let date = Local::now().format("%Y-%m-%d").to_string();
        // TODO: sometimes running these concurrently will lock the db unpredictably. It's kinda
        // funny how this is a problem in our lord 2025
        db.upsert_daily_usage_token_count(UpsertDailyUsageDao {
            user: user_id,
            model: model_id,
            date,
            input_token_count: *prompt_tokens as i64,
            output_token_count: *completion_tokens as i64,
        })
        .await?;
        db.update_model_usage(
            request_id,
            UpdateModelUsageDao {
                model: model_id,
                input_token_count: Some(*prompt_tokens as i64),
                output_token_count: Some(*completion_tokens as i64),
            },
        )
        .await?;
    }
    Ok(())
}
