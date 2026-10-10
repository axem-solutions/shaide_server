# shaide server

This repository contains the shaide server.

# Environment variables

The following table describes the current environment variables that we use.

| Env var name          | Optional? | Comment                                                          |
| --------------------- | --------- | ---------------------------------------------------------------- |
| ADMIN_PASSWORD        | No        | Password used to create the initial `admin` user                 |
| JWT_SECRET            | No        | Secret used to sign user JWTs; must be at least 32 bytes         |
| SHAIDE_SERVER_UI_FQDN | No        | The server UI address                                            |
| SHAIDE_SERVER_UI_PORT | No        | The server UI port                                               |
| WEBAPP_URL            | Yes       | WebApp upstream URL for the `/app` reverse proxy (unset: no proxy) |
| GCP_API_KEY           | Yes       | GCP API key/token (if empty, auth must be provided by other means) |
| HOST                  | Yes       | Server bind host (default: `0.0.0.0`)                            |
| PORT                  | Yes       | Server bind port (default: `8080`)                               |
| VECTOR_DB_URL         | Yes       | Vector DB URL (default: `http://localhost:6334`)                 |
| DATABASE_URL          | Yes       | Database URL used by SQLx tooling/migrations                     |

Example `.env` file.

```env
ADMIN_PASSWORD=admin_password
JWT_SECRET=replace_with_a_random_secret_of_at_least_32_bytes
SHAIDE_SERVER_UI_FQDN=control-panel.localhost
SHAIDE_SERVER_UI_PORT=3000
WEBAPP_URL=http://localhost:3001
GCP_API_KEY=gcp_api_key
HOST=0.0.0.0
PORT=8080
VECTOR_DB_URL=http://localhost:6334

DATABASE_URL=sqlite://crates/shaide-db/schema.sqlite
```

`WEBAPP_URL` is optional: without it the server does not serve `/app`. It must
be reachable from the server. `just dev app` and `just stack app` start the
WebApp container and set it for you (`http://localhost:3001` natively,
`http://webapp:8787` in compose); export `WEBAPP_URL` to point `just dev` at a
WebApp you run yourself. Include any upstream base path in the URL:
`/app/assets/main.js` is forwarded to `<WEBAPP_URL>/assets/main.js`.

# Authentication

Users and admins exchange their username and password for a one-hour JWT:

```sh
curl -X POST http://localhost:8080/v1/login \
  -H 'Content-Type: application/json' \
  -d '{"username":"admin","password":"admin-password"}'
```

Send the returned `access_token` to authenticated endpoints as
`Authorization: Bearer <access_token>`.

Every authenticated request reloads the account from the database. Role changes,
regular-user account expiry and account deletion therefore apply to existing
tokens. Admin routes accept only admins; regular-user routes accept only active
regular users. Both roles can list models using `/v1/models` and
`/v1/embedding_models`, with the existing response formats.

Handlers that need the authenticated identity can use the `Principal` extractor.
It exposes the current database user, role and usage `request_id` after one
authorization. Use one auth extractor per handler: `Principal` accepts either
role, while `Admin` and `AuthUser` enforce their existing role restrictions.
Successful authorization creates one API usage row, including when a later
handler rejects the request; failed authentication or role checks create none.

# Migrations

To create a new migration, you may do the following:

```
sqlx migrate add --source crates/shaide-db/migrations -r -s migration_name
```

To run the migrations:

```
cargo sqlx migrate run --source crates/shaide-db/migrations
```

And to revert a migration, you can:

```
cargo sqlx migrate revert --source crates/shaide-db/migrations
```

# To run it locally, natively

```sh
cargo run # run the server
cargo run --bin shaide-cli -- list-users # list users with the cli. see the cli docs for more info
```

# To build the container image

To build the container image, you have to run:

```sh
docker buildx build --tag shaide:{version} .
```

Before pushing the docker image to an external registry, retag it for the target
registry, for example:

