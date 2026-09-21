# Codex CLI

The [Codex CLI](https://github.com/openai/codex) (Apache 2.0) can use shaide as a custom model
provider. Codex talks to its provider through the OpenAI Responses API only
(`wire_api = "responses"` is the only supported value), so shaide exposes Codex traffic through
`POST /v1/responses` and forwards it to the model's `responses_endpoint`, for example an OpenAI
model deployed on Azure AI Foundry.

## How shaide handles Codex requests

- **Requests** are forwarded as-is. Codex-specific fields (`include`, `store`,
  `prompt_cache_key`, `reasoning`, custom and function tools, encrypted reasoning items) reach the
  provider unchanged.
- **Stream events** are forwarded verbatim, including event types and fields that shaide does not
  know about. Token usage from `response.completed` is recorded against the user, so the daily
  token limits of the model apply to Codex too.
- **Errors** keep the upstream status code and body, so Codex can recognise context window,
  rate limit and quota errors.
- **Model catalog**: Codex calls `GET /v1/models?client_version=...`. shaide answers these requests
  with an empty Codex catalog, so Codex falls back to its bundled metadata for the model slug.

## 1. Register the model

The shaide model name is sent upstream as `model`, so for Foundry it must match the deployment
name. Keeping the OpenAI model name as the deployment name (e.g. `gpt-5.6-sol`) also lets Codex use
its bundled metadata for the model. The Responses endpoint is shown on the deployment's details page
in Foundry; shaide authenticates with Entra ID, so API key authentication can stay disabled.

`--reasoning-effort-values` is validated against what `/v1/chat/completions` can forward (`none`,
`minimal`, `low`, `medium`, `high`, `xhigh`). It does not restrict `/v1/responses`, so Codex can
still request higher efforts the model supports.

```sh
shaide-cli create-model \
  --name gpt-5.6-sol \
  --variant gpt-5.6-sol \
  --platform foundry \
  --api-schema open_ai \
  --chat-completions-endpoint https://<resource>.services.ai.azure.com/openai/v1/chat/completions \
  --responses-endpoint https://<resource>.services.ai.azure.com/openai/v1/responses \
  --reasoning-effort-values low,medium,high,xhigh \
  --supports-images true \
  --context-size 1100000 \
  --max-generated-tokens 128000 \
  --remote https://shaide.example.com \
  --admin-password "$ADMIN_PASSWORD"
```

## 2. Get a token

shaide access tokens expire after one hour, so use a Codex `auth.command` instead of a static API
key. [`scripts/codex-token.sh`](https://github.com/axem-solutions/shaide_server/blob/main/scripts/codex-token.sh)
logs in with `SHAIDE_USERNAME` / `SHAIDE_PASSWORD` and prints a fresh access token.

```sh
export SHAIDE_URL=https://shaide.example.com
export SHAIDE_USERNAME=alice
export SHAIDE_PASSWORD=...
```

## 3. Configure Codex

Keep Codex's built-in OpenAI provider as the default and add shaide as a profile. A profile is a
`<name>.config.toml` file next to `config.toml` that is layered on top of it, so the shaide
settings never leak into OpenAI sessions.

`~/.codex/config.toml` stays as it is, for example:

```toml
model = "gpt-5.5"
```

`~/.codex/shaide.config.toml`:

```toml
model = "gpt-5.6-sol"
model_provider = "shaide"

[model_providers.shaide]
name = "shaide"
base_url = "https://shaide.example.com/v1"
wire_api = "responses"

[model_providers.shaide.auth]
command = "/path/to/shaide_server/scripts/codex-token.sh"
# Refresh well before the one hour token lifetime ends.
refresh_interval_ms = 3000000
```

Use both side by side:

```sh
codex                    # OpenAI, with your ChatGPT login or OPENAI_API_KEY
codex --profile shaide   # shaide
codex exec --profile shaide "explain this repository"
```

Codex re-runs the auth command when shaide answers with `401`, so an expired token is replaced
automatically. For model names Codex does not know, also set `model_context_window` in
`shaide.config.toml`, because Codex cannot read it from shaide's catalog.

> Codex 0.154 and newer reject the legacy `[profiles.shaide]` table in `config.toml` when
> `--profile shaide` is used. Use the separate `shaide.config.toml` file instead.

## Troubleshooting

- `400 Model '<name>' does not expose an OpenAI-compatible Responses endpoint`: the model was
  created without `--responses-endpoint`.
- `403 model_usage_limit_reached`: the user hit the daily token limit of the model.
- Foundry `404 DeploymentNotFound`: the shaide model name differs from the Foundry deployment name.
- `400 invalid_encrypted_content`: the conversation was started with another provider (for example
  plain `codex` against OpenAI) and continued with `--profile shaide`. Encrypted reasoning and
  compaction items can only be decrypted by the provider that created them. Start a new session
  with `codex --profile shaide`, and only resume sessions that were created through shaide.
