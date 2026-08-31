# Common recipes for local development
set shell := ["bash", "-cu"]

migrations_dir := "crates/shaide-db/migrations"
docker_local_tag := "shaide-server:local"

default:
    @just --list

# Run the same checks as CI
check:
    cargo fmt --all -- --check
    SQLX_OFFLINE=true cargo clippy --workspace --all-targets --all-features -- -D warnings
    SQLX_OFFLINE=true cargo test --workspace --all-features

# Start local backend dependencies
services-up:
    docker compose up -d vectordb

# Stop local backend dependencies
services-down:
    docker compose down

# Start local dependencies and run the server
[env("ADMIN_PASSWORD", "admin")]
[env("JWT_SECRET", "local-development-jwt-secret-change-me")]
[env("RUST_LIB_BACKTRACE", "1")]
[env("RUST_SPANTRACE", "0")]
[env("SHAIDE_SERVER_UI_FQDN", "localhost")]
[env("SHAIDE_SERVER_UI_PORT", "3000")]
dev: services-up
    cargo run

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
