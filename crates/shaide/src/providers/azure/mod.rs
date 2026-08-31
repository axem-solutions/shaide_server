mod chat;
mod embedding;

use std::sync::Arc;

use azure_core::credentials::TokenCredential;
use azure_identity::{DeveloperToolsCredential, WorkloadIdentityCredential};
use reqwest::Client;
use thiserror::Error;
use tokio::sync::OnceCell;
use tracing::debug;

const AZURE_AI_SCOPE: &str = "https://ai.azure.com/.default";
const AZURE_COGNITIVE_SERVICES_SCOPE: &str = "https://cognitiveservices.azure.com/.default";
const AZURE_FEDERATED_TOKEN_FILE: &str = "AZURE_FEDERATED_TOKEN_FILE";

#[derive(Debug, Error)]
pub enum AzureError {
    #[error("Could not build azure client")]
    Build(#[source] azure_core::Error),

    #[error("Credentials are insufficient")]
    Credentials(#[source] azure_core::Error),

    #[error("Http error {status_code}: {response_body}")]
    HttpError {
        status_code: hyper::StatusCode,
        response_body: String,
    },

    #[error("Azure request failed")]
    Request(#[source] reqwest::Error),

    #[error("Could not deserialize Azure response")]
    Deserialization(#[source] serde_json::Error),
}

pub struct AzureClient {
    client: Client,
    credentials: Arc<dyn TokenCredential>,
}

impl AzureClient {
    fn create(client: Client) -> Result<Self, AzureError> {
        let credentials: Arc<dyn TokenCredential> =
            if std::env::var_os(AZURE_FEDERATED_TOKEN_FILE).is_some() {
                WorkloadIdentityCredential::new(None).map_err(AzureError::Build)?
            } else {
                DeveloperToolsCredential::new(None).map_err(AzureError::Build)?
            };
        Ok(Self {
            client,
            credentials,
        })
    }

    pub async fn access_token(&self) -> Result<String, AzureError> {
        self.access_token_for_scope(AZURE_AI_SCOPE).await
    }

    pub async fn inference_access_token(&self) -> Result<String, AzureError> {
        self.access_token_for_scope(AZURE_COGNITIVE_SERVICES_SCOPE)
            .await
    }

    async fn access_token_for_scope(&self, scope: &str) -> Result<String, AzureError> {
        let access_token = self
            .credentials
            .get_token(&[scope], None)
            .await
            .map_err(AzureError::Credentials)?;
        Ok(access_token.token.secret().to_string())
    }

    pub fn client(&self) -> &Client {
        &self.client
    }
}

static AZURE_CLIENT: OnceCell<AzureClient> = OnceCell::const_new();

pub async fn get_azure_client() -> Result<&'static AzureClient, AzureError> {
    AZURE_CLIENT.get_or_try_init(build_azure_client).await
}

async fn build_azure_client() -> Result<AzureClient, AzureError> {
    debug!("Initializing Azure client");
    let client = Client::new();
    AzureClient::create(client)
}
