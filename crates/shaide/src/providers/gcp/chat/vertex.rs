use async_openai::{
    error::{OpenAIError, StreamError},
    types::chat::{
        ChatCompletionRequestAssistantMessageContent,
        ChatCompletionRequestAssistantMessageContentPart, ChatCompletionRequestMessage,
        ChatCompletionRequestSystemMessageContent, ChatCompletionRequestSystemMessageContentPart,
        ChatCompletionRequestUserMessageContent, ChatCompletionRequestUserMessageContentPart,
        CreateChatCompletionRequest,
    },
};
use google_cloud_aiplatform_v1::model::{Content, GenerateContentRequest, GenerationConfig, Part};
use reqwest_eventsource::{Event, EventSource};
use shaide_common::open_ai_types::ShaideChatCompletionStreamEvent;
use tokio::sync::mpsc::UnboundedSender;
use tokio_stream::StreamExt;

use crate::error::ShaideError;

fn map_open_ai_chat_completion_request_user_messsage_to_part(
    request: &ChatCompletionRequestUserMessageContent,
) -> Result<Vec<Part>, ShaideError> {
    match request {
        ChatCompletionRequestUserMessageContent::Text(x) => {
            let part = Part::new().set_text(x);
            Ok(vec![part])
        }
        ChatCompletionRequestUserMessageContent::Array(texts) => texts
            .iter()
            .map(|text| {
                let part = match text {
                    ChatCompletionRequestUserMessageContentPart::Text(text) => {
                        Part::new().set_text(&text.text)
                    }
                    ChatCompletionRequestUserMessageContentPart::ImageUrl(_) => {
                        return Err(ShaideError::bad_request(
                            "Vertex message mapping does not support image content".to_owned(),
                        ));
                    }
                    ChatCompletionRequestUserMessageContentPart::InputAudio(_) => {
                        return Err(ShaideError::bad_request(
                            "Vertex message mapping does not support audio input".to_owned(),
                        ));
                    }
                    ChatCompletionRequestUserMessageContentPart::File(_) => {
                        return Err(ShaideError::bad_request(
                            "Vertex message mapping does not support file input".to_owned(),
                        ));
                    }
                };
                Ok(part)
            })
            .collect(),
    }
}

fn map_open_ai_chat_completion_request_assistant_messsage_to_part(
    request: &ChatCompletionRequestAssistantMessageContent,
) -> Result<Vec<Part>, ShaideError> {
    match request {
        ChatCompletionRequestAssistantMessageContent::Text(x) => {
            let part = Part::new().set_text(x);
            Ok(vec![part])
        }
        ChatCompletionRequestAssistantMessageContent::Array(texts) => texts
            .iter()
            .map(|text| {
                let part = match text {
                    ChatCompletionRequestAssistantMessageContentPart::Text(text) => {
                        Part::new().set_text(&text.text)
                    }
                    ChatCompletionRequestAssistantMessageContentPart::Refusal(_) => {
                        return Err(ShaideError::bad_request(
                            "Vertex message mapping does not support assistant refusals".to_owned(),
                        ));
                    }
                };
                Ok(part)
            })
            .collect(),
    }
}

// NOTE: the google cloud API does not support streaming APIs for now.
pub fn map_open_ai_request_to_vertex(
    request: CreateChatCompletionRequest,
    max_tokens: u32,
) -> Result<GenerateContentRequest, ShaideError> {
    let mut generate_content_request = GenerateContentRequest::new();

    // NOTE: this is nice! We can tune a bunch of knobs here. Such as the temperature, top_p,
    // log_probs etc. This would give the users a sort of super user feeling
    let generation_config = GenerationConfig::new().set_max_output_tokens(max_tokens as i32);
    generate_content_request = generate_content_request.set_generation_config(generation_config);

    let mut contents: Vec<Content> = vec![];
    for msg in request.messages {
        match msg {
            ChatCompletionRequestMessage::User(user_req) => {
                let user_parts =
                    map_open_ai_chat_completion_request_user_messsage_to_part(&user_req.content)?;
                let content = Content::new().set_role("user").set_parts(user_parts);
                contents.push(content);
            }
            ChatCompletionRequestMessage::System(system_req) => match system_req.content {
                ChatCompletionRequestSystemMessageContent::Text(text) => {
                    let system_prompt = Content::new()
                        .set_role("model")
                        .set_parts(vec![Part::new().set_text(text)]);
                    generate_content_request =
                        generate_content_request.set_system_instruction(system_prompt);
                }
                ChatCompletionRequestSystemMessageContent::Array(texts) => {
                    let system_prompt =
                        Content::new().set_role("model").set_parts(texts.iter().map(
                            |ChatCompletionRequestSystemMessageContentPart::Text(t)| {
                                Part::new().set_text(&t.text)
                            },
                        ));
                    generate_content_request =
                        generate_content_request.set_system_instruction(system_prompt);
                }
            },
            ChatCompletionRequestMessage::Assistant(req) => {
                let Some(content) = &req.content else {
                    continue;
                };
                let assistant_parts =
                    map_open_ai_chat_completion_request_assistant_messsage_to_part(content)?;
                let content = Content::new().set_role("model").set_parts(assistant_parts);
                contents.push(content);
            }
            _ => {
                return Err(ShaideError::bad_request(
                    "Vertex message mapping only supports user/system/assistant roles".to_owned(),
                ));
            }
        }
    }
    generate_content_request = generate_content_request.set_contents(contents);

    // NOTE: this has no effect.
    generate_content_request = generate_content_request.set_model(request.model);
    Ok(generate_content_request)
}

pub async fn vertex_stream_handler(
    mut event_source: EventSource,
    mut event: Option<Result<Event, reqwest_eventsource::Error>>,
    tx: UnboundedSender<Result<ShaideChatCompletionStreamEvent, OpenAIError>>,
) {
    tokio::spawn(async move {
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
                Ok(event) => match event {
                    Event::Open => {}
                    Event::Message(message) => {
                        if message.data == "[DONE]" {
                            let _ = tx.send(Ok(ShaideChatCompletionStreamEvent::Done));
                            break;
                        }

                        let _ = tx.send(Err(OpenAIError::StreamError(Box::new(
                            StreamError::EventStream(format!(
                                "Vertex stream message format is unsupported: {}",
                                message.data
                            )),
                        ))));
                        break;
                    }
                },
            }
            event = event_source.next().await;
        }
    });
}
