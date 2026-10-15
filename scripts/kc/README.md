# Optional local Knowledge Center

Include `compose.kc.yaml` explicitly to enable the product's public upload facade,
private Knowledge Center (KC), RustFS, dedicated Qdrant and signed KC-MCP retrieval.
The normal `compose.yaml`, profiles, Control Panel and generic `vectordb` behavior
are unchanged when this overlay is absent. No KC configuration is needed then.

## Prerequisites

- Docker Compose **2.24.4 or newer** (`!override` / `!reset` support), Python 3.10+
  and OpenSSL on the operator host. No sudo is needed by these helpers.
- Locally available, trusted server, private KC and private KC-MCP images that
  implement the public upload/internal storage and signed collection-context
  contracts. KC must contain `shaide_knowledge_center`, `sh`, and its extraction
  runtime; KC-MCP must start `kc-mcp`. The server must contain `shaide` and `sh`.
  Supply your own image references; this repository does not publish the private
  images, promise a private registry tag, or build/pull them automatically.
- A locally available RustFS image with `/entrypoint.sh`, `rustfs`, `sh`, webhook
  notifications and the `RUSTFS_NOTIFY_*_SHAIDE` settings below; Qdrant with gRPC on
  6334; and Caddy 2 for the notification relay. Pin trusted image digests in your
  local configuration after validating compatibility. `pull_policy: never` makes
  missing images a clear prerequisite failure.
- An authorized embedding provider with known dimensions, a dedicated **role=user**
  shaide service account, and a registered embedding model. The setup helper only
  creates files. Account/model registration is a separate explicit operator action.
  Do not use the admin account for KC embedding or register real accounts for a test.
- A stable, private credential directory outside the source tree, and enough disk
  for Docker's persistent volumes. The new server database is in its own named
  volume; existing native/container databases are not imported or migrated.

## Create protected files and start

Choose a new destination whose parent already exists and is not a symlink. Replace
all angle-bracket placeholders with your actual local image references and path.
Image references and paths contain no passwords. The helper refuses existing files,
directories, symlinks and dangling symlinks, including symlink parents. It creates
its directory with mode 0700 and every file exclusively with mode 0600, regardless
of umask, and removes only its newly created files if setup fails. It never updates,
truncates or changes permissions on existing credentials.

```sh
export SHAIDE_SERVER_IMAGE='<your-compatible-local-server-image>'
python3 scripts/kc/setup.py '<new-private-directory>' \
  --kc-image '<your-private-or-local-kc-image>' \
  --kc-mcp-image '<your-private-or-local-kc-mcp-image>' \
  --rustfs-image '<your-local-compatible-rustfs-image>'
```

Set `KC_CONFIG` to the generated `config.env` path (a path, never a secret). The
shell function ensures every command uses the same project, overlay and settings:

```sh
export KC_CONFIG='<new-private-directory>/config.env'
export SHAIDE_SERVER_PORT=18081  # choose an unused port; do not restart another stack
kc_compose() {
  docker compose --env-file "$KC_CONFIG" -p shaide-kc-local \
    -f compose.yaml -f compose.kc.yaml --profile server "$@"
}
kc_compose config --quiet
kc_compose up -d --no-build --pull never
kc_compose ps
```

Use an unused **project name** for a new installation. Never reuse the name of a
running stack. The setup writes host UID/GID to `config.env` so nonroot KC, KC-MCP
and Control Panel can read their mounted mode-0600 files. The server and RustFS
run as root with all capabilities dropped except `DAC_OVERRIDE`, needed for these
operator-owned secret binds; they have no Docker socket or privileged mode.
Caddy retains only `NET_BIND_SERVICE`, which its executable file capability requires
even when listening on the internal port 8080.
The generated server admin password, JWT secret and Control Panel session secret
replace the base demo defaults only when the overlay is included. Container wrappers
read files into the component's required environment immediately before execution;
no secret is placed in Compose configuration, command arguments or helper output.
Do not enable shell tracing or publish process environments, secret files or logs.
Compose file-backed secrets are protected bind mounts, not an encrypted secret store.
The local Docker operator can inspect them and controls this trust boundary.

`SHAIDE_KC_QDRANT_IMAGE` and `SHAIDE_KC_NOTIFICATION_IMAGE` optionally override the
locally present Qdrant and Caddy images. `KC_SHAIDE_USERNAME` defaults to
`kc-embedding-service`; if changed in `config.env`, pass the same `--username` to
`operate.py`. Keep the generated keys and tokens stable across restarts. Losing the
JWT secret invalidates sessions; changing storage credentials requires coordinated
RustFS/KC configuration, and changing signing keys/tokens requires coordinating
all participating services. The helper deliberately has no automatic rotation.

