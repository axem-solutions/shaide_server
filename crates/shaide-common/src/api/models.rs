use std::fmt;

use async_openai::types::chat::ReasoningEffort;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;

#[derive(Serialize, Deserialize, ToSchema, Clone, Debug)]
#[serde(rename_all = "snake_case")]
pub enum NativeFimMode {
    CompletionsSuffix,
    FimTokens,
}

/// Only feeds the error message. The validation itself defers to [`ReasoningEffort`], so a value
/// added by a newer `async-openai` is accepted as soon as the dependency is bumped.
const ACCEPTED_REASONING_EFFORT_VALUES: &str = "none, minimal, low, medium, high, xhigh";

#[derive(Debug, PartialEq, Eq)]
pub enum ReasoningEffortValuesError {
    Empty { index: usize },
    Unsupported { index: usize, value: String },
    Duplicate { index: usize, value: String },
}

impl fmt::Display for ReasoningEffortValuesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty { index } => write!(
                f,
                "reasoning_effort_values[{index}] is empty, every value must be a non-empty string"
            ),
            Self::Unsupported { index, value } => write!(
                f,
                "reasoning_effort_values[{index}] is not a value chat completions can forward: \
                 {value:?}, expected one of {ACCEPTED_REASONING_EFFORT_VALUES}"
            ),
            Self::Duplicate { index, value } => write!(
                f,
                "reasoning_effort_values[{index}] is a duplicate of an earlier value: {value:?}"
            ),
        }
    }
}

impl std::error::Error for ReasoningEffortValuesError {}

/// An empty list is valid: the model does not accept `reasoning_effort` at all.
///
/// Every other value has to deserialize into the same [`ReasoningEffort`] that
/// `/v1/chat/completions` parses the request parameter into, so the catalogue can never advertise a
/// value the chat endpoint would reject.
pub fn validate_reasoning_effort_values(
    values: &[String],
) -> Result<(), ReasoningEffortValuesError> {
    for (index, value) in values.iter().enumerate() {
        if value.trim().is_empty() {
            return Err(ReasoningEffortValuesError::Empty { index });
        }
        if serde_json::from_value::<ReasoningEffort>(Value::String(value.clone())).is_err() {
            return Err(ReasoningEffortValuesError::Unsupported {
                index,
                value: value.clone(),
            });
        }
        if values[..index].contains(value) {
            return Err(ReasoningEffortValuesError::Duplicate {
                index,
                value: value.clone(),
            });
        }
    }
    Ok(())
}

#[derive(Serialize, Deserialize, ToSchema, Clone, Debug, Default)]
pub struct VisionLimits {
    pub max_images_per_request: Option<i64>,
    pub max_image_bytes: Option<i64>,
    pub max_image_width_px: Option<i64>,
    pub max_image_height_px: Option<i64>,
}

#[derive(Serialize, Deserialize, ToSchema, Clone, Debug, Default)]
pub struct ListModel {
    pub id: i64,
    pub name: String,
    pub variant: String,
    pub platform: Option<String>,
    pub context_size: i64,
    pub supports_images: bool,
    pub reasoning_effort_values: Vec<String>,
    pub vision_limits: Option<VisionLimits>,
    pub native_fim_mode: Option<NativeFimMode>,
}

#[derive(Serialize, Deserialize, ToSchema, Clone, Debug, Default)]
pub struct OpenAIListModel {
    pub id: String,
    pub object: String,
    pub created: u32,
    pub owned_by: String,
}

#[derive(Serialize, Deserialize, ToSchema, Clone, Debug, Default)]
pub struct ListModelsResponse {
    pub models: Vec<ListModel>,
    pub object: String,
    pub data: Vec<OpenAIListModel>,
}

#[derive(Serialize, Deserialize, ToSchema)]
pub struct CreateModelResponse {
    pub model_id: i64,
}

#[derive(Serialize, Deserialize, ToSchema)]
pub struct CreateModelRequest {
    pub name: String,
    pub variant: String,
    #[serde(alias = "url")]
    pub chat_completions_endpoint: String,
    pub completions_endpoint: Option<String>,
    pub responses_endpoint: Option<String>,
    pub api_schema: String,
    pub daily_input_token_limit: Option<i64>,
    pub daily_output_token_limit: Option<i64>,
    #[serde(default)]
    pub supports_images: bool,
    #[serde(default)]
    pub reasoning_effort_values: Vec<String>,
    pub max_images_per_request: Option<i64>,
    pub max_image_bytes: Option<i64>,
    pub max_image_width_px: Option<i64>,
    pub max_image_height_px: Option<i64>,
    pub max_generated_tokens: i64,
    pub context_size: i64,
    pub platform: Option<String>,
    pub native_fim_mode: Option<NativeFimMode>,
    pub fim_prompt_template: Option<String>,
}

