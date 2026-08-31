use async_openai::{
    error::{OpenAIError, StreamError},
    types::chat::CompletionUsage,
};
use reqwest_eventsource::{Event, EventSource};
use shaide_common::open_ai_types::{
    CreateShaideChatCompletionResponse, CreateShaideChatCompletionStreamResponse,
    ShaideChatChoiceStream, ShaideChatCompletionStreamEvent,
    ShaideChatCompletionStreamResponseDelta, ShaideCompletionUsage,
};
use tokio::sync::mpsc::UnboundedSender;
use tokio_stream::StreamExt;

fn handle_event(
    tx: &UnboundedSender<Result<ShaideChatCompletionStreamEvent, OpenAIError>>,
    event: Event,
) -> bool {
    match event {
        Event::Message(message) => {
            if message.data == "[DONE]" {
                let _ = tx.send(Ok(ShaideChatCompletionStreamEvent::Done));
                return true;
            }
            let response =
                match serde_json::from_str::<CreateShaideChatCompletionResponse>(&message.data) {
                    Ok(output) => Ok(output),
                    Err(err) => Err(OpenAIError::JSONDeserialize(err, message.data)),
                };

            if let Err(_e) = tx.send(response.map(|resp| {
                let usage = if let Some(ShaideCompletionUsage {
                    prompt_tokens: Some(prompt_tokens),
                    completion_tokens: Some(completion_tokens),
                    total_tokens: Some(total_tokens),
                    ..
                }) = resp.usage
                {
                    Some(CompletionUsage {
                        prompt_tokens,
                        completion_tokens,
                        total_tokens,
                        prompt_tokens_details: None,
                        completion_tokens_details: None,
                    })
                } else {
                    None
                };
                ShaideChatCompletionStreamEvent::Chunk(CreateShaideChatCompletionStreamResponse {
                    id: resp.id,
                    choices: resp
                        .choices
                        .into_iter()
                        .map(|choice| ShaideChatChoiceStream {
                            index: choice.index,
                            delta: ShaideChatCompletionStreamResponseDelta {
                                content: choice.delta.content,
                                reasoning_content: choice
                                    .delta
                                    .reasoning_content
                                    .or(choice.delta.reasoning),
                                #[allow(deprecated)]
                                function_call: choice.delta.function_call,
                                tool_calls: choice.delta.tool_calls,
                                role: choice.delta.role,
                                refusal: choice.delta.refusal,
                            },
                            finish_reason: choice.finish_reason,
                            logprobs: choice.logprobs,
                        })
                        .collect(),
                    created: resp.created,
                    model: resp.model,
                    service_tier: resp.service_tier,
                    system_fingerprint: resp.system_fingerprint,
                    object: resp.object,
                    usage,
                })
            })) {
                // rx dropped
                return true;
            }
            false
        }
        Event::Open => false,
    }
}

pub async fn openai_stream_handler(
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
                Ok(event) => {
                    if handle_event(&tx, event) {
                        break;
                    }
                }
            }
            event = event_source.next().await;
        }
    });
}
