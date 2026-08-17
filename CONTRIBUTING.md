# Contributing to shaide Server

Contributions of code, documentation, bug reports, and feature ideas are welcome.

By participating, you agree to follow the axem
[Code of Conduct](https://github.com/axem-solutions/.github/blob/main/CODE_OF_CONDUCT.md).

## Before you start

- Read and understand the documentation.
- Search the [existing issues](https://github.com/axem-solutions/shaide_server/issues)
  before opening a new one.
- Do not open a public issue for a suspected security vulnerability. Report it
  privately to [info@axem.dev](mailto:info@axem.dev).

Questions and proposals are also welcome in
[Discussions](https://github.com/axem-solutions/shaide_server/discussions) or on
[Discord](https://discord.com/invite/Nv6hSzXruK).

## Development setup

shaide Server is a Rust workspace containing the server, common libraries,
database layer, and CLI.

You will need:

- a Rust toolchain with `rustfmt` and `clippy`
- Docker with Compose
- optionally, [`just`](https://github.com/casey/just) and `sqlx-cli` for the
  repository's development and database recipes.

Fork the repository, clone your fork, and create a focused branch from `main`:

```bash
git clone https://github.com/<your-user>/shaide_server.git
cd shaide_server
git switch -c <issue-id>/short-description
```

Configure the environment variables described in `README.md`. To start the
local dependencies and server with the repository recipes, run:

```bash
just dev
```

Alternatively, start only the backing services and run the server directly:

```bash
docker compose up -d caddy vectordb s3
cargo run
```

Or, run every service in a container with Docker Compose:

```bash
docker compose up
```

Stop the backing services with `docker compose down` or `just services-down`.

## Making changes

- Keep changes focused and avoid unrelated refactors.
- Add or update tests for changed behavior.
- Update user-facing documentation and API documentation when behavior changes.
- Never commit credentials, license files, local databases, or `.env` files.

Use [Conventional Commits](https://www.conventionalcommits.org/) for commit
messages, for example:

```text
feat: add trial deployment status endpoint
fix(db): preserve existing model records during migration
docs: clarify local development setup
```

### Database changes

Create database migrations with `sqlx` rather than editing an existing
migration:

```bash
sqlx migrate add --source crates/shaide-db/migrations -r migration_name
```

Migrations must preserve existing user data and include both up and down paths.
After changing a migration or a checked SQL query, apply the migration and
refresh SQLx's offline metadata:

```bash
just db-migrate
just db-prepare
```

## Validate your change

Run the same checks used by pull-request CI from the repository root:

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo build --verbose --all-features
cargo test --verbose --all-features
```

If your change affects container behavior or service integration, also build
the image or exercise the relevant Docker Compose flow. Documentation changes
can be previewed with `mdbook serve` from the `documentation` directory.

## Open a pull request

Push your branch and open a pull request against `main`, or the integration
branch described in the issue.
Complete the pull request template and:

- explain what changed and why
- describe the tests you ran and their results
- call out migrations, configuration changes, API compatibility, and other
  deployment effects
- include documentation updates in the same pull request.

Keep the pull request reviewable, respond to feedback, and ensure all required
checks pass. A maintainer will merge the pull request after approval.
