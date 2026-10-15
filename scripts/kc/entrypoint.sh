#!/bin/sh
# Components currently read these credentials from env, not *_FILE. Do not trace.
set -eu
read_secret() {
    value=$(cat "/run/secrets/$1")
    [ -n "$value" ] || { echo 'Missing or empty secret file' >&2; exit 1; }
}
case "$1" in
    server)
        read_secret admin_password; export ADMIN_PASSWORD="$value"
        read_secret jwt_secret; export JWT_SECRET="$value"
        read_secret internal_token; export KNOWLEDGE_CENTER_INTERNAL_TOKEN="$value"
        read_secret webhook_token; export S3_WEBHOOK_TOKEN="$value"
        unset value
        exec shaide ;;
    kc)
        read_secret internal_token; export KNOWLEDGE_CENTER_INTERNAL_TOKEN="$value"
        read_secret embedding_password; export KC_SHAIDE_PASSWORD="$value"
        read_secret storage_access_key; export S3_USER="$value"
        read_secret storage_secret_key; export S3_PASSWORD="$value"
        unset value
        exec shaide_knowledge_center ;;
    rustfs)
        read_secret storage_access_key; export RUSTFS_ACCESS_KEY="$value"
        read_secret storage_secret_key; export RUSTFS_SECRET_KEY="$value"
        read_secret webhook_token; export RUSTFS_NOTIFY_WEBHOOK_AUTH_TOKEN_SHAIDE="$value"
        unset value
        exec /entrypoint.sh rustfs ;;
    control-panel)
        read_secret session_secret; export SESSION_SECRET="$value"
        unset value
        exec node server.js ;;
    *) echo 'Unknown component' >&2; exit 1 ;;
esac