The overlay removes host ports from Control Panel, webapp, generic vectordb and the
legacy optional Docker MCP gateway. Client ingress is **only**
`http://127.0.0.1:18081` (or your chosen server port); Control Panel is proxied by the
server at `/control-panel`. Add `--profile app` and `WEBAPP_URL=http://webapp:8787` to the function's
Compose command/environment to enable the existing `/app` webapp proxy.
The overlay runs `shaide` directly and replaces the base cloud credential mounts
with named database state; external HTTP providers work without host credential
mounts. Providers requiring cloud SDK/workload credentials need a private local
override with their documented runtime dependencies and read-only credential binds.
Never copy such credentials into this repository.

## Explicit service account and model setup

For a **new local instance**, after server login is available, create the dedicated
service account through the actual admin API. The helper loads the protected
`admin_password` and `embedding_password` files, logs in, calls `POST /v1/user`, and
prints neither the password nor bearer token. Choose a future RFC3339 expiry that
matches your policy. It does not reset an existing user on conflict. Skip creation
when the account already exists; verify its password/expiry instead.

```sh
python3 scripts/kc/operate.py \
  --credentials '<new-private-directory>' --server http://127.0.0.1:18081 \
  create-service-account --expiry '<future-RFC3339-expiry>'
```

For an existing account, configure `KC_SHAIDE_USERNAME` and replace the generated
embedding password **deliberately through your own secure credential workflow**
before start. The setup helper never touches existing credentials. No collection
memberships or administrator role are needed for the embedding service account.

Create an operator-owned mode-0600 JSON file outside source with the exact registered
provider URL, model name, known vector size and provider/schema identifiers:

```json
{
  "url": "<embedding-provider-endpoint>",
  "name": "<model-name>",
  "vector_size": 1024,
  "platform": "<supported-platform>",
  "api_schema": "open_ai",
  "daily_input_token_limit": 100000
}
```

1024 is illustrative; use the provider's verified dimension, never infer it.
The supported platform/schema and provider authentication must match your server
image. The JSON is read from a protected regular file, not passed as command text.
Do not put credentials in endpoint URLs. Registration calls the real
`POST /v1/embedding_model` API and is not run at startup:

```sh
python3 scripts/kc/operate.py \
  --credentials '<new-private-directory>' --server http://127.0.0.1:18081 \
  register-embedding --model-file '<private-model-json>'
python3 scripts/kc/operate.py \
  --credentials '<new-private-directory>' --server http://127.0.0.1:18081 \
  check --model '<model-name>' --vector-size '<verified-dimension>'
```

`check` verifies service login returns role=user, checks the catalog, and actually
embeds synthetic text through `POST /v1/embeddings`, including dimension checks.
It consumes a small amount of provider quota. It creates no account or model.
Account creation, model registration and this check were exercised with synthetic
disposable credentials and an embedding double through the real server; this does
not validate your provider or grant permission to register real users. No license
activation or private licensing workflow is automated here.

## Traffic, trust and notifications

| Path | Services and authorization |
| --- | --- |
| Upload | client → public server `/v1/object-storage/presigned-url`, then relative `/s3/...` PUT → authenticated KC internal storage facade → signed RustFS write |
| Notification | RustFS → fixed POST-only Caddy relay → server `/v1/object-storage/event`, authenticated by a separate webhook token |
| Index | server lifecycle orchestration → KC extraction → service-account login/embedding through server → private KC Qdrant |
| Retrieval | client → server `/v1/mcp/proxy/knowledge-center/mcp` → KC-MCP → KC → private KC Qdrant |

Server and RustFS share **no network**. The relay shares a separate network with
each, accepts only the exact notification POST path, and cannot proxy bucket,
object or arbitrary-target requests. RustFS shares no network with KC-MCP or the
generic server Qdrant. KC has separate API, storage, vector and MCP networks; only
the existing server/frontend/generic services join the ordinary network. All added
networks are internal. No optional backend ports are published.

Only KC and RustFS receive `storage_access_key` / `storage_secret_key`. KC creates
buckets, configures notifications, signs uploads, checks objects and deletes stored
objects/vectors through the frozen component APIs. The server's incoming
`S3_WEBHOOK_TOKEN` is notification authentication, not a storage credential.
RustFS notification target `SHAIDE` uses
`arn:rustfs:sqs:eu-central-1:shaide:webhook`, queue directory
`/data/notification-queue` and the fixed relay endpoint. KC provisions each bucket's
notification configuration when the server creates its collection; no root-key
bootstrap job performs storage operations on the server's behalf.

