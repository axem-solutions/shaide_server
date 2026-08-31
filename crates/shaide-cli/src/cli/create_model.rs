use anyhow::Result;
use clap::Parser;
use reqwest::Method;
use shaide_common::api::models::{
    CreateModelRequest, CreateModelResponse, NativeFimMode, validate_reasoning_effort_values,
};

use crate::{cli::ExecuteServerCommand, shaide_client::ShaideClient};

#[derive(Parser)]
pub struct CreateModelArgs {
    #[arg(long)]
    pub name: String,

    #[arg(long)]
    pub variant: String,

    #[arg(long)]
    pub chat_completions_endpoint: String,

    #[arg(long)]
    pub completions_endpoint: Option<String>,

    #[arg(long)]
    pub responses_endpoint: Option<String>,

    #[arg(long)]
    pub api_schema: String,

    #[arg(long)]
    pub native_fim_mode: Option<String>,

    #[arg(long)]
    pub fim_prompt_template: Option<String>,

    #[arg(long)]
    pub daily_input_token_limit: Option<i64>,

    #[arg(long)]
    pub daily_output_token_limit: Option<i64>,

    #[arg(long)]
    pub supports_images: Option<bool>,

    /// Values the model accepts for `reasoning_effort`, in the order clients should render them
    /// (for example `minimal,low,medium,high`). Leave it out for models that do not accept the
    /// parameter, including reasoning models driven by a thinking mode or a token budget.
    #[arg(long, value_delimiter = ',')]
    pub reasoning_effort_values: Vec<String>,

    #[arg(long)]
    pub max_images_per_request: Option<i64>,

    #[arg(long)]
    pub max_image_bytes: Option<i64>,

    #[arg(long)]
    pub max_image_width_px: Option<i64>,

    #[arg(long)]
    pub max_image_height_px: Option<i64>,

    #[arg(long)]
    pub max_generated_tokens: i64,

    #[arg(long)]
    pub context_size: i64,

    #[arg(long)]
    pub platform: Option<String>,
}

impl CreateModelArgs {
    pub fn into_api_request(self) -> CreateModelRequest {
        let CreateModelArgs {
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
        } = self;

        let native_fim_mode = native_fim_mode.and_then(|mode| match mode.to_lowercase().as_str() {
            "completions_suffix" => Some(NativeFimMode::CompletionsSuffix),
            "fim_tokens" => Some(NativeFimMode::FimTokens),
            _ => None,
        });

        CreateModelRequest {
            name,
            variant,
            chat_completions_endpoint,
            completions_endpoint,
            responses_endpoint,
            api_schema,
            daily_input_token_limit,
            daily_output_token_limit,
            supports_images: supports_images.unwrap_or(false),
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
        }
    }
}

impl ExecuteServerCommand for CreateModelArgs {
    async fn execute_server_command(self, shaide_client: ShaideClient) -> Result<()> {
        let body = self.into_api_request();
        // The server rejects these too, but its 400 body does not deserialize into a response type.
        validate_reasoning_effort_values(&body.reasoning_effort_values)?;
        let body = serde_json::to_string(&body)?;
        let response = shaide_client
            .request("v1/models", Method::POST, Some(body.into()), vec![])
            .await?;
        let response_text = response.text().await?;
        let CreateModelResponse { model_id } = serde_json::from_str(&response_text)?;
        println!("Model with id {model_id} created");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::CreateModelArgs;

    #[derive(Parser)]
    struct TestCli {
        #[command(flatten)]
        create_model: CreateModelArgs,
    }

    fn parse(extra_args: &[&str]) -> CreateModelArgs {
        let mut args = vec![
            "shaide-cli",
            "--name",
            "some-model",
            "--variant",
            "some-model",
            "--chat-completions-endpoint",
            "https://example.com/v1/chat/completions",
            "--api-schema",
            "open_ai",
            "--max-generated-tokens",
            "512",
            "--context-size",
            "32768",
        ];
        args.extend_from_slice(extra_args);
        TestCli::parse_from(args).create_model
    }

    #[test]
    fn reasoning_effort_values_are_parsed_as_a_comma_separated_list() {
        assert_eq!(
            parse(&["--reasoning-effort-values", "minimal,low,medium,high"])
                .into_api_request()
                .reasoning_effort_values,
            vec!["minimal", "low", "medium", "high"]
        );
        assert!(
            parse(&[])
                .into_api_request()
                .reasoning_effort_values
                .is_empty()
        );
    }
}
