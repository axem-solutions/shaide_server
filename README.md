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
| GCP_API_KEY           | Yes       | GCP API key/token (if empty, auth must be provided by other means) |
| HOST                  | Yes       | Server bind host (default: `0.0.0.0`)                            |
| PORT                  | Yes       | Server bind port (default: `8080`)                               |
| VECTOR_DB_URL         | Yes       | Vector DB URL (default: `http://localhost:6334`)                 |
| DATABASE_URL          | Yes       | Local Turso URL used for online query checking                     |

Example `.env` file.

```env
ADMIN_PASSWORD=admin_password
JWT_SECRET=replace_with_a_random_secret_of_at_least_32_bytes
SHAIDE_SERVER_UI_FQDN=control-panel.localhost
SHAIDE_SERVER_UI_PORT=3000
GCP_API_KEY=gcp_api_key
HOST=0.0.0.0
PORT=8080
VECTOR_DB_URL=http://localhost:6334

DATABASE_URL=turso://${HOME}/.config/axem/shaide/db/shaide-server.turso
SQLX_OFFLINE=true
```

# Authentication

Users and admins exchange their username and password for a one-hour JWT:

```sh
curl -X POST http://localhost:8080/v1/login \
  -H 'Content-Type: application/json' \
  -d '{"username":"admin","password":"admin-password"}'
```

Send the returned `access_token` to authenticated endpoints as
`Authorization: Bearer <access_token>`.

# Database and migrations

The server uses the embedded Rust [Turso database](https://github.com/tursodatabase/turso)
through `sqlx-turso`, with no remote service or credentials. This experiment uses
`~/.config/axem/shaide/db/shaide-server.turso` (or under `shaide_ROOT` when set).
It starts a fresh database; the previous `shaide-server.sqlite` is not imported.
The initial schema has a constraint-order adjustment required by Turso, so do not
rename an old SQLite file to the new path.

Startup applies migrations automatically. For development:

```sh
cargo install sqlx-turso-cli --version 0.1.0-alpha.1 --locked
cargo install sqlx-cli --version 0.9.0 --locked
just db-new migration_name
just db-migrate
just db-prepare
```

`db-migrate`, `db-revert`, and `db-prepare` default to the local server database.
Stop the server before using them. Pass a file path to target another database.
No separate schema database is needed; offline builds use the `.sqlx` cache. `just db-shell` uses the local `tursodb`
CLI, which is separate from the Turso Cloud CLI.

Committed `.sqlx` metadata supports builds without a schema database:

```sh
SQLX_OFFLINE=true cargo build
```

See [database development](crates/shaide-db/docs/sqlx.md) for details and the
adapter's experimental limitations.

# To run it locally, natively

```sh
SQLX_OFFLINE=true cargo run -p shaide # run the server
SQLX_OFFLINE=true cargo run --bin shaide-cli -- list-users # list users with the cli. see the cli docs for more info
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
    -e GCP_API_KEY=$(gcloud auth application-default print-access-token) \
    -e ADMIN_PASSWORD={your-admin-password} \
    -e JWT_SECRET={your-jwt-secret-of-at-least-32-bytes} \
    -e SHAIDE_SERVER_UI_FQDN=dummy \
    -e SHAIDE_SERVER_UI_PORT=dummy \
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

To run all services with compose, you can:

```sh
docker compose up
```

For development, you can start the supporting services with:

```sh
docker compose up -d caddy vectordb control-panel mcp-gateway
```

and when you are done with development, or just want to kill the processes

```sh
docker compose down
```

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
    check                  # Run formatting, Clippy, and tests
    db-migrate [database]  # Apply all pending migrations
    db-new name            # Create a new migration
    db-prepare             # Regenerate SQLx offline query data
    db-revert [database]   # Revert the latest migration
    db-shell [database]    # Open a Turso database
    default                # Default recipe
    dev                    # Start local dependencies and run the server
    docker-build [tag]     # Build a local server image
    services-down          # Stop local backend dependencies
    services-up            # Start local backend dependencies
```

`db-shell` defaults to the server database under `~/.config/axem/shaide`, and
`docker-build` defaults to the image tag `shaide-server:local`.
