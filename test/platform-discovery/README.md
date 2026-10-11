# Local platform discovery demo (SHD-1078 / SHD-924)

This isolated Compose project runs the real shaide server discovery API and
Control Panel against a fake Kubernetes API and vLLM metadata servers. It uses
in-cluster TLS, a fake service-account token, the platform's Service labels, and
Docker aliases matching Kubernetes service DNS names. No production discovery
switch or Kubernetes cluster is required.

The fixtures are **discovery-only**: they cannot generate text or embeddings.
The mock rejects inference requests. Discovery only requests `/api/v1/services`,
`/v1/shaide/metadata`, and `/v1/models`; it never probes inference.
Missing metadata is exposed as an error and the affected model is excluded.

## Start

Run from this directory. Docker with Compose v2 is required. The Control Panel
source must contain the `/v1/platform-models` integration. By default Compose
uses the sibling `control-panel-platform-discovery` worktree; set
`CONTROL_PANEL_SOURCE` to an absolute checkout path to override it.

```sh
docker compose up -d --build
```

This builds both applications with their regular Dockerfiles. For a faster
Linux development build, the optional helper packages native Rust and Next.js
standalone artifacts into Debian runtime images with demo-specific tags:

```sh
./build-local-images.sh
docker compose up -d --no-build
```

The helper requires Rust, Node/npm and Docker. `CARGO_TARGET_DIR` can point to
an existing native Rust build cache. `--reuse-builds` skips compilation and
packages the existing artifacts; use it only after building the current source.
This native-artifact path is distinct from testing the release Dockerfiles.

Open **http://127.0.0.1:13000/control-panel/models** and log in with
**admin / admin**. Use the AI Platform model selector to add `demo-chat` and
`demo-embed`. They begin available but unregistered. Backend API:
http://127.0.0.1:18080/v1/platform-models (admin authentication required).

The project is `shaide-discovery-demo`. Ports bind only to loopback and do not
conflict with the usual 3000/8080 stack. Its database, vector store, network,
and fake service-account volume are separate. No cloud credentials or existing
databases are mounted. Prefer the `127.0.0.1` URL if your other Control Panel
uses `localhost`, since browser cookies are shared across ports on the same host.

## Change the available models

Edit `fixtures/scenario.json` to select one of these scenarios:

| Scenario | Available models | Error |
| --- | --- | --- |
| `healthy` | demo-chat, demo-embed | None |
| `missing-metadata` | demo-embed | Chat runtime metadata unavailable |
| `missing-dimensions` | demo-chat | Embedding output dimensions unavailable |
| `duplicates` | demo-embed | Two chat services claim the same served name |
| `empty` | None | None |

For example:

```sh
printf '%s\n' '{"scenario":"missing-metadata"}' > fixtures/scenario.json
docker compose restart shaide_server
```

Then click **Refresh available models**. Restarting clears the server's
30-second discovery cache; alternatively wait for that cache to expire before
refreshing. A missing catalog entry does not remove an already registered
model. Remove installed demo models separately when testing availability.
The fixture definitions and network aliases are in `mock.py` and `compose.yaml`.

## Validate and manage

```sh
python3 smoke.py
docker compose ps
docker compose logs --tail=40 platform-mock shaide_server control-panel
docker compose down
```

The smoke script uses only this demo's loopback ports. It checks anonymous and
ordinary-user rejection, admin login, the rendered models page, adding and
removing both model kinds through Control Panel routes, all five scenarios,
and absence of inference requests in mock logs. Remove any installed
`demo-chat` / `demo-embed` models before running it. It restores `healthy`,
removes the models it added, and leaves an ordinary test user in the isolated
database with a one-day expiry. `down` preserves data for the next start.

The mock's self-signed certificate is regenerated at mock startup. If you
restart or recreate `platform-mock` alone, also restart `shaide_server` so it
reloads the CA. None of these commands affect the existing Compose project.
