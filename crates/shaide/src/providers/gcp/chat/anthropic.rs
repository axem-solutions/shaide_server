use async_openai::{
    error::{OpenAIError, StreamError},
    types::chat::{
        ChatCompletionRequestAssistantMessageContent,
        ChatCompletionRequestAssistantMessageContentPart, ChatCompletionRequestMessage,
        ChatCompletionRequestMessageContentPartText, ChatCompletionRequestSystemMessageContent,
        ChatCompletionRequestSystemMessageContentPart, ChatCompletionRequestUserMessageContent,
        ChatCompletionRequestUserMessageContentPart, CompletionUsage, CreateChatCompletionRequest,
        FinishReason,
    },
};
use reqwest_eventsource::{Event, EventSource};
use serde::{Deserialize, Serialize};
use shaide_common::open_ai_types::{
    CreateShaideChatCompletionStreamResponse, ShaideChatChoiceStream,
    ShaideChatCompletionStreamEvent, ShaideChatCompletionStreamResponseDelta,
};
use tokio::sync::mpsc::UnboundedSender;
use tokio_stream::StreamExt;
use url::Url;

use crate::error::ShaideError;

// Anthropic seems to add this at the end of the stream and it only contains the output tokens
#[derive(Debug, Deserialize, Clone, PartialEq, Serialize)]
pub struct AnthropicOutputUsage {
    output_tokens: Option<u32>,
}

// Here the output token is not the final one
#[derive(Debug, Deserialize, Clone, PartialEq, Serialize)]
pub struct AnthropicUsage {
    output_tokens: Option<u32>,
    input_tokens: Option<u32>,
}

#[derive(Debug, Deserialize, Clone, PartialEq, Serialize)]
pub struct AnthropicDelta {
    pub r#type: Option<String>,
    pub text: Option<String>,
    pub stop_reason: Option<String>,
}

#[derive(Debug, Deserialize, Clone, PartialEq, Serialize)]
pub struct AnthropicStreamMessage {
    pub id: Option<String>,
    usage: Option<AnthropicUsage>,
}

#[derive(Debug, Deserialize, Clone, PartialEq, Serialize)]
pub struct AnthropicStreamCompletionResponse {
    r#type: Option<String>,
    index: Option<i64>,
    delta: Option<AnthropicDelta>,
    message: Option<AnthropicStreamMessage>,
    usage: Option<AnthropicOutputUsage>,
}

impl AnthropicStreamCompletionResponse {
    pub fn stream_ending(&self) -> bool {
        if let Some(message_type) = &self.r#type {
            message_type == "message_stop"
        } else {
            false
        }
    }

    pub fn id(&self) -> Option<String> {
        if let Some(message) = &self.message {
            message.id.clone()
        } else {
            None
        }
    }

    pub fn text(&self) -> Option<String> {
        if let Some(delta) = &self.delta {
            delta.text.clone()
        } else {
            None
        }
    }

    pub fn input_tokens(&self) -> Option<u32> {
        if let Some(AnthropicStreamMessage {
            usage: Some(AnthropicUsage { input_tokens, .. }),
            ..
        }) = self.message
        {
            input_tokens
        } else {
            None
        }
    }

    pub fn output_tokens(&self) -> Option<u32> {
        if let Some(AnthropicOutputUsage { output_tokens }) = self.usage {
            output_tokens
        } else {
            None
        }
    }

    pub fn finish_reason(&self) -> Option<FinishReason> {
        let stop_reason = self.delta.as_ref()?.stop_reason.as_deref()?;
        Some(match stop_reason {
            "max_tokens" | "model_context_window" => FinishReason::Length,
            "tool_use" => FinishReason::ToolCalls,
            "refusal" => FinishReason::ContentFilter,
            _ => FinishReason::Stop,
        })
    }
}

#[derive(Serialize, Debug, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AnthropicImageSource {
    Url { url: String },
    Base64 { media_type: String, data: String },
}

#[derive(Serialize, Debug, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AnthropicMessageContent {
    Text { text: String },
    Image { source: AnthropicImageSource },
}

impl AnthropicMessageContent {
    pub fn text(text: String) -> Self {
        Self::Text { text }
    }

    pub fn image_url(url: String) -> Self {
        if let Some((media_type, data)) = parse_data_url_to_base64_source(&url) {
            return Self::Image {
                source: AnthropicImageSource::Base64 { media_type, data },
            };
        }

        Self::Image {
            source: AnthropicImageSource::Url { url },
        }
    }
}

