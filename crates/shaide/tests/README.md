# Server integration tests

HTTP/DB integration tests for `shaide_server`. They boot the **real** `api_router()` in-process
against a **fresh temporary SQLite database per test**, and fake only what sits at a process
boundary — today the upstream model provider. Router, middleware, extractors, serde and the
database are all real.

Everything here is hermetic: no docker, no cloud credentials, no network beyond localhost. The
suite runs on every PR as part of `cargo test`.

## Layout

| Path | What |
|---|---|
| `harness/mod.rs` | `TestServer` — boot, clients, users, models |
| `harness/fake_upstream.rs` | the faked upstream model provider |
| `harness/sse.rs` | building upstream transcripts, reading the server's SSE |
| `harness/env.rs` | the process-global setup, done once per test binary |
| `smoke.rs` | the harness's own tests |

Add a new test as its own file next to `smoke.rs` (`mod harness;` at the top). Each file is a
separate test binary, so it gets its own process-global state.

## Writing a test

```rust
mod harness;

use crate::harness::{DEFAULT_MODEL, TestServer};

#[tokio::test]
async fn a_created_user_is_listed() {
    let server = TestServer::start().await;
    let user = server.create_user().await;

    let response = server.admin().get("/v1/users").await.assert_ok();

    let users: ListUsersResponse = response.json();
    assert!(users.users.iter().any(|listed| listed.id == user.id));
}
```

`TestServer::start()` gives you a migrated database holding nothing but the admin user, a fake
upstream, and one model — [`DEFAULT_MODEL`] — already pointed at it.
`TestServer::without_default_model()` skips the model when a test defines its own.

### Two call modes

**Request/response** — `tower::ServiceExt::oneshot`, no socket involved. This is the default; use
it for everything that is not streamed.

```rust
server.admin().get("/v1/users").await;
server.user(&user).post("/v1/chat/completions", &body).await;
server.anonymous().get("/v1/users").await;          // no Authorization header
server.with_token("bogus").get("/v1/models").await; // negative auth cases
```

Responses come back as `TestResponse`: `.assert_ok()`, `.assert_status(...)`, `.json::<T>()`,
`.json_value()`, `.text()`, `.header(...)`. Prefer `.json::<T>()` with the type from
`shaide-common` — then a shape change breaks the test here rather than silently at a client.

**Ephemeral-port serve** — a real listener and a real HTTP client. Needed whenever the response is
streamed, because `oneshot` only hands back a whole response.

```rust
let stream = server
    .live_user(&user).await
    .sse("/v1/chat/completions", &body)
    .await;

assert_eq!(stream.status(), StatusCode::OK);
let events = stream.read_events().await;      // parsed events
// or stream.read_to_string().await           // the exact bytes on the wire
```

The listener starts on first use and stops with the `TestServer`.

### Users and auth

Authentication is a JWT access token, and the admin is a `users` row with the admin role — the one
`ensure_admin` writes on a real boot. Every server has one, and `server.admin()` is already
authenticated as it.

```rust
let user = server.create_user().await;                     // unique username, token included
let other = server.create_user_with_username("alice").await;
let expired = server.create_user_with_expiry("bob", Utc::now() - Duration::days(1)).await;

server.admin();                  // admin-authenticated client
server.user(&user);              // user-authenticated client
server.with_token("garbage");    // negative cases
server.access_token(user.id);    // mint a token by hand
```

`create_user` hands back a ready-made token, so most tests never have to log in. The users are
real all the same: they carry [`TEST_PASSWORD`] and can go the long way round through
`POST /v1/login`.

Argon2 is deliberately expensive and a server is booted per test, so the harness hashes that one
shared password once per test binary and reuses the hash for every user, the admin included. The
database ends up in the same state `ensure_admin` produces; only the cost is avoided.

One caveat on isolation: access tokens are signed with a process-wide secret and carry a user *id*,
so a token minted against one `TestServer` would also validate against another if the same id
exists there. Databases are still per-test — assert on `server.db()` rather than on a token
crossing servers.

### Faking the upstream

The upstream is faked **entirely through the database**. Per-model endpoints live in the `models`
table, so the harness seeds `chat_completions_endpoint` with the address of a local stub; no server
code knows it is under test.

```rust
server.upstream_says(&["Hello", ", world"]);   // a well-behaved streamed answer

server.upstream_chat(UpstreamResponse::sse(
    SseTranscript::new(DEFAULT_MODEL)
        .reasoning("thinking")
        .content("answer")
        .finish("stop")
        .usage(11, 7)
        .done()
        .build(),
));

server.upstream_chat(UpstreamResponse::error(StatusCode::TOO_MANY_REQUESTS, r#"{"error":"slow down"}"#));
```

`SseTranscript::raw` appends bytes untouched — that is how a captured transcript (the gpt-oss
harmony leak, a malformed delta) is replayed exactly as recorded.

Responses are scripted per path: `push` queues one, `always` sets what is served once the queue is
empty. Any other path works the same way, so an embedding stub is `server.upstream().push(
"/v1/embeddings", ...)`. An unscripted call answers 500 on purpose — a test that forgot to say what
the model does should fail, not drift.

Requests are recorded, so the request the server *built* is assertable too:

```rust
let request = server.upstream().only_request_to("/v1/chat/completions");
assert_eq!(request.json()["model"], DEFAULT_MODEL);
```

### Models and limits

```rust
let mut model = server.upstream_model("vision-model");   // pre-pointed at the fake upstream
model.supports_images = true;
model.max_images_per_request = Some(2);
server.seed_model(model).await;

server.set_model_limits(DEFAULT_MODEL, Some(100), Some(50)).await;
```

### Asserting the database

`server.db()` is the same `DbConn` the router uses — for seeding rows no route can create, and for
asserting what a request actually wrote (usage rows, memberships).

## What the harness mounts, and what it leaves out

It mounts `api_router()` plus the exact outermost chain the deployed server applies
(`shaide::with_api_middleware`: the 404 fallback, request logging, header forwarding and the
error-response mapping) and permissive CORS. Error bodies are therefore shaped by the same
middleware production uses, which is what makes an assertion on an error body meaningful.

It leaves out two pieces of `start_server`, both deliberately:

- **the `/control-panel` reverse proxy** — it forwards to another process, so that edge belongs to
  the full-stack end-to-end suite rather than here
- **the Prometheus layer and `/metrics`** — `PrometheusMetricLayer::pair()` installs a *global*
  recorder, so a second one in the same process would fail

## Gotchas

- **`cargo test` needs a `DATABASE_URL`.** There is no checked-in `.sqlx` offline data, so the
  sqlx macros are checked against a live database at compile time. Point `DATABASE_URL` at a
  SQLite file with the migrations applied (`just db-migrate`); it is only read while compiling —
  the tests themselves each build their own temporary database.
- **The environment is set once per test binary**, in `harness::env`, before anything reads it.
  Anything that must be configured through the environment belongs there, not in a test.
- **`shaide_ROOT` is redirected** to a directory under `target/tmp`, so tests never read or write
  the database or logs of the machine running them.
- **Test bodies must not assume a fixed admin token or id** — read them from
  `server.admin_token()` and `server.admin_id()`.

## Fixtures

Transcripts are built in code today, which keeps them readable next to the assertion they serve.
`SseTranscript::raw` and `UpstreamResponse::sse` take arbitrary bytes, so a transcript recorded
from a real provider replays through the same scripting API without any change to the harness.
