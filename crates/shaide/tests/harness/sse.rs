//! Building the SSE the fake upstream serves, and reading the SSE the server emits.
//!
//! Both halves are deliberately byte-level. A transcript is a `String` that goes onto the wire
//! verbatim, so a recorded upstream transcript (the gpt-oss harmony leak, a malformed delta) can
//! be replayed exactly as it was captured; and the server's stream is parsed from raw bytes rather
//! than through the server's own types, so a test asserts what a client would actually see.

use futures::StreamExt;
use serde_json::{Value, json};

/// An OpenAI-compatible `chat.completion.chunk`, with every field the server's deserializer looks
/// at spelled out.
pub fn chat_chunk(model: &str, delta: Value, finish_reason: Option<&str>) -> Value {
    json!({
        "id": "chatcmpl-integration-test",
        "object": "chat.completion.chunk",
        "created": 1_700_000_000_u32,
        "model": model,
        "choices": [{
            "index": 0,
            "delta": delta,
            "finish_reason": finish_reason,
            "logprobs": null,
        }],
        "usage": null,
    })
}

/// The final `usage` chunk: no choices, token counts for the whole request. This is the chunk the
/// server's usage accounting — and therefore daily-limit enforcement — keys off.
pub fn usage_chunk(model: &str, prompt_tokens: u32, completion_tokens: u32) -> Value {
    json!({
        "id": "chatcmpl-integration-test",
        "object": "chat.completion.chunk",
        "created": 1_700_000_000_u32,
        "model": model,
        "choices": [],
        "usage": {
            "prompt_tokens": prompt_tokens,
            "completion_tokens": completion_tokens,
            "total_tokens": prompt_tokens + completion_tokens,
        },
    })
}

/// Builds the `text/event-stream` body the fake upstream serves.
#[derive(Debug, Clone)]
pub struct SseTranscript {
    model: String,
    body: String,
}

impl SseTranscript {
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            body: String::new(),
        }
    }

    /// One `data:` event carrying `value`.
    pub fn data(mut self, value: &Value) -> Self {
        self.body.push_str(&format!("data: {value}\n\n"));
        self
    }

    /// Appends `raw` to the body untouched — for replaying a captured transcript, or for the
    /// deliberately malformed shapes the error tests need.
    pub fn raw(mut self, raw: impl AsRef<str>) -> Self {
        self.body.push_str(raw.as_ref());
        self
    }

    /// A content delta.
    pub fn content(self, text: &str) -> Self {
        let chunk = chat_chunk(&self.model, json!({ "content": text }), None);
        self.data(&chunk)
    }

    /// A reasoning delta (`reasoning_content`, which the server folds together with `reasoning`).
    pub fn reasoning(self, text: &str) -> Self {
        let chunk = chat_chunk(&self.model, json!({ "reasoning_content": text }), None);
        self.data(&chunk)
    }

    pub fn finish(self, reason: &str) -> Self {
        let chunk = chat_chunk(&self.model, json!({}), Some(reason));
        self.data(&chunk)
    }

    pub fn usage(self, prompt_tokens: u32, completion_tokens: u32) -> Self {
        let chunk = usage_chunk(&self.model, prompt_tokens, completion_tokens);
        self.data(&chunk)
    }

    /// The `[DONE]` terminator. Without it the server treats the stream as truncated.
    pub fn done(mut self) -> Self {
        self.body.push_str("data: [DONE]\n\n");
        self
    }

    pub fn build(self) -> String {
        self.body
    }

    /// Content deltas, a finish chunk, a usage chunk and `[DONE]` — the shape a well-behaved
    /// upstream produces.
    pub fn happy_path(model: &str, chunks: &[&str]) -> String {
        let mut transcript = Self::new(model);
        for chunk in chunks {
            transcript = transcript.content(chunk);
        }
        transcript.finish("stop").usage(11, 7).done().build()
    }
}

/// One event read off the stream the server emitted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SseEvent {
    pub data: String,
}

impl SseEvent {
    pub fn is_done(&self) -> bool {
        self.data == "[DONE]"
    }

    pub fn json(&self) -> Value {
        serde_json::from_str(&self.data)
            .unwrap_or_else(|error| panic!("SSE event should be JSON ({error}): {}", self.data))
    }

    /// The `choices[0].delta.content` of this event, when it has one.
    pub fn content(&self) -> Option<String> {
        self.json()
            .pointer("/choices/0/delta/content")?
            .as_str()
            .map(str::to_owned)
    }
}

/// Parses an SSE body into its events, dropping comments (keep-alives) and blank lines.
pub fn parse_events(body: &str) -> Vec<SseEvent> {
    body.split("\n\n")
        .filter_map(|block| {
            let data = block
                .lines()
                .filter_map(|line| line.strip_prefix("data:"))
                .map(str::trim_start)
                .collect::<Vec<_>>()
                .join("\n");
            (!data.is_empty()).then_some(SseEvent { data })
        })
        .collect()
}

/// The SSE response the server is streaming back, read incrementally off the socket.
pub struct SseStream {
    status: reqwest::StatusCode,
    content_type: Option<String>,
    body: reqwest::Response,
}

impl SseStream {
    pub(super) fn new(response: reqwest::Response) -> Self {
        Self {
            status: response.status(),
            content_type: response
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned),
            body: response,
        }
    }

    pub fn status(&self) -> reqwest::StatusCode {
        self.status
    }

    pub fn content_type(&self) -> Option<&str> {
        self.content_type.as_deref()
    }

    /// Reads the stream to its end and returns the raw bytes as they arrived — the assertion
    /// surface for "the server emitted exactly this".
    pub async fn read_to_string(self) -> String {
        let mut stream = self.body.bytes_stream();
        let mut body = String::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.expect("SSE stream should not fail mid-read");
            body.push_str(&String::from_utf8_lossy(&chunk));
        }
        body
    }

    /// Reads the stream to its end and parses it into events.
    pub async fn read_events(self) -> Vec<SseEvent> {
        parse_events(&self.read_to_string().await)
    }
}
