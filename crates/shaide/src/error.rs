use async_openai::error::OpenAIError;
use axum::{Json, extract::rejection::QueryRejection, response::IntoResponse};
use color_eyre::eyre::Report;
use hyper::StatusCode;
use shaide_common::api::error::{OpenAiErrorBody, OpenAiErrorResponse};
use shaide_db::error::{Resource, ShaideDBError};
use thiserror::Error;
use tracing::error;

use crate::providers::{
    azure::AzureError, gcp::GcpError, shaide::ShaideProviderError, vector_db::RagCollectionError,
};

#[derive(Debug, Error)]
pub enum ShaideError {
    #[error(transparent)]
    DB(ShaideDBError),

    #[error("Unsupported provider selected: {0}")]
    UnsupportedPlatform(String),

    #[error("Internal server error: {0}")]
    InternalServerError(String),

    #[error("Route not found: {0}")]
    NotFoundRoute(String),

    #[error("Unauthorized")]
    Unauthorized(String),

    #[error("Model usage limit reached")]
    ModelUsageLimitReached(String),

    #[error("Bad request")]
    BadRequest(String),

    #[error("Bad request: {0}")]
    BadRequestWithMessage(String),

    #[error("Request rejected: {message}")]
    RequestRejection {
        status_code: StatusCode,
        message: String,
        code: String,
    },

    #[error("Rag error")]
    RagError(RagCollectionError),

    #[error(transparent)]
    GcpError(GcpError),

    #[error(transparent)]
    AzureError(AzureError),

    #[error(transparent)]
    ShaideProviderError(ShaideProviderError),

    #[error("OpenAIError")]
    OpenAIError(OpenAIError),

    #[error("HTTP request failed")]
    ReqwestError(reqwest::Error),
}

impl ShaideError {
    #[track_caller]
    fn emit_report(error: &Self) {
        #[derive(Debug, Error)]
        #[error("{message}")]
        struct ShaideReportError {
            message: String,
        }

        let report = Report::new(ShaideReportError {
            message: error.to_string(),
        });
        println!("An error has occurred: {report:?}");
    }

    #[track_caller]
    pub fn db(value: ShaideDBError) -> Self {
        Self::reported(Self::DB(value))
    }

    #[track_caller]
    pub fn unsupported_platform(platform: String) -> Self {
        Self::reported(Self::UnsupportedPlatform(platform))
    }

    #[track_caller]
    pub fn internal_server_error(error: String) -> Self {
        Self::reported(Self::InternalServerError(error))
    }

    #[track_caller]
    pub fn not_found_route(route: String) -> Self {
        Self::reported(Self::NotFoundRoute(route))
    }

    #[track_caller]
    pub fn unauthorized(message: String) -> Self {
        Self::reported(Self::Unauthorized(message))
    }

    #[track_caller]
    pub fn model_usage_limit_reached(message: String) -> Self {
        Self::reported(Self::ModelUsageLimitReached(message))
    }

    #[track_caller]
    pub fn bad_request(reason: String) -> Self {
        Self::reported(Self::BadRequest(reason))
    }

    #[track_caller]
    pub fn bad_request_with_message(message: String) -> Self {
        Self::reported(Self::BadRequestWithMessage(message))
    }

    #[track_caller]
    pub fn request_rejection(
        status_code: StatusCode,
        message: String,
        code: impl Into<String>,
    ) -> Self {
        Self::reported(Self::RequestRejection {
            status_code,
            message,
            code: code.into(),
        })
    }

    #[track_caller]
    pub fn rag_error(value: RagCollectionError) -> Self {
        Self::reported(Self::RagError(value))
    }

    #[track_caller]
    pub fn reqwest(error: reqwest::Error) -> Self {
        Self::reported(Self::ReqwestError(error))
    }

    #[track_caller]
    fn reported(error: Self) -> Self {
        Self::emit_report(&error);
        error
    }