fn parse_data_url_to_base64_source(url: &str) -> Option<(String, String)> {
    let url = Url::parse(url).ok()?;
    if url.scheme() != "data" {
        return None;
    }

    let (meta, data) = url.path().split_once(',')?;
    let mut meta_parts = meta.split(';');
    let mime_type = meta_parts.next()?;
    if !meta_parts.any(|part| part.eq_ignore_ascii_case("base64")) {
        None
    } else {
        Some((mime_type.to_owned(), data.to_owned()))
    }
}

#[derive(Serialize, Debug)]
pub struct AnthropicMessage {
    role: String,
    content: Vec<AnthropicMessageContent>,
}

impl AnthropicMessage {
    pub fn new(role: String, content: Vec<AnthropicMessageContent>) -> Self {
        Self { role, content }
    }
}

pub fn map_open_ai_chat_completion_request_user_messsage_to_anthropic_message(
    request: &ChatCompletionRequestUserMessageContent,
) -> Result<Vec<AnthropicMessageContent>, ShaideError> {
    match &request {
        ChatCompletionRequestUserMessageContent::Text(text) => {
            Ok(vec![AnthropicMessageContent::text(text.clone())])
        }
        ChatCompletionRequestUserMessageContent::Array(parts) => parts
            .iter()
            .map(|part| match part {
                ChatCompletionRequestUserMessageContentPart::Text(text) => {
                    Ok(AnthropicMessageContent::text(text.text.clone()))
                }
                ChatCompletionRequestUserMessageContentPart::ImageUrl(image) => Ok(
                    AnthropicMessageContent::image_url(image.image_url.url.clone()),
                ),
                ChatCompletionRequestUserMessageContentPart::InputAudio(_) => {
                    Err(ShaideError::bad_request(
                        "Anthropic message mapping does not support audio input".to_owned(),
                    ))
                }
                ChatCompletionRequestUserMessageContentPart::File(_) => {
                    Err(ShaideError::bad_request(
                        "Anthropic message mapping does not support file input".to_owned(),
                    ))
                }
            })
            .collect(),
    }
}

pub fn map_open_ai_chat_completion_request_assistant_to_anthropic_message(
    request: &ChatCompletionRequestAssistantMessageContent,
) -> Result<Vec<AnthropicMessageContent>, ShaideError> {
    match &request {
        ChatCompletionRequestAssistantMessageContent::Text(text) => {
            Ok(vec![AnthropicMessageContent::text(text.clone())])
        }
        ChatCompletionRequestAssistantMessageContent::Array(texts) => texts
            .iter()
            .map(|text| {
                let text = match text {
                    ChatCompletionRequestAssistantMessageContentPart::Text(text) => {
                        text.text.clone()
                    }
                    ChatCompletionRequestAssistantMessageContentPart::Refusal(_) => {
                        return Err(ShaideError::bad_request(
                            "Anthropic message mapping does not support assistant refusals"
                                .to_owned(),
                        ));
                    }
                };
                Ok(AnthropicMessageContent::text(text))
            })
            .collect(),
    }
}

pub fn map_open_ai_system_request_to_anthropic(
    request: &ChatCompletionRequestSystemMessageContent,
) -> String {
    match &request {
        ChatCompletionRequestSystemMessageContent::Text(text) => text.to_owned(),
        ChatCompletionRequestSystemMessageContent::Array(texts) => {
            let mut system_prompt = String::new();
            for ChatCompletionRequestSystemMessageContentPart::Text(
                ChatCompletionRequestMessageContentPartText { text },
            ) in texts
            {
                system_prompt.push_str(text);
            }
            system_prompt
        }
    }
}

pub fn map_open_ai_request_to_anthropic(
    request: CreateChatCompletionRequest,
    max_tokens: u32,
    stream: bool,
) -> Result<CreateChatCompletionAnthropicRequest, ShaideError> {
    let mut messages = vec![];
    let max_tokens = if let Some(request_max_completion_tokens) = request.max_completion_tokens {
        // NOTE: request max complection tokens has to be less than max tokens
        request_max_completion_tokens.min(max_tokens)
    } else {
        max_tokens
    };
    let mut system = None;
    for msg in request.messages {
        match msg {
            ChatCompletionRequestMessage::User(req) => messages.push(AnthropicMessage::new(
                "user".to_owned(),
                map_open_ai_chat_completion_request_user_messsage_to_anthropic_message(
                    &req.content,
                )?,
            )),
            ChatCompletionRequestMessage::System(req) => {
                system = Some(map_open_ai_system_request_to_anthropic(&req.content));
            }
            ChatCompletionRequestMessage::Assistant(req) => {
                let Some(content) = &req.content else {
                    continue;
                };
                messages.push(AnthropicMessage::new(
                    "assistant".to_owned(),
                    map_open_ai_chat_completion_request_assistant_to_anthropic_message(content)?,
                ));
            }
            _ => {
                return Err(ShaideError::bad_request(
                    "Anthropic message mapping only supports user/system/assistant roles"
                        .to_owned(),
                ));
            }
        }
    }
    Ok(CreateChatCompletionAnthropicRequest::new(
        "vertex-2023-10-16".to_owned(),
        messages,
        max_tokens,
        system,
        stream,
    ))
}