The signed context target id is `knowledge-center`, issuer `shaide-local`, key id
`kc-context-v1`; server signs with the private RSA key, KC-MCP/KC verify the public
key. Audiences are respectively `kc-mcp:knowledge-center` and `kc:knowledge-center`.
The server/KC-MCP gateway token and KC-MCP/KC scoped token are separate from the
server/KC lifecycle token, webhook token, JWT secret and embedding password. Local
HTTP is allowed explicitly only for this private Compose topology. Clients supply
only their shaide bearer login, not downstream service/admin credentials. Do not
send fabricated membership headers or add unsigned retrieval callbacks.
The existing legacy `--profile mcp` Docker gateway is independent and is not needed
for KC; KC uses the server's existing signed MCP gateway implementation.

## Liveness, readiness and recovery

`knowledge-center`'s container healthcheck calls its built-in `healthcheck`
(`GET /v1/health`). This is **liveness only**: it checks neither storage, vectors,
embedding account nor provider. KC-MCP waits for that liveness check, not full
readiness. Storage/vector startup uses `service_started` to avoid a dependency
cycle with account provisioning/server notifications. The server's `/v1/health`,
relay `/health` and Qdrant's `/readyz` are useful individual checks; none proves
that the complete product can process or retrieve documents. KC-MCP has no
standalone readiness endpoint in this component version. `ps` showing running or
healthy is insufficient.

After the embedding check, verify a disposable synthetic collection through the
public Control Panel/server: create a collection with the registered model and
memberships, request a five-minute relative upload URL, upload a small synthetic
PDF/DOCX through the public server, and wait for its file status to be `processed`.
Use the public signed MCP URL above to call `search_knowledge_center` and verify a
citation only for the caller's memberships. Verify file deletion through the public
API removes its object/vectors while preserving unrelated files. Do not expose
RustFS or KC to accomplish these steps. The operator helper does not auto-create
collections or upload fixtures.

Inspect component status and sanitized logs for missing images/secrets, account
expiry/password, model dimensions/provider quota, RustFS notification reachability
and object/vector failures. A dependency outage must fail explicitly, never be
reported as processed or a successful empty retrieval. If an operation is marked
uncertain/quarantined, a timeout does not prove its remote writes stopped. Follow
the component's explicit reconciliation procedure before retry/delete/model changes;
do not clear persisted lifecycle locks by age or edit database rows to force success.

## Preserve state on stop and restart

Keep the same project name, configuration and volume names:

```sh
kc_compose stop
kc_compose start
# Or remove only containers/networks, preserving all named volumes:
kc_compose down
kc_compose up -d --no-build --pull never
```

`kc-server-state` holds `/data/db/shaide-server.sqlite` and SQLite WAL state;
`kc-object-state` holds objects plus the notification queue;
`kc-vector-state` holds KC Qdrant data. KC has no metadata database. The generic
`vectordb` remains a separate service and URL; its preexisting persistence behavior
is not changed. Do not use `down --volumes`, prune volumes, delete credentials, or
switch project names to recover a failed startup. Before attaching preexisting
named volumes, inventory their exact names and compatibility and make a coordinated
backup. The overlay does not discover, rename, replace or restore another stack's
volumes, and does not import the old server database.

Integration tests used both external disk bind overrides and the actual product
named volumes, and verified retained metadata/object/vector state after down/up.
The named-volume run also verifies the product permission configuration without
bind-mount ownership allowances. This proves restart persistence on the
**same disk**, not backup/restore, disaster recovery, migration or release-image
validation. Back up server DB/WAL, objects/notification queue, vectors and protected
credentials consistently while stopped before any destructive maintenance. Restore
and migration are separate operator procedures and have not been validated here.

## Local verification

```sh
# Use a temp/cache directory outside source with sufficient capacity.
TMPDIR='<external-temp-directory>' python3 -m unittest discover -s test/kc-compose -v
# Ordinary startup still parses with no KC settings:
docker compose -f compose.yaml --profile server config --quiet
```

Tests create only disposable synthetic files and run Compose parsing; they do not
start or mutate a running stack. They cover exclusive creation, existing files and
symlinks, failure cleanup, secret-output absence, private network/port contracts,
signed trust configuration and persistent mount declarations. Runtime validation
uses a test-only override outside source with already verified native binaries and
local images, never a build or private binary/source in the public repository.
