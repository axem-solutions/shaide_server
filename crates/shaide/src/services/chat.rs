use std::collections::BTreeMap;

use async_openai::types::chat::{
    ChatChoiceLogprobs, ChatCompletionMessageToolCall, ChatCompletionMessageToolCalls,
    ChatCompletionResponseMessage, CompletionUsage, CreateChatCompletionRequest, FinishReason,
    FunctionCall, FunctionCallStream, Role, ServiceTier,
};
use serde::Serialize;
use shaide_common::open_ai_types::{
    CreateShaideChatCompletionStreamResponse, ShaideChatChoiceStream,
    ShaideChatCompletionResponseStream, ShaideChatCompletionStreamEvent,
};
use shaide_db::ModelDAO;
use tokio_stream::StreamExt;
use tracing::warn;

use crate::{
    error::ShaideError,
    providers::{azure::get_azure_client, gcp::get_gcp_client, shaide::get_axem_client},
};

fn append_fragment(target: &mut Option<String>, fragment: Option<String>) {
    if let Some(fragment) = fragment {
        target.get_or_insert_with(String::new).push_str(&fragment);
    }
}

fn append_items<T>(target: &mut Option<Vec<T>>, items: Option<Vec<T>>) {
    if let Some(items) = items {
        target.get_or_insert_with(Vec::new).extend(items);
    }
}

#[derive(Default)]
struct FunctionCallAccumulator {
    name: Option<String>,
    arguments: Option<String>,
}

impl FunctionCallAccumulator {
    fn push(&mut self, function: FunctionCallStream) {
        append_fragment(&mut self.name, function.name);
        append_fragment(&mut self.arguments, function.arguments);
    }

    fn finish(self) -> Option<FunctionCall> {
        (self.name.is_some() || self.arguments.is_some()).then(|| FunctionCall {
            name: self.name.unwrap_or_default(),
            arguments: self.arguments.unwrap_or_default(),
        })
    }
}

#[derive(Default)]
struct ToolCallAccumulator {
    id: Option<String>,
    function: FunctionCallAccumulator,
}

impl ToolCallAccumulator {
    fn push(&mut self, id: Option<String>, function: Option<FunctionCallStream>) {
        if id.is_some() {
            self.id = id;
        }
        if let Some(function) = function {
            self.function.push(function);
        }
    }

    fn finish(self) -> ChatCompletionMessageToolCalls {
        ChatCompletionMessageToolCalls::Function(ChatCompletionMessageToolCall {
            id: self.id.unwrap_or_default(),
            function: self.function.finish().unwrap_or_default(),
        })
    }
}

#[derive(Serialize)]
struct ShaideChatCompletionResponseMessage {
    #[serde(flatten)]
    openai: ChatCompletionResponseMessage,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_content: Option<String>,
}

#[derive(Serialize)]
struct ShaideChatChoice {
    index: u32,
    message: ShaideChatCompletionResponseMessage,
    #[serde(skip_serializing_if = "Option::is_none")]
    finish_reason: Option<FinishReason>,
    #[serde(skip_serializing_if = "Option::is_none")]
    logprobs: Option<ChatChoiceLogprobs>,
}

#[derive(Default)]
struct ChoiceAccumulator {
    content: Option<String>,
    reasoning_content: Option<String>,
    refusal: Option<String>,
    role: Option<Role>,
    function_call: FunctionCallAccumulator,
    tool_calls: BTreeMap<u32, ToolCallAccumulator>,
    finish_reason: Option<FinishReason>,
    logprobs: Option<ChatChoiceLogprobs>,
}

impl ChoiceAccumulator {
    fn push(&mut self, choice: ShaideChatChoiceStream) {
        append_fragment(&mut self.content, choice.delta.content);
        append_fragment(&mut self.reasoning_content, choice.delta.reasoning_content);
        append_fragment(&mut self.refusal, choice.delta.refusal);
        if choice.delta.role.is_some() {
            self.role = choice.delta.role;
        }
        #[allow(deprecated)]
        if let Some(function_call) = choice.delta.function_call {
            self.function_call.push(function_call);
        }
        if let Some(tool_calls) = choice.delta.tool_calls {
            for tool_call in tool_calls {
                self.tool_calls
                    .entry(tool_call.index)
                    .or_default()
                    .push(tool_call.id, tool_call.function);
            }
        }
        if choice.finish_reason.is_some() {
            self.finish_reason = choice.finish_reason;
        }
        if let Some(logprobs) = choice.logprobs {
            let accumulated = self.logprobs.get_or_insert(ChatChoiceLogprobs {
                content: None,
                refusal: None,
            });
            append_items(&mut accumulated.content, logprobs.content);
            append_items(&mut accumulated.refusal, logprobs.refusal);
        }
    }

    fn finish(self, index: u32) -> ShaideChatChoice {
        #[allow(deprecated)]
        let openai = ChatCompletionResponseMessage {
            content: self.content,
            refusal: self.refusal,
            tool_calls: (!self.tool_calls.is_empty()).then(|| {
                self.tool_calls
                    .into_values()
                    .map(ToolCallAccumulator::finish)
                    .collect()
            }),
            annotations: None,
            role: self.role.unwrap_or(Role::Assistant),
            function_call: self.function_call.finish(),
            audio: None,
        };

        ShaideChatChoice {
            index,
            message: ShaideChatCompletionResponseMessage {
                openai,
                reasoning_content: self.reasoning_content,
            },
            finish_reason: self.finish_reason,
            logprobs: self.logprobs,
        }
    }
}

