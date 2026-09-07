//! The faked upstream model provider.
//!
//! This is the only network boundary the PR-level integration tests cross, and they cross it to
//! localhost. It is wired in **by seeding the database**: a model row's
//! `chat_completions_endpoint` (or an embedding model's `url`) points at this server, so no server
//! code knows it is talking to a fake.
//!
//! Responses are scripted per path — [`FakeUpstream::push`] queues one response, consumed in
//! order, and [`FakeUpstream::always`] sets the response served once the queue is empty. Every
//! request is recorded, so a test can assert the *request* the server built as well as the
//! response it produced.
//!
//! Transcripts are built in code today. Because [`UpstreamResponse::Sse`] serves arbitrary bytes,
//! a transcript recorded from a real provider replays through the same scripting API unchanged.

use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{Arc, Mutex},
};

use axum::{
    Router,
    body::{Body, Bytes},
    extract::{Request, State},
    http::{HeaderMap, Method, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde_json::Value;
use tokio::{net::TcpListener, task::JoinHandle};

/// A request the server sent to the fake upstream.
#[derive(Debug, Clone)]
pub struct RecordedRequest {
    pub method: Method,
    pub path: String,
    pub headers: HeaderMap,
    pub body: Bytes,
}

impl RecordedRequest {
    /// The request body parsed as JSON. Panics with the raw body when it is not JSON, which is
    /// what a test wants to see.
    pub fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or_else(|error| {
            panic!(
                "upstream request body should be JSON ({error}): {}",
                String::from_utf8_lossy(&self.body)
            )
        })
    }
}

/// What the fake upstream answers with.
#[derive(Debug, Clone)]
pub enum UpstreamResponse {
    /// A `text/event-stream` body, served verbatim. This is how recorded transcripts are replayed:
    /// whatever bytes go in here are exactly what the server's provider client reads.
    Sse(String),
    /// A JSON body with status 200.
    Json(Value),
    /// Any status with a body of its own — the error shapes (429, 500, malformed) live here.
    Status {
        status: StatusCode,
        content_type: &'static str,
        body: String,
    },
}

impl UpstreamResponse {
    pub fn sse(body: impl Into<String>) -> Self {
        Self::Sse(body.into())
    }

    pub fn json(body: Value) -> Self {
        Self::Json(body)
    }

    pub fn error(status: StatusCode, body: impl Into<String>) -> Self {
        Self::Status {
            status,
            content_type: "application/json",
            body: body.into(),
        }
    }

    fn into_response(self) -> Response {
        match self {
            Self::Sse(body) => (
                StatusCode::OK,
                [(header::CONTENT_TYPE, "text/event-stream")],
                body,
            )
                .into_response(),
            Self::Json(body) => (StatusCode::OK, axum::Json(body)).into_response(),
            Self::Status {
                status,
                content_type,
                body,
            } => (status, [(header::CONTENT_TYPE, content_type)], body).into_response(),
        }
    }
}

#[derive(Default)]
struct Scripts {
    queued: HashMap<String, Vec<UpstreamResponse>>,
    fallback: HashMap<String, UpstreamResponse>,
}

#[derive(Default)]
struct UpstreamState {
    scripts: Mutex<Scripts>,
    requests: Mutex<Vec<RecordedRequest>>,
}

/// A fake OpenAI-compatible upstream listening on an ephemeral port.
pub struct FakeUpstream {
    address: SocketAddr,
    state: Arc<UpstreamState>,
    server: JoinHandle<()>,
}

impl FakeUpstream {
    pub async fn start() -> Self {
        let state = Arc::new(UpstreamState::default());
        let router = Router::new()
            .fallback(handle)
            .with_state(Arc::clone(&state));

        let listener = TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("fake upstream should bind an ephemeral port");
        let address = listener
            .local_addr()
            .expect("bound listener should report its address");
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, router).await;
        });

        Self {
            address,
            state,
            server,
        }
    }

    pub fn url(&self, path: &str) -> String {
        format!("http://{}{path}", self.address)
    }

    pub fn chat_completions_url(&self) -> String {
        self.url("/v1/chat/completions")
    }

    pub fn embeddings_url(&self) -> String {
        self.url("/v1/embeddings")
    }

    /// Queues one response for `path`. Queued responses are served in the order they were pushed,
    /// one per request.
    pub fn push(&self, path: &str, response: UpstreamResponse) {
        self.scripts()
            .queued
            .entry(path.to_owned())
            .or_default()
            .push(response);
    }

    /// Sets the response served for `path` whenever the queue for it is empty.
    pub fn always(&self, path: &str, response: UpstreamResponse) {
        self.scripts().fallback.insert(path.to_owned(), response);
    }

    /// Every request the server has sent so far, in order.
    pub fn requests(&self) -> Vec<RecordedRequest> {
        self.state
            .requests
            .lock()
            .expect("upstream request log should not be poisoned")
            .clone()
    }

    pub fn requests_to(&self, path: &str) -> Vec<RecordedRequest> {
        self.requests()
            .into_iter()
            .filter(|request| request.path == path)
            .collect()
    }

    /// The single request sent to `path`. Panics when there was not exactly one, which is almost
    /// always the assertion a test actually meant to make.
    pub fn only_request_to(&self, path: &str) -> RecordedRequest {
        let mut requests = self.requests_to(path);
        assert_eq!(
            requests.len(),
            1,
            "expected exactly one upstream request to {path}, got {}",
            requests.len()
        );
        requests.remove(0)
    }

    fn scripts(&self) -> std::sync::MutexGuard<'_, Scripts> {
        self.state
            .scripts
            .lock()
            .expect("upstream scripts should not be poisoned")
    }
}

impl Drop for FakeUpstream {
    fn drop(&mut self) {
        self.server.abort();
    }
}

async fn handle(State(state): State<Arc<UpstreamState>>, request: Request) -> Response {
    let (parts, body) = request.into_parts();
    let body = axum::body::to_bytes(body, usize::MAX)
        .await
        .unwrap_or_default();
    let path = parts.uri.path().to_owned();

    state
        .requests
        .lock()
        .expect("upstream request log should not be poisoned")
        .push(RecordedRequest {
            method: parts.method,
            path: path.clone(),
            headers: parts.headers,
            body,
        });

    let response = {
        let mut scripts = state
            .scripts
            .lock()
            .expect("upstream scripts should not be poisoned");
        let queued = scripts
            .queued
            .get_mut(&path)
            .filter(|queued| !queued.is_empty())
            .map(|queued| queued.remove(0));
        queued.or_else(|| scripts.fallback.get(&path).cloned())
    };

    match response {
        Some(response) => response.into_response(),
        // Loud on purpose: an unscripted call is a test that forgot to say what the model does.
        None => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Body::from(format!(
                "fake upstream has no scripted response for {path}; \
                 script one with FakeUpstream::push or ::always"
            )),
        )
            .into_response(),
    }
}
