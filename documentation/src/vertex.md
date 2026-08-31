# Vertex

This document captures how we actually integrated managed models on Google Cloud. 

# Model garden overview
Google Cloud exposes multiple surfaces for third-party and first-party models. In practice, the URL and the request/streaming schema vary by provider and by model, even when the marketing suggests a single interface. We also exclusively use third-party solutions for now; the docs are written with that in mind.

## stream-raw-predict for Anthropic models

Docs can be found [here](https://cloud.google.com/sdk/gcloud/reference/ai/endpoints/stream-raw-predict).
This kind of API provides access to the Anthropic models. Example:

```sh
#!/bin/sh

export ANTHROPIC_API_KEY=$(gcloud auth print-access-token --impersonate-service-account=vertex-sa@your-gcp-project-id.iam.gserviceaccount.com)
export ANTHROPIC_API_ENDPOINT="https://aiplatform.googleapis.com/v1/projects/your-gcp-project-id/locations/global/publishers/anthropic/models/claude-opus-4-1:streamRawPredict"

curl -sS -X POST \
  -H "Authorization: Bearer $ANTHROPIC_API_KEY" \
  -H "Content-Type: application/json; charset=utf-8" \
  -d '{
        "anthropic_version":"vertex-2023-10-16",
        "messages":[{"role":"user","content":[{"type":"text","text":"Smoke test"}]}],
        "max_tokens":64,
        "stream": true
      }' \
  "$ANTHROPIC_API_ENDPOINT"
```

## Other models

Other models (OpenAI compatible models) are exposed through the `chat/completions` endpoint. Example:

```sh
#!/bin/sh

export OPENAI_API_KEY=$(gcloud auth print-access-token --impersonate-service-account=vertex-sa@your-gcp-project-id.iam.gserviceaccount.com)
export OPENAI_API_ENDPOINT="https://aiplatform.googleapis.com/v1/projects/your-gcp-project-id/locations/global/endpoints/openapi/chat/completions"

curl -sS -X POST \
  -H "Authorization: Bearer $OPENAI_API_KEY" \
  -H "Content-Type: application/json; charset=utf-8" \
  -d '{
        "messages":[{"role":"user","content":[{"type":"text","text":"Smoke test"}]}],
        "max_tokens":64,
        "stream": true
      }' \
  "$OPENAI_API_ENDPOINT"
```

# Regional Availability & Endpoints

Google Cloud's Vertex AI deploys models across different regions based on availability and capacity.

## Latency Considerations

Your physical location relative to the model's region will impact response times:
- **Global endpoints** are routed to the nearest available region
- **Regional endpoints** may add 50-200ms latency if you're far from the region

## Endpoint Patterns

All endpoints follow this pattern:
```
https://{region}-aiplatform.googleapis.com/v1/projects/{project}/locations/{location}/...
```

Where region can be:
- Omitted (global): `https://aiplatform.googleapis.com/...`
- Specific: `https://us-west2-aiplatform.googleapis.com/...`

# Model Comparison

Quick reference for all supported models:

| Model | Provider | Context Size | Max Output | Daily Input Limit | Daily Output Limit | Region | API Schema |
|-------|----------|--------------|------------|-------------------|-------------------|---------|------------|
| Claude Opus 4-1 | Anthropic | 200K | 32K | 1M | 32M | Global | anthropic |
| Claude Sonnet 4-5 | Anthropic | 200K | 32K | 1M | 32M | Global | anthropic |
| GPT OSS 120B | OpenAI-compatible | 128K | 32K | 1M | 32M | Global | open_ai |
| Deepseek V3.1 | Deepseek AI | 128K | 32K | 1M | 32M | us-west2 | open_ai |
| Qwen 3 Coder 480B | Qwen | 128K | 32K | 1M | 32M | us-south1 | open_ai |
| Meta Llama 3.1 405B | Meta | 128K | 32K | 1M | 32M | us-central1 | open_ai |
| Minimax M2 | Minimax AI | 64K | 32K | 1M | 32M | Global | open_ai |

## Model Selection Guide

- **Claude Opus 4-1**: Best for complex reasoning, creative tasks, and high-quality outputs
- **Claude Sonnet 4-5**: Balanced performance and cost, good for general-purpose use
- **Deepseek V3.1**: Optimized for coding and technical tasks
- **Qwen 3 Coder 480B**: Specialized for code generation and understanding
- **Meta Llama 3.1 405B**: Large open model, good for general-purpose tasks
- **GPT OSS 120B**: Smaller open model, faster responses
- **Minimax M2**: Smaller context window, suitable for focused tasks

# Supported models

The following models have been integrated and are available for use through shaide:

## Anthropic Models

### Claude Opus 4-1
```
cargo run --bin shaide-cli -- create-model --name "claude-opus-4-1" --variant "claude-opus-4-1" --url "https://aiplatform.googleapis.com/v1/projects/your-gcp-project-id/locations/global/publishers/anthropic/models/claude-opus-4-1:streamRawPredict" --api-schema "anthropic" --daily-input-token-limit 1000000 --daily-output-token-limit 32000000 --platform vertex --max-generated-tokens 32000 --context-size 200000 --remote http://localhost:8080/v1 --admin-password <ADMIN_PASSWORD>
```

### Claude Sonnet 4-5
```
cargo run --bin shaide-cli -- create-model --name "claude-sonnet-4-5" --variant "claude-sonnet-4-5" --url "https://aiplatform.googleapis.com/v1/projects/your-gcp-project-id/locations/global/publishers/anthropic/models/claude-sonnet-4-5:streamRawPredict" --api-schema "anthropic" --daily-input-token-limit 1000000 --daily-output-token-limit 32000000 --platform vertex --max-generated-tokens 32000 --context-size 200000 --remote http://localhost:8080/v1 --admin-password <ADMIN_PASSWORD>
```

## OpenAI Compatible Models

### GPT OSS 120B MaaS
```
cargo run --bin shaide-cli -- create-model --name "openai/gpt-oss-120b-maas" --variant "openai/gpt-oss-120b-maas" --url "https://aiplatform.googleapis.com/v1/projects/your-gcp-project-id/locations/global/endpoints/openapi/chat/completions" --api-schema "open_ai" --daily-input-token-limit 1000000 --daily-output-token-limit 32000000 --platform vertex --max-generated-tokens 32000 --context-size 128000 --remote http://localhost:8080/v1 --admin-password <ADMIN_PASSWORD>
```

### Deepseek V3.1 MaaS
```
cargo run --bin shaide-cli -- create-model --name "deepseek-ai/deepseek-v3.1-maas" --variant "deepseek-ai/deepseek-v3.1-maas" --url "https://us-west2-aiplatform.googleapis.com/v1/projects/your-gcp-project-id/locations/us-west2/endpoints/openapi/chat/completions" --api-schema "open_ai" --daily-input-token-limit 1000000 --daily-output-token-limit 32000000 --platform vertex --max-generated-tokens 32000 --context-size 128000 --remote http://localhost:8080/v1 --admin-password <ADMIN_PASSWORD>
```

### Qwen 3 Coder 480B Instruct MaaS
```
cargo run --bin shaide-cli -- create-model --name "qwen/qwen3-coder-480b-a35b-instruct-maas" --variant "qwen/qwen3-coder-480b-a35b-instruct-maas" --url "https://us-south1-aiplatform.googleapis.com/v1beta1/projects/your-gcp-project-id/locations/us-south1/endpoints/openapi/chat/completions" --api-schema "open_ai" --daily-input-token-limit 1000000 --daily-output-token-limit 32000000 --platform vertex --max-generated-tokens 32000 --context-size 128000 --remote http://localhost:8080/v1 --admin-password <ADMIN_PASSWORD>
```

### Meta Llama 3.1 405B Instruct MaaS
```
cargo run --bin shaide-cli -- create-model --name "meta/llama-3.1-405b-instruct-maas" --variant "meta/llama-3.1-405b-instruct-maas" --url "https://us-central1-aiplatform.googleapis.com/v1beta1/projects/your-gcp-project-id/locations/us-central1/endpoints/openapi/chat/completions" --api-schema "open_ai" --daily-input-token-limit 1000000 --daily-output-token-limit 32000000 --platform vertex --max-generated-tokens 32000 --context-size 128000 --remote http://localhost:8080/v1 --admin-password <ADMIN_PASSWORD>
```

### Minimax M2 MaaS
```
cargo run --bin shaide-cli -- create-model --name "minimaxai/minimax-m2-maas" --variant "minimaxai/minimax-m2-maas" --url "https://aiplatform.googleapis.com/v1/projects/your-gcp-project-id/locations/global/endpoints/openapi/chat/completions" --api-schema "open_ai" --daily-input-token-limit 1000000 --daily-output-token-limit 32000000 --platform vertex --max-generated-tokens 32000 --context-size 64000 --remote http://localhost:8080/v1 --admin-password <ADMIN_PASSWORD>
```
