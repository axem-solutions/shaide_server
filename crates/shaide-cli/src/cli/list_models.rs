use anyhow::Result;
use clap::Parser;
use reqwest::Method;
use shaide_common::api::models::{ListModel, ListModelsResponse, VisionLimits};

use crate::{cli::ExecuteServerCommand, shaide_client::ShaideClient};

#[derive(Parser)]
pub struct ListModelsArg;

impl ExecuteServerCommand for ListModelsArg {
    async fn execute_server_command(self, shaide_client: ShaideClient) -> Result<()> {
        let response = shaide_client
            .request("v1/models", Method::GET, None, vec![])
            .await?;
        let response_text = response.text().await?;
        let models_response: ListModelsResponse = serde_json::from_str(&response_text)?;
        for ListModel {
            id,
            name,
            variant,
            platform,
            context_size,
            supports_images,
            vision_limits,
            native_fim_mode,
            ..
        } in models_response.models
        {
            let VisionLimits {
                max_images_per_request,
                max_image_bytes,
                max_image_width_px,
                max_image_height_px,
            } = vision_limits.unwrap_or_default();
            println!(
                "Model id: {id}, name: {name}, variant: {variant}, platform: {platform:?}, \
                 context_size: {context_size:?}, supports_images: {supports_images:?}, \
                 max_images_per_request: {max_images_per_request:?}, \
                 max_image_bytes: {max_image_bytes:?}, \
                 max_image_width_px: {max_image_width_px:?}, \
                 max_image_height_px: {max_image_height_px:?}, \
                 native_fim_mode: {native_fim_mode:?}"
            )
        }
        Ok(())
    }
}
