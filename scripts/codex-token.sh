#!/usr/bin/env sh
# Prints a shaide access token to stdout for the Codex CLI `auth.command` hook.
#
# Required environment variables:
#   SHAIDE_URL       shaide server base URL, e.g. https://shaide.example.com
#   SHAIDE_USERNAME  shaide user name
#   SHAIDE_PASSWORD  shaide user password
set -eu

: "${SHAIDE_URL:?SHAIDE_URL is not set}"
: "${SHAIDE_USERNAME:?SHAIDE_USERNAME is not set}"
: "${SHAIDE_PASSWORD:?SHAIDE_PASSWORD is not set}"

body=$(python3 -c 'import json, os; print(json.dumps({"username": os.environ["SHAIDE_USERNAME"], "password": os.environ["SHAIDE_PASSWORD"]}))')

curl --silent --show-error --fail-with-body \
    --header 'Content-Type: application/json' \
    --data "$body" \
    "${SHAIDE_URL%/}/v1/login" |
    python3 -c 'import json, sys; print(json.load(sys.stdin)["access_token"])'
