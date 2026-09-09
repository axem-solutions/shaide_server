//! Streamed chat happy path.
//!
//! `POST /v1/chat/completions` with `stream: true`, against the fake upstream's happy-path
//! transcript: the SSE the server emits must be well-formed and `[DONE]`-terminated, content
//! chunks must arrive in the order the upstream sent them, the final usage chunk must be present,
//! and the accounting a daily limit is later checked against must land in the database.

mod harness;

use axum::http::StatusCode;

use crate::harness::{DEFAULT_MODEL, TestServer};

fn chat_request(model: &str, prompt: &str, stream: bool) -> serde_json::Value {
    serde_json::json!({
        "model": model,
        "stream": stream,
        "messages": [{ "role": "user", "content": prompt }],
    })
}

#[tokio::test]
async fn a_streamed_chat_completion_streams_content_in_order_and_records_usage() {
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

    let content_in_order: Vec<String> = chunks
        .iter()
        .filter_map(harness::SseEvent::content)
        .collect();
    assert_eq!(
        content_in_order,
        vec!["Hello".to_owned(), ", world".to_owned()],
        "content chunks should arrive in the order the upstream sent them"
    );

    let usage_event = chunks
        .iter()
        .find(|event| !event.json()["usage"].is_null())
        .expect("a final usage chunk should be present in the stream");
    let usage = &usage_event.json()["usage"];
    assert_eq!(usage["prompt_tokens"], 11);
    assert_eq!(usage["completion_tokens"], 7);

    // The usage row daily-limit enforcement reads (`DbConn::get_daily_usage`) must reflect this
    // request, not just the SSE the client saw.
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let daily_usage = server
        .db()
        .get_user_daily_usages(&today, user.id)
        .await
        .expect("daily usage should be queryable");
    let model_usage = daily_usage
        .iter()
        .find(|usage| usage.model_name == DEFAULT_MODEL)
        .unwrap_or_else(|| panic!("no daily usage row for {DEFAULT_MODEL}, got {daily_usage:?}"));
    assert_eq!(model_usage.total_input_token_count, 11);
    assert_eq!(model_usage.total_output_token_count, 7);

    // The upstream was reached only because a database row pointed the model at it.
    let upstream_request = server.upstream().only_request_to("/v1/chat/completions");
    assert_eq!(upstream_request.json()["model"], DEFAULT_MODEL);
    assert_eq!(upstream_request.json()["stream"], true);
}