```sh
docker tag shaide:latest us-east5-docker.pkg.dev/your-gcp-project-id/proxy/shaide:{version}
docker push us-east5-docker.pkg.dev/your-gcp-project-id/proxy/shaide:{version}
```

# To run the container locally

To run the built container image locally with Vertex AI as provider:

1. Authenticate with Google Cloud using your personal account:

```bash
gcloud auth application-default login
```

Alternatively you can also impersonate the service account, but for local
development the personal account is recommended.

2. Run the container with:

```bash
docker run -p 8080:8080 \
    --add-host=host.docker.internal:host-gateway \
    -e GCP_API_KEY=$(gcloud auth application-default print-access-token) \
    -e ADMIN_PASSWORD={your-admin-password} \
    -e JWT_SECRET={your-jwt-secret-of-at-least-32-bytes} \
    -e SHAIDE_SERVER_UI_FQDN=dummy \
    -e SHAIDE_SERVER_UI_PORT=dummy \
    -e WEBAPP_URL=http://host.docker.internal:3001 \
    -v ~/.config:/root/.config \
    shaide:{version}
```

**Notes:**

- `your-admin-password` creates the initial `admin` user and is used to log in
- `GCP_API_KEY` is obtained from your authenticated Google Cloud session
- The dummy values for `SHAIDE_SERVER_UI_FQDN` and `SHAIDE_SERVER_UI_PORT` are
  required but not used when using Vertex AI
- The `~/.config:/root/.config` volume mount provides access to the shaide
  server database

## Docker compose

`compose.yaml` mirrors the k8s deployment and runs in one of two modes:

```sh
# Everything the shaide server depends on, to run the server natively
SHAIDE_SERVER_FQDN=host.docker.internal docker compose up -d
cargo run

# Every service in containers, the shaide server included
docker compose --profile server up -d --build
```

`just dev` and `just stack` wrap these. The webapp (`app` profile) and the MCP
gateway (`mcp` profile) are optional: pass the profile names to the recipes,
e.g. `just dev app mcp`, or `--profile app` to compose. When running the
containerized server with the webapp, also set `WEBAPP_URL=http://webapp:8787`.

The containerized server is built from the `dev` Docker target, which adds
`azure-cli`. It keeps its data in `~/.config/axem-docker`, separate from the
native server's `~/.config/axem`, and reads a copy of `~/.azure` (create the
directory if you do not use Azure).

Images default to a local build of this repository and the released WebApp and
Control Panel. To switch, set `SHAIDE_SERVER_IMAGE`, `SHAIDE_WEBAPP_IMAGE` or
`SHAIDE_CONTROL_PANEL_IMAGE`, for example:

```sh
export SHAIDE_SERVER_IMAGE=ghcr.io/axem-solutions/shaide_server:v1.0.0
export SHAIDE_CONTROL_PANEL_IMAGE=shaide_control_panel:latest
```

Released server images are built from the `runtime` target and do not include
`azure-cli`.

The Qdrant dashboard is available at http://localhost:6333/dashboard.

Stop everything with `just services-down` or
`docker compose --profile '*' down`.

# Documentation

To run the documentation:

```sh
cargo install mdbook
cd documentation
mdbook serve
```

# Using just

