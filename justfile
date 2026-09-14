# Common recipes for local development
set shell := ["bash", "-cu"]

migrations_dir := "crates/shaide-db/migrations"
server_db := env_var_or_default("shaide_ROOT", env_var("HOME") / ".config/axem/shaide") / "db/shaide-server.turso"
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
    SQLX_OFFLINE=true cargo run -p shaide

# Regenerate SQLx offline query data
db-prepare database=server_db: (db-migrate database)
    cargo clean -p shaide-db
    SQLX_OFFLINE=false sqlx-turso prepare --database-url "turso://{{ absolute_path(database) }}" -- -p shaide-db --lib

# Apply all pending migrations
db-migrate database=server_db:
    SQLX_OFFLINE=true cargo run -p shaide-db-migrate -- run "{{ database }}"

# Revert the latest migration
db-revert database=server_db:
    SQLX_OFFLINE=true cargo run -p shaide-db-migrate -- revert "{{ database }}"

# Open a Turso database, defaulting to the server's local database
db-shell database=server_db:
    tursodb "{{ database }}"

# Create a new migration
db-new name:
    cargo sqlx migrate add --source {{ migrations_dir }} -r -s {{ name }}

# Build a local server image
docker-build tag=docker_local_tag:
    docker buildx build --tag {{ tag }} .
