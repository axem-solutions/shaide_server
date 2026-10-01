# Common recipes for local development
set shell := ["bash", "-cu"]

migrations_dir := "crates/shaide-db/migrations"
docker_local_tag := "shaide-server:local"

default:
    @just --list

# Run the same checks as CI
check:
    cargo fmt --all -- --check
    cargo clippy --workspace --all-targets --all-features -- -D warnings
    cargo test --workspace --all-features

# Start what a natively running shaide server needs; optional profiles: app, mcp
[env("SHAIDE_SERVER_FQDN", "host.docker.internal")]
services-up *profiles:
    # Free the server port in case the containerized server is still running.
    docker compose --profile server stop shaide_server
    docker compose {{ prepend("--profile ", profiles) }} up -d --remove-orphans

# Stop every compose service, including the optional ones
services-down:
    docker compose --profile '*' down --remove-orphans

# Start local dependencies and run the server natively; optional profiles: app, mcp
[env("ADMIN_PASSWORD", "admin")]
[env("JWT_SECRET", "local-development-jwt-secret-change-me")]
[env("RUST_LIB_BACKTRACE", "1")]
[env("RUST_SPANTRACE", "0")]
[env("SHAIDE_SERVER_UI_FQDN", "localhost")]
[env("SHAIDE_SERVER_UI_PORT", "3000")]
dev *profiles: (services-up profiles)
    {{ if profiles =~ '(^|\s)app(\s|$)' { 'export WEBAPP_URL="${WEBAPP_URL:-http://localhost:3001}";' } else { '' } }} cargo run

# Run every service in containers, the server too; optional profiles: app, mcp
stack *profiles:
    {{ if profiles =~ '(^|\s)app(\s|$)' { 'export WEBAPP_URL=http://webapp:8787;' } else { '' } }} \
    docker compose --profile server {{ prepend("--profile ", profiles) }} up -d --remove-orphans \
        {{ if env("SHAIDE_SERVER_IMAGE", "") == "" { "--build" } else { "" } }}

# Regenerate SQLx offline query data
db-prepare:
    cargo sqlx prepare --workspace

# Apply all pending migrations
db-migrate:
    cargo sqlx migrate run --source {{ migrations_dir }}

# Revert the latest migration
db-revert:
    cargo sqlx migrate revert --source {{ migrations_dir }}

# Open a SQLite database, defaulting to the server's local database
db-shell database="$HOME/.config/axem/shaide/db/shaide-server.sqlite":
    sqlite3 "{{ database }}"

# Create a new migration
db-new name:
    cargo sqlx migrate add --source {{ migrations_dir }} -r -s {{ name }}

# Build a local server image
docker-build tag=docker_local_tag:
    docker buildx build --tag {{ tag }} .
