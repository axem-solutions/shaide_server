use crate::{
    providers::shaide::{AxemClient, ShaideProviderError, router_retry},
    routes::completions::{NativeCompletionPayload, ProviderCompletionResponse},
};

impl AxemClient {
    pub async fn post_native_completion(
        &self,
        url: &str,
        payload: &NativeCompletionPayload,
    ) -> Result<ProviderCompletionResponse, ShaideProviderError> {
        let response = router_retry::send(|| {
            self.client
                .post(url)
                .header("Content-Type", "application/json; charset=utf-8")
                .json(payload)
        })
        .await?;
        let status_code = response.status();
        let body = response.text().await?;
        if !status_code.is_success() {
            Err(ShaideProviderError::HttpError {
                status_code,
                response_body: body,
            })
        } else {
            Ok(serde_json::from_str(&body)?)
        }
    }
}
