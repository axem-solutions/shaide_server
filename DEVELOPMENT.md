# Development

This document describes how to develop this project

## Releasing

Releases are handled by the AI agent. From within the products working directory:

```
/release shaide_server vX.Y.Z
```

or: "release shaide_server vX.Y.Z"

The agent generates a CHANGELOG entry from git history, asks for confirmation, then runs `scripts/release.ts` which bumps `[workspace.package]` version in `Cargo.toml`, commits, and pushes the tag.

GitHub Actions (`publish.yml`) then builds and pushes Docker images:
- `ghcr.io/axem-solutions/shaide_server:vX.Y.Z`
- `ghcr.io/axem-solutions/shaide_server:latest`

A GitHub Release and a Jira Release (`shaide_server vX.Y.Z`) are also created automatically.

**Running the script directly:**
```bash
npx tsx scripts/release.ts vX.Y.Z [changelog-file] [--dry-run]
```