#[derive(Serialize)]
pub struct CollectedChatCompletion {
    id: String,
    choices: Vec<ShaideChatChoice>,
    created: u32,
    model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    service_tier: Option<ServiceTier>,
    #[serde(skip_serializing_if = "Option::is_none")]
    system_fingerprint: Option<String>,
    object: String,
    pub usage: Option<CompletionUsage>,
}

#[derive(Default)]
struct ChatCompletionAccumulator {
    id: Option<String>,
    created: u32,
    model: String,
    service_tier: Option<ServiceTier>,
    system_fingerprint: Option<String>,
    usage: Option<CompletionUsage>,
    choices: BTreeMap<u32, ChoiceAccumulator>,
}

impl ChatCompletionAccumulator {
    fn new(model: String) -> Self {
        Self {
            model,
            ..Self::default()
        }
    }

    fn push(&mut self, chunk: CreateShaideChatCompletionStreamResponse) {
        if self.id.is_none() {
            self.created = chunk.created;
        }
        self.id.get_or_insert(chunk.id);
        if self.service_tier.is_none() {
            self.service_tier = chunk.service_tier;
        }
        if self.system_fingerprint.is_none() {
            self.system_fingerprint = chunk.system_fingerprint;
        }
        if chunk.usage.is_some() {
            self.usage = chunk.usage;
        }
        for choice in chunk.choices {
            self.choices.entry(choice.index).or_default().push(choice);
        }
    }

    fn finish(self, received_done: bool) -> Result<CollectedChatCompletion, ShaideError> {
        if !received_done {
            return Err(ShaideError::internal_server_error(
                "Provider chat stream ended before its completion marker".to_owned(),
            ));
        }
        if self.choices.is_empty() {
            return Err(ShaideError::internal_server_error(
                "Provider returned a completed chat stream without choices".to_owned(),
            ));
        }
        if self
            .choices
            .values()
            .any(|choice| choice.finish_reason.is_none())
        {
            return Err(ShaideError::internal_server_error(
                "Provider chat stream ended before every choice had a finish reason".to_owned(),
            ));
        }
        let usage = self.usage.ok_or_else(|| {
            ShaideError::internal_server_error(
                "Provider chat stream ended without token usage".to_owned(),
            )
        })?;
        let id = self.id.ok_or_else(|| {
            ShaideError::internal_server_error("Provider returned an empty chat stream".to_owned())
        })?;
        let choices = self
            .choices
            .into_iter()
            .map(|(index, choice)| choice.finish(index))
            .collect::<Vec<_>>();
        Ok(CollectedChatCompletion {
            id,
            choices,
            created: self.created,
            model: self.model,
            service_tier: self.service_tier,
            system_fingerprint: self.system_fingerprint,
            object: "chat.completion".to_owned(),
            usage: Some(usage),
        })
    }
}

pub async fn completion_stream(
    mut request: CreateChatCompletionRequest,
    model: &ModelDAO,
) -> Result<ShaideChatCompletionResponseStream, ShaideError> {
    let max_generated_tokens = request
        .max_completion_tokens
        .map(|max_completion_token| max_completion_token.min(model.max_generated_tokens as u32))
        .unwrap_or(model.max_generated_tokens as u32);
    request.max_completion_tokens = Some(max_generated_tokens);
    match model.platform.as_deref() {
        Some("vertex") => {
            let gcp_client = get_gcp_client().await?;
            let stream_completion = gcp_client
                .stream_gcp_completion_response(request, model)
                .await?;
            Ok(stream_completion)
        }
        Some("axem") => {
            let axem_client = get_axem_client().await;
            let stream_completion = axem_client.stream_chat_completion(request, model).await?;
            Ok(stream_completion)
        }
        Some("foundry") => {
            let azure_client = get_azure_client().await?;
            let stream_completion = azure_client.stream_chat_completion(request, model).await?;
            Ok(stream_completion)
        }
        Some(platform) => Err(ShaideError::unsupported_platform(platform.to_owned())),
        None => {
            warn!(model = model.name, "Model has no configured platform");
            Err(ShaideError::unsupported_platform("none".to_owned()))
        }
    }
}

pub async fn collect_chat_completion(
    mut stream: ShaideChatCompletionResponseStream,
    model: String,
) -> Result<CollectedChatCompletion, ShaideError> {
    let mut accumulator = ChatCompletionAccumulator::new(model);
    let mut received_done = false;
    while let Some(event) = stream.next().await {
        match event {
            Ok(ShaideChatCompletionStreamEvent::Chunk(chunk)) => accumulator.push(chunk),
            Ok(ShaideChatCompletionStreamEvent::Done) => {
                received_done = true;
                break;
            }
            Err(error) => return Err(error.into()),
        }
    }
    accumulator.finish(received_done)
}
