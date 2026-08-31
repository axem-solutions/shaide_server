pub mod chat;
pub mod completion;
pub mod embedding;

use reqwest::Client as ReqwestClient;
use thiserror::Error;
use tokio::sync::OnceCell;
use tracing::debug;

#[derive(Debug, Error)]
pub enum ShaideProviderError {
    #[error("Deserialization error: {0}")]
    DeserializationError(#[from] serde_json::Error),

    #[error("Request error")]
    Request(#[from] reqwest::Error),

    #[error("Http error {status_code}: {response_body}")]
    HttpError {
        status_code: hyper::StatusCode,
        response_body: String,
    },
}

pub struct AxemClient {
    client: ReqwestClient,
}

impl AxemClient {
    pub fn new(client: ReqwestClient) -> Self {
        Self { client }
    }

    pub fn client(&self) -> &ReqwestClient {
        &self.client
    }
}

static AXEM_CLIENT: OnceCell<AxemClient> = OnceCell::const_new();

async fn build_axem_client() -> AxemClient {
    debug!("Initializing shaide platform client");
    let client = ReqwestClient::new();
    AxemClient::new(client)
}

pub async fn get_axem_client() -> &'static AxemClient {
    AXEM_CLIENT.get_or_init(build_axem_client).await
}
