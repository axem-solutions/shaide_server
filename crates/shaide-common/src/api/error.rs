use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// OpenAI-compatible error response envelope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct OpenAiErrorResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
    pub error: OpenAiErrorBody,
}

impl OpenAiErrorResponse {
    pub fn new(error: OpenAiErrorBody) -> Self {
        Self {
            message: Some(error.message.clone()),
            reason: None,
            error,
        }
    }

    pub fn from_reason(error: OpenAiErrorBody) -> Self {
        Self {
            message: None,
            reason: Some(error.message.clone()),
            error,
        }
    }

    pub fn ensure_legacy_message(mut self) -> Self {
        if self.message.is_none() && self.reason.is_none() {
            self.message = Some(self.error.message.clone());
        }
        self
    }

    pub fn ensure_legacy_reason(mut self) -> Self {
        if self.message.is_none() && self.reason.is_none() {
            self.reason = Some(self.error.message.clone());
        }
        self
    }
}

/// Error details returned inside the top-level `error` property.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct OpenAiErrorBody {
    pub message: String,
    pub r#type: String,
    pub param: Option<String>,
    pub code: Option<String>,
}

impl OpenAiErrorBody {
    pub fn new(
        message: impl Into<String>,
        error_type: impl Into<String>,
        param: Option<String>,
        code: Option<String>,
    ) -> Self {
        Self {
            message: message.into(),
            r#type: error_type.into(),
            param,
            code,
        }
    }
}
