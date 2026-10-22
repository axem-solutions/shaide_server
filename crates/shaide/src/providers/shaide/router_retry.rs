//! Retries requests the llm-d router rejected.
//!
//! Platform models are served behind the llm-d router (its endpoint picker,
//! EPP). The router can answer a request itself, for example with 503 while no
//! model server pod is ready, and then names why in the
//! `x-llm-d-request-dropped-reason` header. A reason starting with `rejected-`
//! means the request never reached a model server, so sending it again is
//! safe and cheap. `evicted` means the request was dispatched and GPU time was
//! spent, so it is not retried here.

use std::time::Duration;

use reqwest::{RequestBuilder, Response, header::HeaderMap};
use tracing::debug;

/// Header the llm-d router sets on the responses it generates itself.
pub const DROPPED_REASON_HEADER: &str = "x-llm-d-request-dropped-reason";

/// Prefix of the reasons for which no inference work was done.
const REJECTED_PREFIX: &str = "rejected-";

/// Total number of attempts, including the first one.
pub const MAX_ATTEMPTS: u32 = 3;

/// Delay before the first retry; it doubles with every further retry.
const BASE_DELAY: Duration = Duration::from_millis(500);

/// Whether the router rejected the request before any model server saw it.
pub fn is_router_rejection(headers: &HeaderMap) -> bool {
    headers
        .get(DROPPED_REASON_HEADER)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|reason| reason.starts_with(REJECTED_PREFIX))
}

/// The delay before retry number `retry`, counting from zero.
pub fn retry_delay(retry: u32) -> Duration {
    BASE_DELAY * 2u32.saturating_pow(retry)
}

/// Whether a response to attempt number `attempt` (counting from zero) should
/// be retried.
pub fn should_retry(attempt: u32, response: &Response) -> bool {
    !response.status().is_success()
        && is_router_rejection(response.headers())
        && attempt + 1 < MAX_ATTEMPTS
}

/// Sends the request `build` creates, again while the router rejects it. The
/// last response is returned whatever its status, for the caller to handle.
pub async fn send(build: impl Fn() -> RequestBuilder) -> Result<Response, reqwest::Error> {
    let mut attempt = 0;
    loop {
        let response = build().send().await?;
        if !should_retry(attempt, &response) {
            return Ok(response);
        }

        let delay = retry_delay(attempt);
        debug!(
            status_code = %response.status(),
            reason = ?response.headers().get(DROPPED_REASON_HEADER),
            attempt = attempt + 1,
            delay_ms = delay.as_millis(),
            "The llm-d router rejected the request; retrying"
        );
        tokio::time::sleep(delay).await;
        attempt += 1;
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
    };

    use axum::{Router, http::StatusCode, response::IntoResponse, routing::post};
    use reqwest::header::HeaderValue;

    use super::*;

    /// Serves `rejections` router rejections with `reason`, then 200, and
    /// counts the requests it received.
    async fn upstream(
        rejections: u32,
        status: StatusCode,
        reason: &'static str,
    ) -> (String, Arc<AtomicU32>) {
        let requests = Arc::new(AtomicU32::new(0));
        let counter = requests.clone();
        let app = Router::new().route(
            "/v1/embeddings",
            post(move || {
                let counter = counter.clone();
                async move {
                    let seen = counter.fetch_add(1, Ordering::SeqCst);
                    if seen < rejections {
                        (status, [(DROPPED_REASON_HEADER, reason)], "dropped").into_response()
                    } else {
                        (StatusCode::OK, "ok").into_response()
                    }
                }
            }),
        );

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        (format!("http://{address}/v1/embeddings"), requests)
    }

    #[test]
    fn only_rejected_reasons_are_router_rejections() {
        for (reason, expected) in [
            ("rejected-no-endpoints", true),
            ("rejected-saturated", true),
            ("rejected-shutting-down", true),
            ("evicted", false),
        ] {
            let mut headers = HeaderMap::new();
            headers.insert(DROPPED_REASON_HEADER, HeaderValue::from_static(reason));
            assert_eq!(is_router_rejection(&headers), expected, "{reason}");
        }
        assert!(!is_router_rejection(&HeaderMap::new()));
    }

    #[test]
    fn the_delay_doubles() {
        assert_eq!(retry_delay(0), Duration::from_millis(500));
        assert_eq!(retry_delay(1), Duration::from_secs(1));
    }

    #[tokio::test]
    async fn a_rejected_request_is_sent_again() {
        let (url, requests) =
            upstream(1, StatusCode::SERVICE_UNAVAILABLE, "rejected-no-endpoints").await;
        let client = reqwest::Client::new();

        let response = send(|| client.post(&url)).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(requests.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn retries_stop_after_the_last_attempt() {
        let (url, requests) = upstream(
            u32::MAX,
            StatusCode::TOO_MANY_REQUESTS,
            "rejected-saturated",
        )
        .await;
        let client = reqwest::Client::new();

        let response = send(|| client.post(&url)).await.unwrap();

        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(requests.load(Ordering::SeqCst), MAX_ATTEMPTS);
    }

    #[tokio::test]
    async fn an_evicted_request_is_not_sent_again() {
        let (url, requests) = upstream(1, StatusCode::TOO_MANY_REQUESTS, "evicted").await;
        let client = reqwest::Client::new();

        let response = send(|| client.post(&url)).await.unwrap();

        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(requests.load(Ordering::SeqCst), 1);
    }
}
