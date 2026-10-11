use std::collections::BTreeSet;

use serde_json::Value;
use shaide_common::api::{
    embedding_models::InsertEmbeddingModelRequest,
    models::CreateModelRequest,
    platform_models::{PlatformModel, PlatformModelsResponse},
};

use super::{DiscoveryResult, Target};

fn positive(value: Option<&Value>) -> Option<i64> {
    value.and_then(Value::as_i64).filter(|n| *n > 0)
}

fn names(value: &Value) -> Vec<String> {
    match value {
        Value::String(name) if !name.trim().is_empty() => vec![name.clone()],
        Value::Array(values) => values
            .iter()
            .filter_map(|v| v.as_str())
            .filter(|s| !s.trim().is_empty())
            .map(str::to_owned)
            .collect(),
        _ => vec![],
    }
}

/// Accept JSON objects or JSON-encoded objects; never parse Python repr/config text.
fn object(value: Option<&Value>) -> Option<Value> {
    match value? {
        value @ Value::Object(_) => Some(value.clone()),
        Value::String(value) => serde_json::from_str::<Value>(value)
            .ok()
            .filter(Value::is_object),
        _ => None,
    }
}

pub(super) fn normalize(
    target: &Target,
    info: Option<&Value>,
    models: Option<&Value>,
) -> DiscoveryResult {
    let config = object(info.and_then(|info| info.get("vllm_config")))
        .ok_or("Runtime metadata is unavailable. vLLM must expose GET /server_info?config_format=json; discovery does not enable development mode or run inference probes.")?;
    let model = object(config.get("model_config"))
        .ok_or("vLLM did not return a structured model_config.")?;
    let mut served_names: BTreeSet<String> = model
        .get("served_model_name")
        .map(names)
        .unwrap_or_default()
        .into_iter()
        .collect();
    // /v1/models is the authoritative served-name list, including aliases. Ignore LoRA adapters
    // because they can have different capabilities than the base model described by server_info.
    if let Some(cards) = models
        .and_then(|value| value.get("data"))
        .and_then(Value::as_array)
    {
        let base_names: BTreeSet<_> = cards
            .iter()
            .filter(|card| card.get("parent").is_none_or(Value::is_null))
            .filter_map(|card| card.get("id").and_then(Value::as_str))
            .filter(|name| !name.trim().is_empty())
            .map(str::to_owned)
            .collect();
        if !base_names.is_empty() {
            served_names = base_names;
        }
    }
    if served_names.is_empty() {
        served_names.extend(model.get("model").map(names).unwrap_or_default());
    }
    if served_names.is_empty() {
        return Err("vLLM did not report a served model name.");
    }
    let mut result = PlatformModelsResponse::default();
    match target.category.as_str() {
        "generative" => {
            let context_size = positive(model.get("max_model_len"))
                .ok_or("vLLM did not report a valid runtime context size.")?;
            // max_new_tokens in vLLM's override is a hard output cap, unlike a sampling default.
            // In its absence, use the context window as shaide's upper bound (input still consumes it).
            let max_generated_tokens =
                positive(model.pointer("/override_generation_config/max_new_tokens"))
                    .unwrap_or(context_size)
                    .min(context_size);
            let multimodal = model.get("multimodal_config").filter(|v| v.is_object());
            let image_limit = multimodal
                .and_then(|mm| mm.pointer("/limit_per_prompt/image"))
                .or_else(|| multimodal.and_then(|mm| mm.pointer("/limit_mm_per_prompt/image")));
            let image_count = image_limit.and_then(Value::as_i64).or_else(|| {
                image_limit
                    .and_then(|v| v.get("count"))
                    .and_then(Value::as_i64)
            });
            let hf_config = object(model.get("hf_config"));
            let has_image_config = image_limit.is_some()
                || hf_config
                    .as_ref()
                    .and_then(|hf| hf.get("vision_config"))
                    .is_some_and(Value::is_object);
            let supports_images = has_image_config
                && !multimodal
                    .and_then(|mm| mm.get("language_model_only"))
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                && image_count != Some(0);
            for name in served_names {
                result
                    .models
                    .push(PlatformModel::Generative(Box::new(CreateModelRequest {
                        variant: name.clone(),
                        name,
                        chat_completions_endpoint: format!("{}/v1/chat/completions", target.base),
                        completions_endpoint: Some(format!("{}/v1/completions", target.base)),
                        // Responses/FIM/reasoning support cannot be inferred from a model name or architecture.
                        responses_endpoint: None,
                        api_schema: "open_ai".into(),
                        platform: Some("axem".into()),
                        max_generated_tokens,
                        context_size,
                        supports_images,
                        max_images_per_request: if supports_images {
                            image_count.filter(|count| *count > 0)
                        } else {
                            None
                        },
                        reasoning_effort_values: vec![],
                        max_image_bytes: None,
                        max_image_width_px: None,
                        max_image_height_px: None,
                        daily_input_token_limit: None,
                        daily_output_token_limit: None,
                        native_fim_mode: None,
                        fim_prompt_template: None,
                    })));
            }
        }
        "embedder" => {
            // Do not assume hidden_size is the output dimension: pooling/projection can change it.
            let vector_size = positive(model.pointer("/pooler_config/dimensions"))
                .ok_or("vLLM did not report embedding output dimensions (model_config.pooler_config.dimensions). No inference probe was run; expose the output dimensions in runtime metadata before retrying.")?;
            for name in served_names {
                result
                    .models
                    .push(PlatformModel::Embedding(InsertEmbeddingModelRequest {
                        name,
                        url: format!("{}/v1/embeddings", target.base),
                        vector_size,
                        platform: Some("axem".into()),
                        api_schema: Some("open_ai".into()),
                        daily_input_token_limit: None,
                    }));
            }
        }
        _ => return Err("Unsupported model category."),
    }
    Ok(result)
}
