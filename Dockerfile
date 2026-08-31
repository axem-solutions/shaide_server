FROM rust:1.96.1 AS base
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

FROM debian:trixie-slim
COPY --from=builder /usr/local/cargo/bin/shaide /usr/local/bin/shaide
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates
ENV PATH="/opt/venv/bin:${PATH}"

CMD ["shaide"]