pub fn map_antrhropic_response_to_openai(
    id: String,
    response: AnthropicStreamCompletionResponse,
    input_tokens: u32,
    model: &str,
) -> Option<CreateShaideChatCompletionStreamResponse> {
    let content = response.text();
    let finish_reason = response.finish_reason();
    let choices = if content.is_some() || finish_reason.is_some() {
        vec![ShaideChatChoiceStream {
            index: 0,
            delta: ShaideChatCompletionStreamResponseDelta {
                content,
                reasoning_content: None,
                #[allow(deprecated)]
                function_call: None,
                tool_calls: None,
                role: None,
                refusal: None,
            },
            finish_reason,
            logprobs: None,
        }]
    } else {
        vec![]
    };
    let usage = response
        .output_tokens()
        .map(|output_tokens| CompletionUsage {
            prompt_tokens: input_tokens,
            completion_tokens: output_tokens,
            total_tokens: input_tokens + output_tokens,
            prompt_tokens_details: None,
            completion_tokens_details: None,
        });
    if !choices.is_empty() || usage.is_some() {
        Some(CreateShaideChatCompletionStreamResponse {
            id,
            choices,
            created: 0,
            model: model.into(),
            service_tier: None,
            #[allow(deprecated)]
            system_fingerprint: None,
            // NOTE: according to the docs, this is always `chat.completion.chunk`
            object: "chat.completion.chunk".into(),
            usage,
        })
    } else {
        None
    }
}

// NOTE: documentation based on https://docs.claude.com/en/api/messages
#[derive(Serialize, Debug)]
pub struct CreateChatCompletionAnthropicRequest {
    pub anthropic_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
    pub messages: Vec<AnthropicMessage>,
    pub max_tokens: u32,
    pub stream: bool,
}

impl CreateChatCompletionAnthropicRequest {
    pub fn new(
        anthropic_version: String,
        messages: Vec<AnthropicMessage>,
        max_tokens: u32,
        system: Option<String>,
        stream: bool,
    ) -> Self {
        Self {
            anthropic_version,
            system,
            messages,
            max_tokens,
            stream,
        }
    }
}

// TODO: this is a bit convoluted, maybe simplify
fn handle_event(
    tx: &UnboundedSender<Result<ShaideChatCompletionStreamEvent, OpenAIError>>,
    event: Event,
    message_id: &mut Option<String>,
    input_tokens: &mut u32,
    model: &str,
) -> bool {
    match event {
        Event::Message(message) => {
            let anthropic_response =
                match serde_json::from_str::<AnthropicStreamCompletionResponse>(&message.data) {
                    Ok(anthropic_response) => anthropic_response,
                    Err(e) => {
                        let _ = tx.send(Err(OpenAIError::JSONDeserialize(e, message.data)));
                        return false;
                    }
                };
            if let Some(new_input_tokens) = anthropic_response.input_tokens() {
                *input_tokens = new_input_tokens
            }
            if anthropic_response.stream_ending() {
                let _ = tx.send(Ok(ShaideChatCompletionStreamEvent::Done));
                return true;
            }
            let id = if let Some(id) = message_id {
                id.clone()
            } else if let Some(id) = anthropic_response.id() {
                *message_id = Some(id.clone());
                id.clone()
            } else {
                // should continue processing until an id is found
                return false;
            };
            if let Some(response) =
                map_antrhropic_response_to_openai(id, anthropic_response, *input_tokens, model)
            {
                tx.send(Ok(ShaideChatCompletionStreamEvent::Chunk(response)))
                    .unwrap();
            }
            false
        }
        Event::Open => false,
    }
}

