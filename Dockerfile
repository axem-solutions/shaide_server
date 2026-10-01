# Keep in step with rust-toolchain.toml.
FROM rust:1.98.1 AS base
WORKDIR /app
RUN cargo install cargo-chef --locked

# build recepie
FROM base AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM base AS builder
RUN apt-get update && apt-get install -y --no-install-recommends cmake
ENV SQLX_OFFLINE=true
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json
COPY . .
RUN cargo install --locked --path ./crates/shaide

FROM debian:trixie-slim AS runtime-base
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Local development image, built by compose. azure-cli is needed by
# DeveloperToolsCredential, which shells out to `az` to authenticate against
# foundry when AZURE_FEDERATED_TOKEN_FILE is not set. In k8s workload identity
# sets it, so the release image does not ship the CLI.
FROM runtime-base AS dev
RUN apt-get update && apt-get install -y --no-install-recommends python3 python3-venv \
    && python3 -m venv /opt/venv \
    && /opt/venv/bin/pip install --no-cache-dir azure-cli \
    && rm -rf /var/lib/apt/lists/*
ENV PATH="/opt/venv/bin:${PATH}"
COPY --from=builder /usr/local/cargo/bin/shaide /usr/local/bin/shaide
CMD ["shaide"]

# Release image. Keep it the last stage: CI builds the default target.
FROM runtime-base AS runtime
COPY --from=builder /usr/local/cargo/bin/shaide /usr/local/bin/shaide
CMD ["shaide"]