    pub(crate) fn openai_error_type(status: StatusCode) -> &'static str {
        match status {
            StatusCode::BAD_REQUEST => "invalid_request_error",
            StatusCode::UNAUTHORIZED => "authentication_error",
            StatusCode::FORBIDDEN => "permission_error",
            StatusCode::NOT_FOUND => "not_found_error",
            StatusCode::TOO_MANY_REQUESTS => "rate_limit_error",
            StatusCode::CONFLICT | StatusCode::LOCKED => "conflict_error",
            status if status.is_server_error() => "api_error",
            _ => "invalid_request_error",
        }
    }

    fn classify(self) -> ClassifiedError {
        match self {
            Self::DB(ShaideDBError::DBError(value))
                if matches!(value, sqlx::Error::RowNotFound) =>
            {
                error!(error = %value, "Database row not found");
                ClassifiedError::new(
                    StatusCode::NOT_FOUND,
                    "A database row could not be found",
                    None,
                    "resource_not_found",
                )
            }
            Self::DB(ShaideDBError::DBError(value)) => {
                error!(error = %value, "Database exception");
                ClassifiedError::internal()
            }
            Self::DB(ShaideDBError::InsertFailedOnConflict(value)) => ClassifiedError::new(
                StatusCode::CONFLICT,
                format!("Conflict when inserting instance type: {value}"),
                None,
                "resource_conflict",
            ),
            Self::DB(ShaideDBError::NotFound(value)) => ClassifiedError::new(
                StatusCode::NOT_FOUND,
                format!("Could not find the required {value}"),
                None,
                "resource_not_found",
            ),
            Self::UnsupportedPlatform(platform) => ClassifiedError::new(
                StatusCode::BAD_REQUEST,
                format!("Requested platform '{platform}' is not supported"),
                None,
                "unsupported_platform",
            ),
            Self::InternalServerError(reason) => {
                error!(error = %reason, "Unhandled internal server error");
                ClassifiedError::internal()
            }
            Self::NotFoundRoute(route) => ClassifiedError::new(
                StatusCode::NOT_FOUND,
                format!("Could not find the route {route}"),
                None,
                "route_not_found",
            ),
            Self::Unauthorized(message) => ClassifiedError::new(
                StatusCode::UNAUTHORIZED,
                message,
                None,
                "authentication_failed",
            ),
            Self::ModelUsageLimitReached(message) => ClassifiedError::new(
                StatusCode::FORBIDDEN,
                message,
                Some("model"),
                "model_usage_limit_reached",
            ),
            Self::BadRequest(message) | Self::BadRequestWithMessage(message) => {
                ClassifiedError::new(StatusCode::BAD_REQUEST, message, None, "invalid_request")
            }
            Self::RequestRejection {
                status_code,
                message,
                code,
            } => ClassifiedError::new_owned_code(status_code, message, None, code),
            Self::RagError(RagCollectionError::CollectionNotFound) => ClassifiedError::new(
                StatusCode::NOT_FOUND,
                "Collection not found",
                None,
                "collection_not_found",
            ),
            Self::RagError(RagCollectionError::ServiceUnavailble) => ClassifiedError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "Vector service unavailable",
                None,
                "service_unavailable",
            ),
            Self::RagError(RagCollectionError::CollectionAlreadyExists) => ClassifiedError::new(
                StatusCode::CONFLICT,
                "Collection already exists",
                None,
                "collection_already_exists",
            ),
            Self::RagError(RagCollectionError::Other(reason)) => {
                error!(reason = %reason, "RAG pipeline failed");
                ClassifiedError::internal()
            }
            Self::GcpError(error) => classify_gcp_error(error),
            Self::AzureError(error) => classify_azure_error(error),
            Self::ShaideProviderError(error) => classify_shaide_provider_error(error),
            Self::OpenAIError(OpenAIError::ApiError(error)) => ClassifiedError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                body: OpenAiErrorResponse::from_reason(OpenAiErrorBody {
                    message: error.api_error.message,
                    r#type: error.api_error.r#type.unwrap_or_else(|| {
                        Self::openai_error_type(StatusCode::INTERNAL_SERVER_ERROR).to_owned()
                    }),
                    param: error.api_error.param,
                    code: error.api_error.code,
                }),
            },
            Self::OpenAIError(error) => {
                error!(error = %error, "OpenAI provider operation failed");
                ClassifiedError::internal()
            }
            Self::ReqwestError(error) => {
                error!(error = %error, "HTTP request failed");
                ClassifiedError::internal()
            }
        }
    }
}