#[derive(Serialize, Deserialize, ToSchema)]
pub struct DeleteModelRequest {
    pub model_id: i64,
}

#[derive(Serialize, Deserialize, ToSchema)]
pub struct SetModelLimitsRequest {
    pub name: String,
    pub daily_input_token_limit: Option<i64>,
    pub daily_output_token_limit: Option<i64>,
}

#[cfg(test)]
mod tests {
    use async_openai::types::chat::CreateChatCompletionRequest;

    use super::{CreateModelRequest, ReasoningEffortValuesError, validate_reasoning_effort_values};

    fn values(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn valid_lists_are_accepted() {
        for case in [
            vec![],
            values(&["low", "medium", "high"]),
            values(&["none", "minimal", "low", "medium", "high", "xhigh"]),
        ] {
            assert_eq!(
                validate_reasoning_effort_values(&case),
                Ok(()),
                "{case:?} should be accepted"
            );
        }
    }

    /// The catalogue must never advertise a value `/v1/chat/completions` would reject: an accepted
    /// value has to survive deserialization into the request type the chat route parses.
    #[test]
    fn accepted_values_are_forwardable_by_chat_completions() {
        for value in ["none", "minimal", "low", "medium", "high", "xhigh"] {
            validate_reasoning_effort_values(&values(&[value]))
                .unwrap_or_else(|err| panic!("catalogue should accept {value:?}: {err}"));

            let request: CreateChatCompletionRequest = serde_json::from_str(&format!(
                r#"{{"model": "some-model", "messages": [], "reasoning_effort": "{value}"}}"#
            ))
            .unwrap_or_else(|err| panic!("chat completions should accept {value:?}: {err}"));
            assert!(request.reasoning_effort.is_some());
        }
    }

    #[test]
    fn values_chat_completions_cannot_forward_are_rejected() {
        for (index, value) in [" low", "LOW", "ultra", "thinking"].into_iter().enumerate() {
            assert_eq!(
                validate_reasoning_effort_values(&values(&[value])),
                Err(ReasoningEffortValuesError::Unsupported {
                    index: 0,
                    value: value.to_owned()
                }),
                "case {index}: {value:?} should be rejected"
            );
        }
    }

    #[test]
    fn empty_string_value_is_rejected() {
        assert_eq!(
            validate_reasoning_effort_values(&values(&["low", "", "high"])),
            Err(ReasoningEffortValuesError::Empty { index: 1 })
        );
        assert_eq!(
            validate_reasoning_effort_values(&values(&["   "])),
            Err(ReasoningEffortValuesError::Empty { index: 0 })
        );
    }

    #[test]
    fn the_error_message_names_the_offending_value_and_the_accepted_ones() {
        let error = validate_reasoning_effort_values(&values(&["low", "ultra"]))
            .expect_err("unknown value should be rejected");

        assert_eq!(
            error.to_string(),
            "reasoning_effort_values[1] is not a value chat completions can forward: \"ultra\", \
             expected one of none, minimal, low, medium, high, xhigh"
        );
    }

    #[test]
    fn duplicate_value_is_rejected() {
        assert_eq!(
            validate_reasoning_effort_values(&values(&["low", "high", "low"])),
            Err(ReasoningEffortValuesError::Duplicate {
                index: 2,
                value: "low".to_owned()
            })
        );
    }

    #[test]
    fn create_model_request_defaults_to_no_reasoning_effort_values() {
        let request: CreateModelRequest = serde_json::from_str(
            r#"{
                "name": "some-model",
                "variant": "some-model",
                "chat_completions_endpoint": "https://example.com/v1/chat/completions",
                "completions_endpoint": null,
                "responses_endpoint": null,
                "api_schema": "open_ai",
                "daily_input_token_limit": null,
                "daily_output_token_limit": null,
                "max_generated_tokens": 512,
                "context_size": 32768,
                "platform": null,
                "native_fim_mode": null,
                "fim_prompt_template": null
            }"#,
        )
        .expect("request should deserialize");
        assert!(request.reasoning_effort_values.is_empty());
    }
}
