# Using SQLx with local Turso

`shaide-db` uses `sqlx-turso = 0.1.0-alpha.1`, which wraps the Rust Turso engine
and SQLx 0.9. It does not use libSQL, remote connections, or cloud sync.
The adapter is experimental and currently pins the `turso` SDK to
`0.7.0-pre.3`; the resolved engine versions are recorded in `Cargo.lock`.

## Queries and offline builds

Import checked query macros from `sqlx_turso`, not `sqlx`. Keep using `sqlx`
for shared traits, errors, and `migrate!`. The adapter's describe metadata
currently reports unknown parameter types and conservative column nullability.
Use explicit column aliases such as `id as "id!: i64"` and
`platform as "platform?: String"` to match the schema and DAO types. In
particular, SQL INTEGER columns must decode as i64 to preserve large values.
Use `INSERT ... RETURNING id` for inserted IDs.

The committed `.sqlx/query-*.json` files contain Turso metadata. Build and test
without opening a schema database:

```sh
SQLX_OFFLINE=true cargo build
SQLX_OFFLINE=true cargo test -p shaide-db
```

## Migrations and metadata preparation

Install the metadata helper and migration-file generator:

```sh
cargo install sqlx-turso-cli --version 0.1.0-alpha.1 --locked
cargo install sqlx-cli --version 0.9.0 --locked
```

Stop the server, then create and apply a migration and regenerate metadata:

```sh
just db-new migration_name
just db-migrate
just db-prepare
```

`db-migrate` uses the separate `shaide-db-migrate` tool to create or migrate
the local server database through Turso. No separate schema database is needed. This tool builds independently of the application query macros,
so it can apply schema changes before query metadata is regenerated. `db-prepare` also applies pending
migrations, then uses `sqlx-turso prepare` to check the library and regenerate
`.sqlx`. The recipe first cleans the database crate's build artifacts because
this alpha helper otherwise skips cached macro expansion and can produce an
empty metadata directory. Commit the migration, query changes, and updated metadata together.
Do not use stock `cargo sqlx prepare` or `cargo sqlx migrate run` with Turso URLs.

Preparation checks only the database library: simultaneous online compilation
of library and test targets can contend for Turso's schema file lock. Run
workspace and test checks offline afterwards (`just check`). If macro queries
are added to other targets, include them in preparation with `--jobs 1`.

To revert the latest migration on the local server database:

```sh
just db-revert
```

The migration and preparation recipes accept an explicit database file path. Server startup
runs the same embedded migrations automatically.

## Local files and the experiment

The server uses `shaide-server.turso` under its usual database directory. This
is a fresh experimental database; existing SQLite data is not automatically
migrated. Turso rejects the old schema's `REFERENCES ... NOT NULL` ordering;
the baseline now puts `NOT NULL` before `REFERENCES` with identical semantics.
Consequently, the baseline checksum also differs from the SQLite baseline.
Do not rename or directly reuse the old database file.

Install the `tursodb` CLI from the [Turso repository](https://github.com/tursodatabase/turso)
to inspect local databases with `just db-shell`. Close the server first: this
integration does not enable experimental multi-process access or MVCC.
