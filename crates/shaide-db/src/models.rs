use chrono::{DateTime, Utc};
use shaide_common::api::models::{CreateModelRequest, ListModel, NativeFimMode, VisionLimits};
use sqlx::{query, query_as};
use tracing::warn;

use crate::{
    DbConn,
    error::{Resource, ShaideDBError},
};

#[derive(Debug)]
pub enum ApiSchemaDao {
    OpenAI,
    Vertex,
    Anthropic,
}

impl From<String> for ApiSchemaDao {
    fn from(value: String) -> Self {
        match value.as_str() {
            "open_ai" => Self::OpenAI,
            "vertex" => Self::Vertex,
            "anthropic" => Self::Anthropic,
            _ => unreachable!(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum NativeFimModeDao {
    CompletionsSuffix,
    FimTokens,
}

impl NativeFimModeDao {
    fn as_str(&self) -> &'static str {
        match self {
            Self::CompletionsSuffix => "completions_suffix",
            Self::FimTokens => "fim_tokens",
        }
    }
}

#[derive(Debug)]
pub struct FimModeDao(pub Option<NativeFimModeDao>);

impl From<NativeFimModeDao> for FimModeDao {
    fn from(value: NativeFimModeDao) -> Self {
        Self(Some(value))
    }
}

impl From<String> for NativeFimModeDao {
    fn from(value: String) -> Self {
        match value.as_str() {
            "completions_suffix" => Self::CompletionsSuffix,
            "fim_tokens" => Self::FimTokens,
            _ => unreachable!(),
        }
    }
}

impl From<Option<String>> for FimModeDao {
    fn from(value: Option<String>) -> Self {
        match value {
            Some(val) => Self(Some(val.into())),
            None => Self(None),
        }
    }
}

/// Stored as a JSON array of strings, in the order clients should render.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReasoningEffortValuesDao(pub Vec<String>);

impl ReasoningEffortValuesDao {
    fn to_json(&self) -> String {
        serde_json::to_string(&self.0).expect("a list of strings always serializes")
    }
}

impl From<String> for ReasoningEffortValuesDao {
    fn from(value: String) -> Self {
        match serde_json::from_str(&value) {
            Ok(values) => Self(values),
            // A hand-edited row degrades to "no effort selector" instead of failing the whole
            // listing, but it must not do so silently.
            Err(error) => {
                warn!(
                    stored_value = %value,
                    %error,
                    "Ignoring unreadable reasoning_effort_values, treating the model as not \
                     accepting reasoning_effort"
                );
                Self::default()
            }
        }
    }
}

impl From<Vec<String>> for ReasoningEffortValuesDao {
    fn from(value: Vec<String>) -> Self {
        Self(value)
    }
}

#[derive(Debug)]
pub struct ModelDAO {
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub id: i64,
    pub name: String,
    pub variant: String,
    pub chat_completions_endpoint: String,
    pub completions_endpoint: Option<String>,
    pub responses_endpoint: Option<String>,
    pub api_schema: ApiSchemaDao,
    pub daily_input_token_limit: Option<i64>,
    pub daily_output_token_limit: Option<i64>,
    pub supports_images: bool,
    pub reasoning_effort_values: ReasoningEffortValuesDao,
    pub max_images_per_request: Option<i64>,
    pub max_image_bytes: Option<i64>,
    pub max_image_width_px: Option<i64>,
    pub max_image_height_px: Option<i64>,
    pub max_generated_tokens: i64,
    pub context_size: i64,
    // NOTE: If the model is not hosted on prem, we are storing the name of the platform where the
    // model is hosted
    pub platform: Option<String>,
    pub native_fim_mode: FimModeDao,
    pub fim_prompt_template: Option<String>,
}

fn native_fim_mode_dao_to_native_fim_mode(fim_mode_dao: FimModeDao) -> Option<NativeFimMode> {
    match fim_mode_dao {
        FimModeDao(Some(x)) => match x {
            NativeFimModeDao::CompletionsSuffix => Some(NativeFimMode::CompletionsSuffix),
            NativeFimModeDao::FimTokens => Some(NativeFimMode::FimTokens),
        },
        FimModeDao(None) => None,
    }
}

impl ModelDAO {
    pub fn to_api_response(self) -> ListModel {
        let has_vision_limits = self.max_images_per_request.is_some()
            || self.max_image_bytes.is_some()
            || self.max_image_width_px.is_some()
            || self.max_image_height_px.is_some();
        ListModel {
            vision_limits: has_vision_limits.then_some(VisionLimits {
                max_images_per_request: self.max_images_per_request,
                max_image_bytes: self.max_image_bytes,
                max_image_width_px: self.max_image_width_px,
                max_image_height_px: self.max_image_height_px,
            }),
            id: self.id,
            name: self.name,
            variant: self.variant,
            platform: self.platform,
            context_size: self.context_size,
            supports_images: self.supports_images,
            reasoning_effort_values: self.reasoning_effort_values.0,
            native_fim_mode: native_fim_mode_dao_to_native_fim_mode(self.native_fim_mode),
        }
    }
}

pub struct InsertModelDAO {
    pub name: String,
    pub variant: String,
    pub chat_completions_endpoint: String,
    pub completions_endpoint: Option<String>,
    pub responses_endpoint: Option<String>,
    pub api_schema: String,
    pub daily_input_token_limit: Option<i64>,
    pub daily_output_token_limit: Option<i64>,
    pub supports_images: bool,
    pub reasoning_effort_values: ReasoningEffortValuesDao,
    pub max_images_per_request: Option<i64>,
    pub max_image_bytes: Option<i64>,
    pub max_image_width_px: Option<i64>,
    pub max_image_height_px: Option<i64>,
    pub max_generated_tokens: i64,
    pub context_size: i64,
    pub platform: Option<String>,
    pub native_fim_mode: Option<NativeFimModeDao>,
    pub fim_prompt_template: Option<String>,
}

impl InsertModelDAO {
    pub fn from_create_model_request(insert_model: CreateModelRequest) -> Self {
        Self {
            name: insert_model.name,
            variant: insert_model.variant,
            chat_completions_endpoint: insert_model.chat_completions_endpoint,
            completions_endpoint: insert_model.completions_endpoint,
            responses_endpoint: insert_model.responses_endpoint,
            api_schema: insert_model.api_schema,
            daily_input_token_limit: insert_model.daily_input_token_limit,
            daily_output_token_limit: insert_model.daily_output_token_limit,
            supports_images: insert_model.supports_images,
            reasoning_effort_values: insert_model.reasoning_effort_values.into(),
            max_images_per_request: insert_model.max_images_per_request,
            max_image_bytes: insert_model.max_image_bytes,
            max_image_width_px: insert_model.max_image_width_px,
            max_image_height_px: insert_model.max_image_height_px,
            max_generated_tokens: insert_model.max_generated_tokens,
            context_size: insert_model.context_size,
            platform: insert_model.platform,
            native_fim_mode: insert_model.native_fim_mode.map(|mode| match mode {
                NativeFimMode::CompletionsSuffix => NativeFimModeDao::CompletionsSuffix,
                NativeFimMode::FimTokens => NativeFimModeDao::FimTokens,
            }),
            fim_prompt_template: insert_model.fim_prompt_template,
        }
    }
}

pub struct SetModelLimitsDao {
    pub name: String,
    pub daily_input_token_limit: Option<i64>,
    pub daily_output_token_limit: Option<i64>,
}

impl DbConn {
    pub async fn get_model_by_name(&self, name: &str) -> Result<ModelDAO, ShaideDBError> {
        let model = query_as!(
            ModelDAO,
            r#"SELECT
                id as "id!",
                created_at as "created_at!: DateTime<Utc>",
                updated_at as "updated_at!: DateTime<Utc>",
                name as "name: String",
                variant as "variant: String",
                chat_completions_endpoint as "chat_completions_endpoint: String",
                completions_endpoint,
                responses_endpoint,
                api_schema as "api_schema: String",
                daily_input_token_limit,
                daily_output_token_limit,
                supports_images as "supports_images: bool",
                reasoning_effort_values as "reasoning_effort_values: String",
                max_images_per_request,
                max_image_bytes,
                max_image_width_px,
                max_image_height_px,
                max_generated_tokens as "max_generated_tokens: i64",
                context_size as "context_size: i64",
                platform,
                native_fim_mode,
                fim_prompt_template
            FROM models WHERE name = ?"#,
            name
        )
        .fetch_optional(&self.pool)
        .await?;
        if let Some(model) = model {
            Ok(model)
        } else {
            Err(ShaideDBError::NotFound(Resource::Model))
        }
    }

    pub async fn create_model(&self, insert_model: InsertModelDAO) -> Result<i64, ShaideDBError> {
        let InsertModelDAO {
            name,
            variant,
            chat_completions_endpoint,
            completions_endpoint,
            responses_endpoint,
            api_schema,
            daily_input_token_limit,
            daily_output_token_limit,
            supports_images,
            reasoning_effort_values,
            max_images_per_request,
            max_image_bytes,
            max_image_width_px,
            max_image_height_px,
            max_generated_tokens,
            context_size,
            platform,
            native_fim_mode,
            fim_prompt_template,
        } = insert_model;
        let native_fim_mode = native_fim_mode.map(|mode| mode.as_str());
        let reasoning_effort_values = reasoning_effort_values.to_json();
        let mut transaction = self.pool.begin().await?;

        let res = query!(
            "INSERT INTO models (
                name,
                variant,
                chat_completions_endpoint,
                completions_endpoint,
                responses_endpoint,
                api_schema,
                daily_input_token_limit,
                daily_output_token_limit,
                supports_images,
                reasoning_effort_values,
                max_images_per_request,
                max_image_bytes,
                max_image_width_px,
                max_image_height_px,
                max_generated_tokens,
                context_size,
                platform,
                native_fim_mode,
                fim_prompt_template
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            name,
            variant,
            chat_completions_endpoint,
            completions_endpoint,
            responses_endpoint,
            api_schema,
            daily_input_token_limit,
            daily_output_token_limit,
            supports_images,
            reasoning_effort_values,
            max_images_per_request,
            max_image_bytes,
            max_image_width_px,
            max_image_height_px,
            max_generated_tokens,
            context_size,
            platform,
            native_fim_mode,
            fim_prompt_template
        )
        .execute(&mut *transaction)
        .await
        .map_err(|err| match &err {
            sqlx::Error::Database(error) => {
                if error.is_unique_violation() {
                    ShaideDBError::InsertFailedOnConflict("model".to_owned())
                } else {
                    ShaideDBError::DBError(err)
                }
            }
            _ => ShaideDBError::DBError(err),
        })?;

        transaction.commit().await?;
        Ok(res.last_insert_rowid())
    }

    pub async fn delete_model_by_id(&self, id: i64) -> Result<(), ShaideDBError> {
        let mut transaction = self.pool.begin().await?;
        query!("DELETE FROM models WHERE id = ?", id)
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await?;
        Ok(())
    }

    pub async fn list_models(&self) -> Result<Vec<ModelDAO>, ShaideDBError> {
        let models = query_as!(
            ModelDAO,
            r#"SELECT
                id as "id!",
                created_at as "created_at!: DateTime<Utc>",
                updated_at as "updated_at!: DateTime<Utc>",
                name as "name: String",
                variant as "variant: String",
                chat_completions_endpoint as "chat_completions_endpoint: String",
                completions_endpoint,
                responses_endpoint,
                api_schema as "api_schema: String",
                daily_input_token_limit,
                daily_output_token_limit,
                supports_images as "supports_images: bool",
                reasoning_effort_values as "reasoning_effort_values: String",
                max_images_per_request,
                max_image_bytes,
                max_image_width_px,
                max_image_height_px,
                max_generated_tokens as "max_generated_tokens: i64",
                context_size as "context_size: i64",
                platform,
                native_fim_mode,
                fim_prompt_template
            FROM models"#
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(models)
    }

    pub async fn set_model_limits(
        &self,
        set_model_limits: SetModelLimitsDao,
    ) -> Result<(), ShaideDBError> {
        let name = &set_model_limits.name;
        match set_model_limits {
            SetModelLimitsDao {
                daily_input_token_limit: Some(daily_input_token_limit),
                daily_output_token_limit: None,
                ..
            } => {
                query!(
                    "UPDATE models SET daily_input_token_limit = ? WHERE name = ?",
                    daily_input_token_limit,
                    name
                )
                .execute(&self.pool)
                .await?;
            }
            SetModelLimitsDao {
                daily_input_token_limit: None,
                daily_output_token_limit: Some(daily_output_token_limit),
                ..
            } => {
                query!(
                    "UPDATE models SET daily_output_token_limit = ? WHERE name = ?",
                    daily_output_token_limit,
                    name
                )
                .execute(&self.pool)
                .await?;
            }
            SetModelLimitsDao {
                daily_input_token_limit: Some(daily_input_token_limit),
                daily_output_token_limit: Some(daily_output_token_limit),
                ..
            } => {
                query!(
                    "UPDATE models SET daily_input_token_limit = ?, daily_output_token_limit = ? WHERE name = ?",
                    daily_input_token_limit,
                    daily_output_token_limit,
                    name
                )
                .execute(&self.pool)
                .await?;
            }
            _ => {}
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::ReasoningEffortValuesDao;

    fn dao(values: &[&str]) -> ReasoningEffortValuesDao {
        ReasoningEffortValuesDao(values.iter().map(|value| (*value).to_owned()).collect())
    }

    #[test]
    fn values_survive_a_json_round_trip() {
        for case in [
            vec![],
            vec!["low", "medium", "high"],
            vec!["high", "minimal", "low"],
        ] {
            let encoded = dao(&case).to_json();
            assert_eq!(
                ReasoningEffortValuesDao::from(encoded.clone()),
                dao(&case),
                "{case:?} should survive {encoded}"
            );
        }
    }

    #[test]
    fn an_empty_list_is_stored_as_an_empty_json_array() {
        assert_eq!(dao(&[]).to_json(), "[]");
    }

    #[test]
    fn the_stored_order_is_the_configured_order() {
        assert_eq!(
            dao(&["high", "minimal", "low"]).to_json(),
            r#"["high","minimal","low"]"#
        );
    }

    /// A row that is not a JSON array of strings degrades to "no effort selector" rather than
    /// taking the whole model listing down.
    #[test]
    fn unreadable_rows_degrade_to_no_values() {
        for stored in ["", "not json", "{}", "[1, 2]", "[\"low\", 2]", "null"] {
            assert_eq!(
                ReasoningEffortValuesDao::from(stored.to_owned()),
                ReasoningEffortValuesDao::default(),
                "{stored:?} should degrade to no values"
            );
        }
    }

    /// Degrading is only acceptable because it leaves a trace: without the warning an unreadable
    /// row would silently drop a model's effort selector.
    #[test]
    fn unreadable_rows_are_logged() {
        let logs = CapturedLogs::default();
        let subscriber = tracing_subscriber::fmt()
            .with_writer(logs.clone())
            .without_time()
            .finish();

        tracing::subscriber::with_default(subscriber, || {
            let _ = ReasoningEffortValuesDao::from("not json".to_owned());
            let _ = ReasoningEffortValuesDao::from(r#"["low"]"#.to_owned());
        });

        let captured = logs.contents();
        assert!(
            captured.contains("Ignoring unreadable reasoning_effort_values"),
            "the unreadable row should be logged, got: {captured:?}"
        );
        assert!(
            captured.contains("not json"),
            "the log should name the offending value, got: {captured:?}"
        );
        assert_eq!(
            captured.matches("Ignoring unreadable").count(),
            1,
            "a readable row must not log, got: {captured:?}"
        );
    }

    #[derive(Clone, Default)]
    struct CapturedLogs(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

    impl CapturedLogs {
        fn contents(&self) -> String {
            String::from_utf8(self.0.lock().unwrap().clone()).expect("logs are utf-8")
        }
    }

    impl std::io::Write for CapturedLogs {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for CapturedLogs {
        type Writer = Self;

        fn make_writer(&'a self) -> Self::Writer {
            self.clone()
        }
    }
}
