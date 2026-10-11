#!/usr/bin/env bash
# Package native Linux builds for a quick local demo. Default: rebuild first.
set -euo pipefail
DEMO_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
SERVER_SOURCE=$(cd "$DEMO_DIR/../.." && pwd)
CONTROL_PANEL_SOURCE=${CONTROL_PANEL_SOURCE:-$(cd "$SERVER_SOURCE/../control-panel-platform-discovery" && pwd)}
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-$SERVER_SOURCE/target}
export CARGO_PROFILE_DEV_DEBUG=0
export CARGO_BUILD_JOBS=${CARGO_BUILD_JOBS:-2}
if [[ $# -gt 1 || ( $# -eq 1 && $1 != --reuse-builds ) ]]; then
  echo 'Usage: build-local-images.sh [--reuse-builds]' >&2
  exit 2
fi
if [[ ${1:-} != --reuse-builds ]]; then
  (cd "$SERVER_SOURCE" && cargo build --locked --all-features)
  (cd "$CONTROL_PANEL_SOURCE" && npm ci && npm run build)
fi
# The packaged artifacts must be Linux/glibc builds for the host architecture.
test -x "$CARGO_TARGET_DIR/debug/shaide"
test -f "$CONTROL_PANEL_SOURCE/.next/standalone/server.js"
BUILD_CONTEXT=$(mktemp -d -t shaide-discovery-images.XXXXXX)
trap 'rm -rf -- "$BUILD_CONTEXT"' EXIT
mkdir -p "$BUILD_CONTEXT/server" "$BUILD_CONTEXT/panel/.next"
cp "$CARGO_TARGET_DIR/debug/shaide" "$BUILD_CONTEXT/server/shaide"
cat > "$BUILD_CONTEXT/server/Dockerfile" <<'DOCKERFILE'
FROM debian:trixie-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates libssl3t64 \
    && rm -rf /var/lib/apt/lists/*
COPY shaide /usr/local/bin/shaide
CMD ["shaide"]
DOCKERFILE
cp -a "$CONTROL_PANEL_SOURCE/.next/standalone/." "$BUILD_CONTEXT/panel/"
cp -a "$CONTROL_PANEL_SOURCE/.next/static" "$BUILD_CONTEXT/panel/.next/"
cp -a "$CONTROL_PANEL_SOURCE/public" "$BUILD_CONTEXT/panel/"
cat > "$BUILD_CONTEXT/panel/Dockerfile" <<'DOCKERFILE'
FROM node:20-bookworm-slim
WORKDIR /app
COPY --chown=node:node . .
USER node
ENV NODE_ENV=production HOSTNAME=0.0.0.0 PORT=3000
CMD ["node", "server.js"]
DOCKERFILE
docker build -t shaide-server:discovery-demo "$BUILD_CONTEXT/server"
docker build -t shaide-control-panel:discovery-demo "$BUILD_CONTEXT/panel"
docker compose -f "$DEMO_DIR/compose.yaml" build platform-mock
