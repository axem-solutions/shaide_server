use async_openai::types::chat::CreateChatCompletionRequest;
use reqwest_eventsource::{Event, RequestBuilderExt};
use shaide_common::open_ai_types::ShaideChatCompletionResponseStream;
use shaide_db::ModelDAO;
use tokio_stream::StreamExt;

use crate::{
    providers::shaide::{AxemClient, ShaideProviderError},
    utils::openai_stream_handler,
};

impl AxemClient {
    pub async fn stream_chat_completion(
        &self,
        request: CreateChatCompletionRequest,
        model: &ModelDAO,
    ) -> Result<ShaideChatCompletionResponseStream, ShaideProviderError> {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let mut event_source = {
            self.client
                .post(&model.chat_completions_endpoint)
                .json(&request)
                .eventsource()
                .unwrap()
        };
        let first_event: Option<Result<Event, reqwest_eventsource::Error>> =
            event_source.next().await;
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
