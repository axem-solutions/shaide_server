//! Fixed-target, authenticated KC transport. No object-storage provider here.
use crate::error::ShaideError;
use axum::body::Body;
use http::{HeaderMap, Method, StatusCode};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use std::time::Duration;
use url::Url;

pub const MAX_UPLOAD_BYTES: usize = 16 * 1024 * 1024;
#[derive(Clone)]
pub struct KcConfig {
    url: String,
    token: String,
    webhook_token: String,
}
impl std::fmt::Debug for KcConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KcConfig").field("enabled", &true).finish()
    }
}
impl KcConfig {
    pub fn uses_token(&self, token: &str) -> bool {
        self.token == token || self.webhook_token == token
    }
    pub fn parse(
        url: Option<&str>,
        token: Option<&str>,
        webhook_token: Option<&str>,
        allow_http: bool,
    ) -> Result<Option<Self>, &'static str> {
        let Some(raw) = url.filter(|s| !s.is_empty()) else {
            if token.is_some() || webhook_token.is_some() {
                return Err("KC tokens require KNOWLEDGE_CENTER_URL");
            }
            return Ok(None);
        };
        let url = Url::parse(raw).map_err(|_| "invalid KC URL")?;
        if url.host_str().is_none()
            || !(url.scheme() == "https" || (url.scheme() == "http" && allow_http))
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || url.path() != "/"
        {
            return Err("KC URL must be an origin; HTTP needs explicit local opt-in");
        }
        let token = token
            .filter(|v| v.len() >= 32 && !v.chars().any(char::is_whitespace))
            .ok_or("KC internal token must have at least 32 bytes")?;
        let webhook_token = webhook_token
            .filter(|v| v.len() >= 32 && !v.chars().any(char::is_whitespace))
            .ok_or("KC webhook token must have at least 32 bytes")?;
        if token == webhook_token {
            return Err("internal and notification tokens must differ");
        }
        Ok(Some(Self {
            url: url.as_str().trim_end_matches('/').into(),
            token: token.into(),
            webhook_token: webhook_token.into(),
        }))
    }
}
#[derive(Clone)]
pub struct KcClient {
    config: KcConfig,
    http: reqwest::Client,
}
#[derive(Debug)]
pub struct KcFailure {
    pub code: &'static str,
    pub status: StatusCode,
}
impl KcFailure {
    fn unknown() -> Self {
        Self {
            code: "kc_outcome_unknown",
            status: StatusCode::SERVICE_UNAVAILABLE,
        }
    }
    pub fn public(self) -> ShaideError {
        ShaideError::request_rejection(
            self.status,
            "Knowledge Center operation failed; reconciliation may be required".into(),
            self.code,
        )
    }
}
impl KcClient {
    pub fn new(config: KcConfig) -> Result<Self, &'static str> {
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(180))
            .build()
            .map_err(|_| "could not initialize KC transport")?;
        Ok(Self { config, http })
    }
    pub fn authentic_notification(&self, headers: &HeaderMap) -> bool {
        let Some(raw) = headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
        else {
            return false;
        };
        // Fixed-length constant-work comparison; never format either secret.
        let expected = self.config.webhook_token.as_bytes();
        let actual = raw.as_bytes();
        if expected.len() != actual.len() {
            return false;
        }
        expected
            .iter()
            .zip(actual)
            .fold(0u8, |acc, (a, b)| acc | (a ^ b))
            == 0
    }
    fn request(&self, method: Method, path: &str) -> reqwest::RequestBuilder {
        self.http
            .request(method, format!("{}{path}", self.config.url))
            .bearer_auth(&self.config.token)
    }
    async fn checked(
        response: Result<reqwest::Response, reqwest::Error>,
    ) -> Result<reqwest::Response, KcFailure> {
        let response = response.map_err(|_| KcFailure::unknown())?;
        if matches!(response.status(), StatusCode::OK | StatusCode::NO_CONTENT) {
            return Ok(response);
        }
        let status = response.status();
        let bytes = Self::bounded_response(response).await.unwrap_or_default();
        let v: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        let raw = v
            .get("error_code")
            .or_else(|| v.get("error"))
            .and_then(Value::as_str);
        let code = match raw {
            Some("invalid_request") => "invalid_request",
            Some("invalid_upload_signature") => "invalid_upload_signature",
            Some("object_not_found") => "object_not_found",
            Some("lifecycle_busy") => "lifecycle_busy",
            Some("processing_timeout") => "processing_timeout",
            Some("resource_limit") => "resource_limit",
            Some("storage_unavailable") => "storage_unavailable",
            Some("extraction_failed") => "extraction_failed",
            Some("empty_document") => "empty_document",
            Some("collection_dimension_mismatch") => "collection_dimension_mismatch",
            Some("vector_store_unavailable") => "vector_store_unavailable",
            Some("collection_not_found") => "collection_not_found",
            Some("quota_exceeded") => "quota_exceeded",
            Some("embedding_auth") => "embedding_auth",
            Some("embedding_quota") => "embedding_quota",
            Some("embedding_rejected") => "embedding_rejected",
            Some("embedding_model_not_found") => "embedding_model_not_found",
            Some("embedding_unavailable") => "embedding_unavailable",
            Some("embedding_invalid_response") => "embedding_invalid_response",
            _ => "kc_outcome_unknown",
        };
        Err(KcFailure {
            code,
            status: if status.is_client_error()
                && ((status != StatusCode::UNAUTHORIZED && status != StatusCode::FORBIDDEN)
                    || matches!(code, "invalid_upload_signature" | "embedding_quota"))
            {
                status
            } else {
                StatusCode::SERVICE_UNAVAILABLE
            },
        })
    }
    async fn bounded_response(mut response: reqwest::Response) -> Result<Vec<u8>, KcFailure> {
        let mut data = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| KcFailure::unknown())? {
            if data.len() + chunk.len() > 64 * 1024 {
                return Err(KcFailure::unknown());
            }
            data.extend_from_slice(&chunk);
        }
        Ok(data)
    }
    pub async fn json<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<T, KcFailure> {
        let mut request = self.request(method, path);
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = Self::checked(request.send().await).await?;
        serde_json::from_slice(&Self::bounded_response(response).await?)
            .map_err(|_| KcFailure::unknown())
    }
    /// The frozen collection acknowledgement is exactly an empty JSON object.
    pub async fn collection_ack(
        &self,
        method: Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<(), KcFailure> {
        let acknowledgement: Value = self.json(method, path, body).await?;
        if acknowledgement != json!({}) {
            return Err(KcFailure::unknown());
        }
        Ok(())
    }
    pub async fn presign(
        &self,
        collection: &str,
        hash: &str,
        content_type: &str,
    ) -> Result<String, KcFailure> {
        let v:Value=self.json(Method::POST,"/internal/v1/storage/presigned-url",Some(json!({"collection":collection,"key":hash,"content_type":content_type,"expires_in_seconds":300}))).await?;
        let url = v["presigned_url"].as_str().ok_or_else(KcFailure::unknown)?;
        let prefix = format!("/s3/{collection}/{hash}?");
        if !url.starts_with(&prefix) || url.contains('#') || url.len() > 4096 {
            return Err(KcFailure::unknown());
        }
        let parsed =
            Url::parse(&format!("http://local.invalid{url}")).map_err(|_| KcFailure::unknown())?;
        let expiry: Vec<_> = parsed
            .query_pairs()
            .filter(|(k, _)| k == "X-Amz-Expires")
            .collect();
        if expiry.len() != 1 || expiry[0].1 != "300" {
            return Err(KcFailure::unknown());
        }
        Ok(url.into())
    }
    pub async fn upload(
        &self,
        collection: &str,
        hash: &str,
        query: &str,
        headers: &HeaderMap,
        body: Body,
    ) -> Result<StatusCode, KcFailure> {
        let path = format!("/internal/v1/storage/upload/{collection}/{hash}?{query}");
        let mut request = self.request(Method::PUT, &path);
        for (name, value) in headers {
            if matches!(
                name.as_str(),
                "content-type" | "content-length" | "content-md5"
            ) || name.as_str().starts_with("x-amz-")
            {
                request = request.header(name, value);
            }
        }
        // Bound actual bytes as well as any declared length. No buffering of an upload.
        use futures::StreamExt;
        let mut stream = body.into_data_stream();
        let bounded: std::pin::Pin<
            Box<dyn futures::Stream<Item = Result<axum::body::Bytes, std::io::Error>> + Send>,
        > = Box::pin(async_stream::try_stream! {
            let mut size=0usize;
            while let Some(chunk)=stream.next().await {
                let chunk=chunk.map_err(|_|std::io::Error::other("upload body failed"))?;
                size=size.checked_add(chunk.len()).ok_or_else(||std::io::Error::other("upload limit exceeded"))?;
                if size>MAX_UPLOAD_BYTES {Err(std::io::Error::other("upload limit exceeded"))?;}
                yield chunk;
            }
        });
        let response = Self::checked(
            request
                .body(reqwest::Body::wrap_stream(bounded))
                .send()
                .await,
        )
        .await?;
        let status = response.status();
        Self::bounded_response(response).await?;
        Ok(status)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn configuration_is_optional_and_rejects_credential_urls_and_shared_secrets() {
        assert!(KcConfig::parse(None, None, None, false).unwrap().is_none());
        let token = "synthetic-internal-token-32-bytes-long";
        let hook = "synthetic-webhook-token-32-bytes-long";
        for url in [
            "http://localhost:5",
            "https://u:p@localhost",
            "https://localhost?token=x",
            "https://localhost/base",
            "file:///tmp/kc",
        ] {
            assert!(KcConfig::parse(Some(url), Some(token), Some(hook), false).is_err());
        }
        assert!(
            KcConfig::parse(Some("https://localhost"), Some(token), Some(token), false).is_err()
        );
        let c = KcConfig::parse(Some("https://localhost"), Some(token), Some(hook), false)
            .unwrap()
            .unwrap();
        assert!(!format!("{c:?}").contains(token));
        assert!(!format!("{c:?}").contains(hook));
    }
}
