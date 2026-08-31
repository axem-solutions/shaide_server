use crate::{
    providers::gcp::{GcpClient, GcpError},
    routes::completions::{NativeCompletionPayload, ProviderCompletionResponse},
};

impl GcpClient {
    pub async fn post_native_completion(
        &self,
        url: &str,
        payload: &NativeCompletionPayload,
    ) -> Result<ProviderCompletionResponse, GcpError> {
        let token = self.access_token().await?;
        let request = self
            .client
            .post(url)
            .header("Content-Type", "application/json; charset=utf-8")
            .bearer_auth(token)
            .json(payload);
        let response = request.send().await?;
        let status_code = response.status();
        let body = response.text().await?;
        if !status_code.is_success() {
            Err(GcpError::UnexpectedResponse {
                status_code,
                response_body: body,
                service: "native-completion".into(),
            })
        } else {
            Ok(serde_json::from_str(&body)?)
        }
    }
}
