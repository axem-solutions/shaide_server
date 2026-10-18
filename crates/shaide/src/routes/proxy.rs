use axum::Router;
use axum_reverse_proxy::ReverseProxy;

/// Proxies `path`, `path/` and everything below it to `target`.
///
/// `ReverseProxy` mounts itself with `Router::nest`, which matches `path` and `path/{*rest}` but
/// not `path/` itself, because a catch-all never matches an empty remainder. `path/` is the
/// canonical URL of an app built for that subpath, so it gets its own route.
pub fn subpath_proxy(path: &str, target: &str) -> Router {
    let proxy = ReverseProxy::new(path, target);
    Router::new()
        .route_service(&format!("{path}/"), proxy.clone())
        .merge(proxy)
}

#[cfg(test)]
mod tests {
    use axum::{
        Router,
        body::{Body, to_bytes},
        http::{Request, StatusCode, Uri},
    };
    use tower::ServiceExt;

    use super::subpath_proxy;

    /// Starts an upstream that answers every request with the path and query it received.
    async fn spawn_echo_upstream() -> String {
        // `ReverseProxy::new` builds a TLS-capable client, which needs the provider `main` installs.
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
        let upstream = Router::new().fallback(|uri: Uri| async move {
            uri.path_and_query()
                .map(|path_and_query| path_and_query.to_string())
                .unwrap_or_default()
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, upstream).await.unwrap() });
        format!("http://{address}")
    }

    async fn upstream_path_for(app: &Router, path: &str) -> (StatusCode, String) {
        let response = app
            .clone()
            .oneshot(Request::get(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        (status, String::from_utf8(body.to_vec()).unwrap())
    }

    #[tokio::test]
    async fn proxies_the_subpath_with_and_without_trailing_slash() {
        let upstream = spawn_echo_upstream().await;
        let app = subpath_proxy("/app", &upstream);

        for (path, expected_upstream_path) in [
            ("/app", "/"),
            ("/app/", "/"),
            ("/app/index.html", "/index.html"),
            ("/app/assets/index.js?v=1", "/assets/index.js?v=1"),
        ] {
            assert_eq!(
                upstream_path_for(&app, path).await,
                (StatusCode::OK, expected_upstream_path.to_owned()),
                "request to {path}"
            );
        }
    }

    #[tokio::test]
    async fn leaves_other_paths_to_the_rest_of_the_router() {
        let upstream = spawn_echo_upstream().await;
        let app = subpath_proxy("/app", &upstream);

        for path in ["/", "/application", "/v1/models"] {
            let (status, _) = upstream_path_for(&app, path).await;
            assert_eq!(status, StatusCode::NOT_FOUND, "request to {path}");
        }
    }
}