pub async fn anthropic_stream_handler(
    mut event_source: EventSource,
    mut event: Option<Result<Event, reqwest_eventsource::Error>>,
    tx: UnboundedSender<Result<ShaideChatCompletionStreamEvent, OpenAIError>>,
    model: String,
) {
    tokio::spawn(async move {
        let mut message_id = None;
        let mut input_tokens = 0;
        while let Some(ev) = event {
            match ev {
                Err(e) => {
                    // in this case we should retry authenticating
                    if let Err(_e) = tx.send(Err(OpenAIError::StreamError(Box::new(
                        StreamError::EventStream(e.to_string()),
                    )))) {
                        break;
                    }
                }
                Ok(event) => {
                    if handle_event(&tx, event, &mut message_id, &mut input_tokens, &model) {
                        break;
                    }
                }
            }
            event = event_source.next().await;
        }
    });
}

#[cfg(test)]
mod tests {
    use async_openai::types::chat::{
        ChatCompletionRequestMessageContentPartImage, ChatCompletionRequestMessageContentPartText,
        ChatCompletionRequestUserMessageContent, ChatCompletionRequestUserMessageContentPart,
        ImageUrl,
    };
    use serde_json::json;

    use super::map_open_ai_chat_completion_request_user_messsage_to_anthropic_message;

    #[test]
    fn map_user_message_supports_image_url_content_for_anthropic() {
        let content = ChatCompletionRequestUserMessageContent::Array(vec![
            ChatCompletionRequestUserMessageContentPart::ImageUrl(
                ChatCompletionRequestMessageContentPartImage {
                    image_url: ImageUrl {
                        url: "https://example.com/image.png".to_owned(),
                        detail: None,
                    },
                },
            ),
            ChatCompletionRequestUserMessageContentPart::Text(
                ChatCompletionRequestMessageContentPartText {
                    text: "What is in the above image?".to_owned(),
                },
            ),
        ]);

        let mapped =
            map_open_ai_chat_completion_request_user_messsage_to_anthropic_message(&content)
                .expect("mapping should succeed");

        let as_json = serde_json::to_value(mapped).expect("serialize mapped content");
        assert_eq!(
            as_json,
            json!([
                {
                    "type": "image",
                    "source": {
                        "type": "url",
                        "url": "https://example.com/image.png"
                    }
                },
                {
                    "type": "text",
                    "text": "What is in the above image?"
                }
            ])
        );
    }

    #[test]
    fn map_user_message_maps_data_url_image_to_base64_source_for_anthropic() {
        let content = ChatCompletionRequestUserMessageContent::Array(vec![
            ChatCompletionRequestUserMessageContentPart::ImageUrl(
                ChatCompletionRequestMessageContentPartImage {
                    image_url: ImageUrl {
                        url: "data:image/png;base64,aGVsbG8=".to_owned(),
                        detail: None,
                    },
                },
            ),
            ChatCompletionRequestUserMessageContentPart::Text(
                ChatCompletionRequestMessageContentPartText {
                    text: "What is in this image?".to_owned(),
                },
            ),
        ]);

        let mapped =
            map_open_ai_chat_completion_request_user_messsage_to_anthropic_message(&content)
                .expect("mapping should succeed");

        let as_json = serde_json::to_value(mapped).expect("serialize mapped content");
        assert_eq!(
            as_json,
            json!([
                {
                    "type": "image",
                    "source": {
                        "type": "base64",
                        "media_type": "image/png",
                        "data": "aGVsbG8="
                    }
                },
                {
                    "type": "text",
                    "text": "What is in this image?"
                }
            ])
        );
    }

    #[test]
    fn map_user_message_maps_parameterized_data_url_image_to_base64_source_for_anthropic() {
        let content = ChatCompletionRequestUserMessageContent::Array(vec![
            ChatCompletionRequestUserMessageContentPart::ImageUrl(
                ChatCompletionRequestMessageContentPartImage {
                    image_url: ImageUrl {
                        url: "data:image/png;charset=utf-8;base64,aGVsbG8=".to_owned(),
                        detail: None,
                    },
                },
            ),
            ChatCompletionRequestUserMessageContentPart::Text(
                ChatCompletionRequestMessageContentPartText {
                    text: "What is in this image?".to_owned(),
                },
            ),
        ]);

        let mapped =
            map_open_ai_chat_completion_request_user_messsage_to_anthropic_message(&content)
                .expect("mapping should succeed");

        let as_json = serde_json::to_value(mapped).expect("serialize mapped content");
        assert_eq!(
            as_json,
            json!([
                {
                    "type": "image",
                    "source": {
                        "type": "base64",
                        "media_type": "image/png",
                        "data": "aGVsbG8="
                    }
                },
                {
                    "type": "text",
                    "text": "What is in this image?"
                }
            ])
        );
    }
}