impl From<ShaideDBError> for ShaideError {
    #[track_caller]
    fn from(value: ShaideDBError) -> Self {
        match value {
            ShaideDBError::NotFound(Resource::User) => {
                Self::unauthorized("Invalid credentials".to_owned())
            }
            value => Self::db(value),
        }
    }
}

impl From<RagCollectionError> for ShaideError {
    #[track_caller]
    fn from(value: RagCollectionError) -> Self {
        Self::rag_error(value)
    }
}

impl From<GcpError> for ShaideError {
    #[track_caller]
    fn from(value: GcpError) -> Self {
        let error = Self::GcpError(value);
        if !matches!(&error, Self::GcpError(GcpError::UnexpectedResponse { .. })) {
            Self::emit_report(&error);
        }
        error
    }
}

impl From<AzureError> for ShaideError {
    #[track_caller]
    fn from(value: AzureError) -> Self {
        let error = Self::AzureError(value);
        if !matches!(&error, Self::AzureError(AzureError::HttpError { .. })) {
            Self::emit_report(&error);
        }
        error
    }
}

impl From<ShaideProviderError> for ShaideError {
    #[track_caller]
    fn from(value: ShaideProviderError) -> Self {
        Self::reported(Self::ShaideProviderError(value))
    }
}

impl From<OpenAIError> for ShaideError {
    #[track_caller]
    fn from(value: OpenAIError) -> Self {
        Self::reported(Self::OpenAIError(value))
    }
}

impl From<reqwest::Error> for ShaideError {
    #[track_caller]
    fn from(value: reqwest::Error) -> Self {
        Self::reqwest(value)
    }
}

impl From<QueryRejection> for ShaideError {
    #[track_caller]
    fn from(rejection: QueryRejection) -> Self {
        Self::request_rejection(
            rejection.status(),
            rejection.body_text(),
            "invalid_query_parameters",
        )
    }
}

impl IntoResponse for ShaideError {
    fn into_response(self) -> axum::response::Response {
        let ClassifiedError { status, body } = self.classify();
        (status, Json(body)).into_response()
    }
}

struct ClassifiedError {
    status: StatusCode,
    body: OpenAiErrorResponse,
}

impl ClassifiedError {
    fn new(
        status: StatusCode,
        message: impl Into<String>,
        param: Option<&'static str>,
        code: &'static str,
    ) -> Self {
        Self::new_owned_code(status, message, param, code.to_owned())
    }

    fn new_owned_code(
        status: StatusCode,
        message: impl Into<String>,
        param: Option<&'static str>,
        code: String,
    ) -> Self {
        let error_type = ShaideError::openai_error_type(status);
        let error = OpenAiErrorBody::new(message, error_type, param.map(str::to_owned), Some(code));
        let body = if status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error() {
            OpenAiErrorResponse::from_reason(error)
        } else {
            OpenAiErrorResponse::new(error)
        };
        Self { status, body }
    }

    fn internal() -> Self {
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Internal server error",
            None,
            "internal_error",
        )
    }
}

