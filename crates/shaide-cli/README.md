# shaide-cli

Authenticated commands log in as the built-in `admin` user and use the returned
JWT for the request.

## Usage

[See the dedicated page.](docs/CommandLineHelp.md)

## Releasing

Releases are handled by the AI agent. From within the products working
directory:

```
/release shaide_cli_client vX.Y.Z
```

or: "release shaide_cli_client vX.Y.Z"

The agent generates a CHANGELOG entry, runs `scripts/release.ts` to bump
`Cargo.toml`, commit, and push the tag. GitHub Actions then cross-compiles Linux
binaries (`amd64` + `arm64`) and creates the GitHub Release with the binaries
attached. A Jira Release (`shaide_cli_client vX.Y.Z`) is also created.

**Running the script directly:**

```bash
npx tsx scripts/release.ts vX.Y.Z [changelog-file] [--dry-run]
```

> **Note:** CI requires an SSH deploy key for the `shaide-common` git dependency
> before builds will succeed.

# Insert model

```sh
cargo run create-model \
  --name openai/gpt-oss-120b-maas \
  --variant openai/gpt-oss-120b-maas \
  --chat-completions-endpoint https://aiplatform.googleapis.com/v1/projects/your-gcp-project-id/locations/global/endpoints/openapi/chat/completions \
  --api-schema open_ai \
  --supports-reasoning-effort true \
  --supports-images false \
  --max-generated-tokens 32000 \
  --context-size 32000 \
  --platform vertex \
  --remote <REMOTE> \
  --admin-password <ADMIN_PASSWORD>
```

# Delete model

```sh
cargo run delete-model \
  --id 1 \
  --remote <REMOTE> \
  --admin-password <ADMIN_PASSWORD>
```

# Insert embedding model

```bash
cargo run create-embedding-model \
--name "example-embeddings-v1" \
--url "https://api.example.com/v1/embeddings" \
--vector-size 768 \
--platform "example-ai" \
--api-schema "openai" \
--remote <REMOTE> \
--admin-password <ADMIN_PASSWORD>
```

# Adding new users

Add a single user with a provided username:

```sh
cargo run add-user \
  example-username \
  example-user-password \
  --expiry 2026-12-31T23:59:59Z \
  --remote <REMOTE> \
  --admin-password <ADMIN_PASSWORD>
```

Add multiple generated users:

```sh
cargo run add-users \
  10 \
  --remote <REMOTE> \
  --admin-password <ADMIN_PASSWORD>
```

# Update command line help

Eventually this should be automated. But for now:

```sh
cargo run markdown-help > docs/CommandLineHelp.md
```
