//! Harness smoke tests.
//!
//! These assert the harness itself: that the real router boots, that both call modes work, that
//! the upstream is faked purely by what the database says, and that nothing leaks between tests.
//! The behaviour of the routes belongs in suites of their own.

mod harness;

use axum::http::{StatusCode, header};
use chrono::{Duration, Utc};
use shaide_common::api::{
    error::OpenAiErrorResponse,
    models::ListModelsResponse,
    users::{
        AccessTokenResponse, CreateUserRequest, CreateUserResponse, ListUsersResponse, LoginRequest,
    },
};

use crate::harness::{DEFAULT_MODEL, TestServer};

fn chat_request(model: &str, prompt: &str, stream: bool) -> serde_json::Value {
    serde_json::json!({
        "model": model,
        "stream": stream,
        "messages": [{ "role": "user", "content": prompt }],
    })
}

#[tokio::test]
async fn the_admin_lists_users_through_the_real_router() {
    let server = TestServer::start().await;
    let user = server.create_user().await;

    let response = server.admin().get("/v1/users").await.assert_ok();

    let ListUsersResponse { users } = response.json();
    let listed = users
        .iter()
        .find(|listed| listed.id == user.id)
        .expect("the created user should be listed");
    assert_eq!(listed.username, user.username);
    assert!(
        users.iter().any(|listed| listed.id == server.admin_id()),
        "the admin the harness ensured should be listed too"
    );
}

#[tokio::test]
async fn a_request_without_a_token_is_rejected_with_the_documented_error_body() {
    let server = TestServer::start().await;

    let response = server
        .anonymous()
        .get("/v1/users")
        .await
        .assert_status(StatusCode::UNAUTHORIZED);

    assert_eq!(
        response.header(header::CONTENT_TYPE).unwrap(),
        "application/json"
    );
    let body: OpenAiErrorResponse = response.json();
    assert_eq!(body.error.code.as_deref(), Some("authentication_failed"));
    assert_eq!(body.error.message, "Bearer token not set");
}

#[tokio::test]
async fn a_users_access_token_is_accepted_by_the_real_auth_middleware() {
    let server = TestServer::start().await;
    let user = server.create_user().await;

    let response = server.user(&user).get("/v1/models").await.assert_ok();

    let models: ListModelsResponse = response.json();
    assert!(
        models
            .models
            .iter()
            .any(|model| model.name == DEFAULT_MODEL),
        "the seeded model should be listed"
    );
}

#[tokio::test]
async fn a_users_access_token_does_not_open_an_admin_route() {
    let server = TestServer::start().await;
    let user = server.create_user().await;

    let response = server
        .user(&user)
        .get("/v1/users")
        .await
        .assert_status(StatusCode::UNAUTHORIZED);

    let body: OpenAiErrorResponse = response.json();
    assert_eq!(body.error.message, "Authenticated user is not an admin");
}

#[tokio::test]
async fn an_unknown_token_never_reaches_a_route() {
    let server = TestServer::start().await;

    server
        .with_token("not-a-real-token")
        .get("/v1/models")
        .await
        .assert_status(StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn a_harness_user_can_log_in_over_http() {
    let server = TestServer::start().await;
    let user = server.create_user().await;

    // The harness hands out ready-made tokens, but its users are real enough to authenticate the
    // long way round — which is what keeps the password hash it shares honest.
    let issued: AccessTokenResponse = server
        .anonymous()
        .post(
            "/v1/login",
            &LoginRequest {
                username: user.username.clone(),
                password: user.password.clone(),
            },
        )
        .await
        .assert_ok()
        .json();

    assert_eq!(issued.token_type, "Bearer");
    server
        .with_token(issued.access_token)
        .get("/v1/models")
        .await
        .assert_ok();
}

#[tokio::test]
async fn a_write_through_http_lands_in_the_database() {
    let server = TestServer::start().await;

    let created: CreateUserResponse = server
        .admin()
        .post(
            "/v1/user",
            &CreateUserRequest {
                username: "created-over-http".to_owned(),
                password: "created-over-http-password".to_owned(),
                expiry: Utc::now() + Duration::days(1),
            },
        )
        .await
        .assert_ok()
        .json();

    let stored = server
        .db()
        .get_user_by_username("created-over-http")
        .await
        .expect("the user should have been written to the database");
    assert_eq!(stored.id, created.id);
}

#[tokio::test]
async fn the_ephemeral_port_mode_streams_the_fake_upstreams_answer() {
    let server = TestServer::start().await;
    let user = server.create_user().await;
    server.upstream_says(&["Hello", ", world"]);

    let stream = server
        .live_user(&user)
        .await
        .sse(
            "/v1/chat/completions",
            &chat_request(DEFAULT_MODEL, "hi", true),
        )
        .await;
    assert_eq!(stream.status(), StatusCode::OK);
    assert_eq!(
        stream.content_type(),
        Some("text/event-stream"),
        "chat completions should be served as SSE"
    );

    let events = stream.read_events().await;
    let (last, chunks) = events.split_last().expect("the stream should not be empty");
    assert!(last.is_done(), "the stream should end with [DONE]");
    let content: String = chunks
        .iter()
        .filter_map(harness::SseEvent::content)
        .collect();
    assert_eq!(content, "Hello, world");

    // The upstream was reached only because a database row pointed the model at it.
    let upstream_request = server.upstream().only_request_to("/v1/chat/completions");
    assert_eq!(upstream_request.json()["model"], DEFAULT_MODEL);
}

#[tokio::test]
async fn an_unscripted_upstream_call_fails_loudly() {
    let server = TestServer::start().await;
    let user = server.create_user().await;

    // No `upstream_says`: the fake upstream answers 500, and the server classifies it as an
    // unavailable provider rather than pretending everything is fine.
    let response = server
        .user(&user)
        .post(
            "/v1/chat/completions",
            &chat_request(DEFAULT_MODEL, "hi", false),
        )
        .await
        .assert_status(StatusCode::SERVICE_UNAVAILABLE);

    let body: OpenAiErrorResponse = response.json();
    assert_eq!(body.error.code.as_deref(), Some("provider_unavailable"));
}

#[tokio::test]
async fn servers_share_no_state() {
    let first = TestServer::start().await;
    let second = TestServer::start().await;
    let user = first.create_user().await;

    let ListUsersResponse { users } = second.admin().get("/v1/users").await.assert_ok().json();

    assert!(
        !users.iter().any(|listed| listed.username == user.username),
        "a user created against one server must not exist in another, got {users:?}"
    );
}