fn classify_upstream_error(
    status: StatusCode,
    response_body: String,
    service: &str,
) -> ClassifiedError {
    if let Ok(body) = serde_json::from_str::<OpenAiErrorResponse>(&response_body) {
        let body = if status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error() {
            body.ensure_legacy_reason()
        } else {
            body.ensure_legacy_message()
        };
        return ClassifiedError { status, body };
    }

    match status {
        StatusCode::UNAUTHORIZED => ClassifiedError::new(
            status,
            format!("{service} unauthorized"),
            None,
            "provider_authentication_error",
        ),
        StatusCode::TOO_MANY_REQUESTS => ClassifiedError::new(
            status,
            format!("{service} rate limited"),
            None,
            "provider_rate_limit_exceeded",
        ),
        status if status.is_server_error() => ClassifiedError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            format!("{service} unavailable"),
            None,
            "provider_unavailable",
        ),
        _ => ClassifiedError::new(
            StatusCode::BAD_REQUEST,
            format!("{service} rejected the request"),
            None,
            "provider_request_rejected",
        ),
    }
}

fn classify_gcp_error(error: GcpError) -> ClassifiedError {
    match error {
        GcpError::Build(reason) => {
            error!(reason = %reason, "Failed to initialize GCP client");
            ClassifiedError::internal()
        }
        GcpError::Credentials(reason) => {
            error!(reason = %reason, "Failed to refresh GCP access token");
            ClassifiedError::new(
                StatusCode::UNAUTHORIZED,
                "GCP provider authentication failed",
                None,
                "provider_authentication_error",
            )
        }
        GcpError::DeserializationError(reason) => {
            error!(reason = %reason, "Failed to deserialize GCP response");
            ClassifiedError::internal()
        }
        GcpError::BadRequest(message) => {
            ClassifiedError::new(StatusCode::BAD_REQUEST, message, None, "invalid_request")
        }
        GcpError::UnexpectedResponse {
            status_code,
            response_body,
            service,
        } => {
            error!(service, status_code = %status_code, response_body, "Upstream service returned error response");
            classify_upstream_error(status_code, response_body, &service)
        }
        GcpError::Request(reason) => {
            error!(reason = %reason, "GCP request failed");
            ClassifiedError::internal()
        }
        GcpError::MaxRetriesHaveBeenAchieved { service } => ClassifiedError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Maximum retries reached for {service}"),
            None,
            "provider_retries_exhausted",
        ),
    }
}

fn classify_azure_error(error: AzureError) -> ClassifiedError {
    match error {
        AzureError::Build(reason) => {
            error!(reason = %reason, "Failed to initialize Azure client");
            ClassifiedError::internal()
        }
        AzureError::Credentials(reason) => {
            error!(reason = %reason, "Failed to refresh Azure access token");
            ClassifiedError::new(
                StatusCode::UNAUTHORIZED,
                "Azure provider authentication failed",
                None,
                "provider_authentication_error",
            )
        }
        AzureError::HttpError {
            status_code,
            response_body,
        } => {
            error!(status_code = %status_code, response_body, "Azure provider returned error response");
            classify_upstream_error(status_code, response_body, "Azure provider")
        }
        AzureError::Request(reason) => {
            error!(reason = %reason, "Azure provider request failed");
            ClassifiedError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "Azure provider unavailable",
                None,
                "provider_unavailable",
            )
        }
        AzureError::Deserialization(reason) => {
            error!(reason = %reason, "Failed to deserialize Azure provider response");
            ClassifiedError::internal()
        }
    }
}

fn classify_shaide_provider_error(error: ShaideProviderError) -> ClassifiedError {
    match error {
        ShaideProviderError::DeserializationError(reason) => {
            error!(reason = %reason, "Failed to deserialize Shaide provider response");
            ClassifiedError::internal()
        }
        ShaideProviderError::Request(reason) => {
            error!(reason = %reason, "Shaide provider request failed");
            ClassifiedError::internal()
        }
        ShaideProviderError::HttpError {
            status_code,
            response_body,
        } => {
            error!(status_code = %status_code, response_body, "Shaide provider returned error response");
            classify_upstream_error(status_code, response_body, "Shaide provider")
        }
    }
}
