use anyhow::{Error, Result};
use reqwest::{Body, Client, Method, Request, RequestBuilder, Response, StatusCode, Url};
use shaide_common::api::{
    error::OpenAiErrorResponse,
    users::{AccessTokenResponse, LoginRequest},
};
use thiserror::Error;

#[derive(Debug, Error)]
enum ShaideClientError {
    #[error("Unauthorized access!")]
    Unauthorized,

    #[error("{0}")]
    Conflict(String),

    #[error("Unknown")]
    Unknown,
}

pub struct ShaideClient {
    client: Client,
    url: Url,
    token: String,
}

impl ShaideClient {
    pub async fn login(url: Url, password: String) -> Result<Self> {
        let client = reqwest::Client::new();
        let mut login_url = url.clone();
        login_url.set_path("/v1/login");
        let response = client
            .post(login_url)
            .json(&LoginRequest {
                username: "admin".to_owned(),
                password,
            })
            .send()
            .await?;
        if response.status() == StatusCode::UNAUTHORIZED {
            return Err(Error::new(ShaideClientError::Unauthorized));
        }
        if response.status() != StatusCode::OK {
            return Err(Error::new(ShaideClientError::Unknown));
        }
        let token: AccessTokenResponse = response.json().await?;
        Ok(Self {
            client,
            url,
            token: token.access_token,
        })
    }

    pub async fn request(
        &self,
        path: &str,
        method: Method,
        body: Option<Body>,
        query: Vec<(String, String)>,
    ) -> Result<Response> {
        let mut url = self.url.clone();
        url.set_path(path);
        for (name, value) in query {
            url.query_pairs_mut().append_pair(&name, &value);
        }
        let request = Request::new(method, url);
        let mut builder = RequestBuilder::from_parts(self.client.clone(), request);
        builder = builder.bearer_auth(&self.token);
        builder = builder.header("Content-Type", "application/json");
        if let Some(body) = body {
            builder = builder.body(body);
        }
        let response = builder.send().await?;
        match response.status() {
            StatusCode::OK => Ok(response),
            StatusCode::UNAUTHORIZED => Err(Error::new(ShaideClientError::Unauthorized)),
            StatusCode::CONFLICT => {
                let text = response.text().await?;
                let conflict: OpenAiErrorResponse = serde_json::from_str(&text)?;
                Err(Error::new(ShaideClientError::Conflict(
                    conflict.error.message,
                )))
            }
            _ => Err(Error::new(ShaideClientError::Unknown)),
        }
    }
}