To run commonly used commands, you can use
[just](https://github.com/casey/just).

```
Available recipes:
    check                  # Run the same checks as CI
    db-migrate             # Apply all pending migrations
    db-new name            # Create a new migration
    db-prepare             # Regenerate SQLx offline query data
    db-revert              # Revert the latest migration
    db-shell [database]    # Open a SQLite database
    default                # Default recipe
    dev *profiles          # Start local dependencies and run the server natively
    docker-build [tag]     # Build a local server image
    services-down          # Stop every compose service
    services-up *profiles  # Start what a natively running server needs
    stack *profiles        # Run every service in containers, the server too
```

`db-shell` defaults to the server database under `~/.config/axem/shaide`, and
`docker-build` defaults to the image tag `shaide-server:local`.

## AI Platform model discovery

`GET /v1/platform-models` is an **admin-only** catalog of models deployed on the
AI Platform. It is separate from `/v1/models` and `/v1/embedding_models`, which
continue to list database registrations. Discovery does not add database rows:
removing a registration still disables user inference until an administrator
explicitly adds it again.

The server lists Kubernetes Services with these labels using its in-cluster
ServiceAccount and the `shaide-server-topology-reader` RBAC from shaide PR #8:

- `app.kubernetes.io/part-of=app-serving`
- `axem.dev/model-slug`
- `axem.dev/model-category=generative` or `embedder`

Generative models use the labeled ExternalName gateway Service (HTTP port 80
when no port is declared). Embedders use their labeled ClusterIP Service and
its declared HTTP port. The server does not reconstruct deployment names, read
Pulumi files, use an ambient local kubeconfig, or consume `vllm-config.json`.

Each target is queried with **GET only** at `/v1/shaide/metadata`
and `/v1/models`. The latter supplies public served names rather than local
weight paths, including aliases but excluding LoRA adapters. No inference probe
is made, including for embedding dimensions. Requests have three-second timeouts,
eight-way concurrency, a 20-second metadata deadline, a 2 MiB response limit,
and no redirect following or credential forwarding. Results (including errors)
are cached for 30 seconds, and concurrent refreshes share the same work.

The response contains `models` and `warnings`. Each model has a `kind` of
`generative` or `embedding` plus the fields accepted by the corresponding
registration endpoint; `api_schema` is `open_ai` and `platform` is `axem`.
A warning contains `source` (namespace/Service or a conflicting served name)
and `message`. Unavailable or malformed metadata excludes only the affected
Service. Duplicate names within a model kind exclude all conflicting entries.
Failure to access Kubernetes returns HTTP 503. The Control Panel renders these
errors while preserving custom-model registration.

### Runtime metadata requirements

The platform exposes **`GET /v1/shaide/metadata`** using a small module loaded
through vLLM's supported `--middleware` option. Enable
`shaide.runtimeMetadata.enabled: true` in the model's values file. The platform
mounts the embedded module from a read-only ConfigMap and explicitly disables
`VLLM_SERVER_DEV_MODE`. No sidecar, inference request, or full configuration dump
is needed. The route remains behind vLLM's existing authentication middleware.
Deployments protected by a vLLM API key currently report unavailable metadata:
discovery does not forward administrator credentials to model services.

The response must declare `schema_version: 1`. Other versions, unavailable
metadata, or malformed responses exclude the affected model with an error.
There is no fallback to the development-only `/server_info` endpoint.

The response must contain a structured `vllm_config.model_config`:

- Served names come from `/v1/models`, then `served_model_name`, then `model`.
- Generative `max_model_len` supplies context size. An explicit
  `override_generation_config.max_new_tokens` caps output; otherwise shaide's
  output upper bound is the context size. Input still consumes that window,
  and upstream generation policies can impose a lower limit.
- Image support and count are derived from runtime image configuration.
  Unsupported/unreported reasoning effort, Responses, FIM and image byte/pixel
  limits are not inferred from model names.
- Embedding output dimensions must be explicit in
  `model_config.pooler_config.dimensions`. `hidden_size` is not a reliable
  substitute because projection/pooling can change output dimensions. If absent,
  expose the output dimensions in runtime metadata before retrying, or register
  known settings through the existing server model API. Discovery never runs an
  embedding request to measure dimensions.

A local Compose deployment without Kubernetes returns a discovery-unavailable
error. Installed models, third-party models and add/remove operations continue
working through the existing APIs.

For local end-to-end testing without a cluster, see the isolated
[platform discovery Compose demo](test/platform-discovery/README.md). It mocks
the Kubernetes API and vLLM metadata while exercising the real server and
Control Panel, including missing-metadata errors, without inference probes.
