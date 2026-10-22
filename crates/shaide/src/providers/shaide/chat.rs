use async_openai::types::chat::CreateChatCompletionRequest;
use reqwest_eventsource::{Event, RequestBuilderExt};
use shaide_common::open_ai_types::ShaideChatCompletionResponseStream;
use shaide_db::ModelDAO;
use tokio_stream::StreamExt;

use crate::{
    providers::shaide::{AxemClient, ShaideProviderError, router_retry},
    utils::openai_stream_handler,
};

impl AxemClient {
    pub async fn stream_chat_completion(
        &self,
        request: CreateChatCompletionRequest,
        model: &ModelDAO,
    ) -> Result<ShaideChatCompletionResponseStream, ShaideProviderError> {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let mut attempt = 0;
        let (event_source, first_event) = loop {
            let mut event_source = self
                .client
                .post(&model.chat_completions_endpoint)
                .json(&request)
                .eventsource()
                .unwrap();
            let first_event: Option<Result<Event, reqwest_eventsource::Error>> =
                event_source.next().await;

            // The llm-d router answers before streaming starts, so a rejected
            // request has not produced any output yet and can be sent again.
            if let Some(Err(reqwest_eventsource::Error::InvalidStatusCode(_, response))) =
                &first_event
                && router_retry::should_retry(attempt, response)
            {
                event_source.close();
                tokio::time::sleep(router_retry::retry_delay(attempt)).await;
                attempt += 1;
                continue;
            }

            break (event_source, first_event);
        };
        match first_event {
            Some(Err(reqwest_eventsource::Error::InvalidStatusCode(status_code, response))) => {
                // Read the response body to get the actual error message
                let response_body = response
                    .text()
                    .await
                    .unwrap_or_else(|_| "Could not read response body".to_string());
                Err(ShaideProviderError::HttpError {
                    status_code,
                    response_body,
                })
            }
            _ => {
                openai_stream_handler::openai_stream_handler(event_source, first_event, tx).await;
                Ok(Box::pin(
                    tokio_stream::wrappers::UnboundedReceiverStream::new(rx),
                ))
            }
        }
    }
}
