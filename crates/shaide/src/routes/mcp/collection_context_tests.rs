use super::*;
use crate::{config::StaticMcpServer, services::auth::init_test_auth_service};
use axum::{body::to_bytes, routing::any};
use chrono::{Duration, Utc};
use context::CONTEXT_HEADER;
use serde_json::json;
use std::sync::Arc;
use temp_testdir::TempDir;
use tower::ServiceExt;

struct Fixture {
    _root: TempDir,
    db: DbConn,
    user: i64,
    admin: i64,
    user_token: String,
    admin_token: String,
    state: GatewayState,
    received: Arc<tokio::sync::Mutex<Vec<(http::HeaderMap, serde_json::Value)>>>,
    task: tokio::task::JoinHandle<()>,
    public: Vec<u8>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}
async fn fixture() -> Fixture {
    crate::install_rustls_crypto_provider();
    let root = TempDir::default();
    let db = DbConn::new(&root.join("gateway.sqlite")).await.unwrap();
    let user = db
        .create_user(
            "synthetic-user".into(),
            String::new(),
            Utc::now() + Duration::days(1),
        )
        .await
        .unwrap();
    let admin = db
        .create_admin("synthetic-admin".into(), String::new())
        .await
        .unwrap();
    let auth = init_test_auth_service();
    let key = root.join("key.pem");
    let public = root.join("public.pem");
    assert!(
        std::process::Command::new("openssl")
            .args([
                "genpkey",
                "-algorithm",
                "RSA",
                "-pkeyopt",
                "rsa_keygen_bits:2048",
                "-out"
            ])
            .arg(&key)
            .output()
            .unwrap()
            .status
            .success()
    );
    assert!(
        std::process::Command::new("openssl")
            .args(["rsa", "-in"])
            .arg(&key)
            .args(["-RSAPublicKey_out", "-outform", "DER", "-out"])
            .arg(&public)
            .output()
            .unwrap()
            .status
            .success()
    );
    let token = root.join("token");
    std::fs::write(
        &token,
        format!("{}{}", context::fresh_jti(), context::fresh_jti()),
    )
    .unwrap();
    let received = Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let receiver = received.clone();
    let app = Router::new().route(
        "/mcp",
        any(move |request: Request| {
            let received = receiver.clone();
            async move {
                let (parts, body) = request.into_parts();
                let bytes = to_bytes(body, context::BODY_LIMIT).await.unwrap();
                let value = if bytes.is_empty() {
                    serde_json::Value::Null
                } else {
                    serde_json::from_slice(&bytes).unwrap()
                };
                received.lock().await.push((parts.headers, value));
                Json(json!({"jsonrpc":"2.0","id":1,"result":{"synthetic":true}}))
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let raw=json!([{"server_id":"kc","url":url,"issuer":"synthetic-gateway","kc_audience":"kc:kc","kid":"kc-context-v1","signing_key_file":key,"gateway_token_file":token}]).to_string();
    let targets = ContextTargets::parse(Some(&raw), Some("true"), "synthetic-jwt-secret").unwrap();
    let discovery = crate::services::mcp::McpService::from_static(&[
        StaticMcpServer {
            name: "kc".into(),
            url: url.clone(),
        },
        StaticMcpServer {
            name: "ordinary".into(),
            url,
        },
    ]);
    let state = GatewayState {
        db: db.clone(),
        targets,
        client: reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap(),
        discovery: Some(discovery),
    };
    Fixture {
        user_token: auth.issue_access_token(user, None).unwrap().0,
        admin_token: auth.issue_access_token(admin, None).unwrap().0,
        public: std::fs::read(public).unwrap(),
        _root: root,
        db,
        user,
        admin,
        state,
        received,
        task,
    }
}
impl Fixture {
    async fn request(
        &self,
        token: &str,
        path: &str,
        method: http::Method,
        body: Option<serde_json::Value>,
    ) -> Response {
        let body = body.map(|v| v.to_string()).unwrap_or_default();
        gateway_routes(self.state.clone())
            .oneshot(
                Request::builder()
                    .uri(path)
                    .method(method)
                    .header("authorization", format!("Bearer {token}"))
                    .header("x-mcp-authorization", "Bearer forged")
                    .header("x-shaide-collection-context", "forged-context")
                    .header("x-kc-token", "forged-private")
                    .header("x-user-id", "999")
                    .header("connection", "authorization,x-shaide-collection-context")
                    .header("mcp-session-id", "same-session")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap()
    }
    async fn usage(&self) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM api_usage")
            .fetch_one(&self.db.pool)
            .await
            .unwrap()
    }
    async fn last_claims(&self) -> context::Claims {
        let received = self.received.lock().await;
        let compact = received.last().unwrap().0[CONTEXT_HEADER].to_str().unwrap();
        let mut validation = jsonwebtoken::Validation::new(jsonwebtoken::Algorithm::RS256);
        validation.leeway = 0;
        validation.set_audience(&["kc-mcp:kc", "kc:kc"]);
        validation.set_issuer(&["synthetic-gateway"]);
        jsonwebtoken::decode::<context::Claims>(
            compact,
            &jsonwebtoken::DecodingKey::from_rsa_der(&self.public),
            &validation,
        )
        .unwrap()
        .claims
    }
    async fn collection(&self, name: &str, members: Vec<i64>) -> i64 {
        let model = self
            .db
            .insert_embedding_model(shaide_db::embedding_models::InsertEmbeddingModelDao {
                name: name.into(),
                url: "https://synthetic.invalid/embedding".into(),
                vector_size: 8,
                platform: None,
                api_schema: None,
                daily_input_token_limit: None,
            })
            .await
            .unwrap();
        self.db
            .create_organization(shaide_db::organizations::InsertOrganizationDao {
                name: name.into(),
                description: "synthetic-description".into(),
                can_users_upload: false,
                embedding_model_id: model,
                user_ids: members,
            })
            .await
            .unwrap()
    }
}
fn list_body() -> serde_json::Value {
    json!({"jsonrpc":"2.0","id":1,"method":"tools/list","_meta":{"role":"admin","client":"kept"},"params":{"_meta":{"shaide/context":"forged","progressToken":1}}})
}
#[tokio::test]
async fn gateway_zero_admin_membership_current_role_catalog_and_accounting() {
    let f = fixture().await;
    assert_eq!(
        f.request(
            &f.user_token,
            "/v1/mcp/proxy/kc/mcp",
            http::Method::POST,
            Some(list_body())
        )
        .await
        .status(),
        StatusCode::OK
    );
    assert!(f.last_claims().await.collections.is_empty());
    assert_eq!(f.usage().await, 1);
    let first = f.collection("first", vec![f.user]).await;
    let second = f.collection("second", vec![]).await;
    for (hash, name) in [("synth-a", "alpha.txt"), ("synth-b", "beta.txt")] {
        f.db.insert_file(shaide_db::files::InsertFileDao {
            hash: hash.into(),
            organization_id: first,
            file_name: name.into(),
            mime_type: "text/plain".into(),
        })
        .await
        .unwrap();
        f.db.claim_file_for_processing(first, hash).await.unwrap();
        f.db.mark_file_processed(first, hash, Some(10), None)
            .await
            .unwrap();
    }
    sqlx::query("UPDATE files SET uploaded_at='2026-01-01 00:00:00'")
        .execute(&f.db.pool)
        .await
        .unwrap();
    let response = f
        .request(
            &f.user_token,
            "/v1/mcp/proxy/kc/mcp",
            http::Method::POST,
            Some(list_body()),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["cache-control"], "private, no-store");
    let claims = f.last_claims().await;
    assert_eq!(claims.sub, f.user.to_string());
    assert_eq!(claims.role, "user");
    assert_eq!(claims.collections.len(), 1);
    assert_eq!(claims.collections[0].collection_id, first);
    assert_eq!(
        claims.catalog.as_ref().unwrap().entries[0].document_names,
        ["alpha.txt", "beta.txt"]
    );
    let received = f.received.lock().await;
    let (headers, body) = received.last().unwrap();
    assert!(!headers.contains_key("x-kc-token"));
    assert!(!headers.contains_key("x-user-id"));
    assert!(!headers.contains_key("x-mcp-authorization"));
    assert_ne!(headers["authorization"], format!("Bearer {}", f.user_token));
    assert_ne!(headers["authorization"], "Bearer forged");
    assert_eq!(body["_meta"], json!({"client":"kept"}));
    assert_eq!(body["params"]["_meta"], json!({"progressToken":1}));
    drop(received);
    assert_eq!(
        f.request(
            &f.admin_token,
            "/v1/mcp/proxy/kc/mcp",
            http::Method::POST,
            Some(list_body())
        )
        .await
        .status(),
        StatusCode::OK
    );
    let admin = f.last_claims().await;
    assert_eq!(admin.sub, f.admin.to_string());
    assert_eq!(admin.role, "admin");
    assert_eq!(
        admin
            .collections
            .iter()
            .map(|c| c.collection_id)
            .collect::<Vec<_>>(),
        [first, second]
    );
    assert_ne!(admin.jti, claims.jti);
    let usage = f.usage().await;
    assert_eq!(
        f.request(
            &f.admin_token,
            "/v1/mcp/proxy/ordinary/mcp",
            http::Method::POST,
            Some(list_body())
        )
        .await
        .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(f.usage().await, usage);
    f.db.remove_organization_members(first, &[f.user])
        .await
        .unwrap();
    assert_eq!(
        f.request(
            &f.user_token,
            "/v1/mcp/proxy/kc/mcp",
            http::Method::GET,
            None
        )
        .await
        .status(),
        StatusCode::OK
    );
    assert!(f.last_claims().await.collections.is_empty());
    assert!(f.last_claims().await.catalog.is_none());
    sqlx::query("UPDATE users SET role='admin' WHERE id=?")
        .bind(f.user)
        .execute(&f.db.pool)
        .await
        .unwrap();
    assert_eq!(
        f.request(
            &f.user_token,
            "/v1/mcp/proxy/kc/mcp",
            http::Method::DELETE,
            None
        )
        .await
        .status(),
        StatusCode::OK
    );
    assert_eq!(f.last_claims().await.collections.len(), 2);
    sqlx::query("UPDATE users SET role='user',expiry=? WHERE id=?")
        .bind(Utc::now() - Duration::days(1))
        .bind(f.user)
        .execute(&f.db.pool)
        .await
        .unwrap();
    let before = f.usage().await;
    assert_eq!(
        f.request(
            &f.user_token,
            "/v1/mcp/proxy/kc/mcp",
            http::Method::GET,
            None
        )
        .await
        .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(f.usage().await, before);
}
#[tokio::test]
async fn discovery_filters_admin_and_ordinary_forwarding_preserves_body_key() {
    let f = fixture().await;
    for (token, count) in [(&f.user_token, 2), (&f.admin_token, 1)] {
        let response = f
            .request(token, "/v1/list-mcps", http::Method::GET, None)
            .await;
        assert_eq!(response.status(), StatusCode::OK);
        let value: serde_json::Value = serde_json::from_slice(
            &to_bytes(response.into_body(), context::BODY_LIMIT)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(value["servers"].as_array().unwrap().len(), count);
    }
    let original = json!({"jsonrpc":"2.0","method":"tools/list","_meta":{"shaide/context":"ordinary-body-kept","role":"body-kept"}});
    assert_eq!(
        f.request(
            &f.user_token,
            "/v1/mcp/proxy/ordinary/mcp",
            http::Method::POST,
            Some(original.clone())
        )
        .await
        .status(),
        StatusCode::OK
    );
    let received = f.received.lock().await;
    let (headers, body) = received.last().unwrap();
    assert!(!headers.contains_key(CONTEXT_HEADER));
    assert!(!headers.contains_key("x-kc-token"));
    assert_eq!(headers["authorization"], "Bearer forged");
    assert_eq!(*body, original);
    assert_eq!(f.usage().await, 3);
}
#[tokio::test]
async fn unsupported_paths_mismatch_and_overflow_never_dispatch() {
    let mut f = fixture().await;
    for path in [
        "/v1/mcp/proxy/kc/internal",
        "/v1/mcp/proxy/kc/%6dcp",
        "/v1/mcp/proxy/kc/mcp?q=1",
        "/v1/mcp/proxy/kc/mcp/",
    ] {
        assert_eq!(
            f.request(&f.user_token, path, http::Method::POST, Some(list_body()))
                .await
                .status(),
            StatusCode::NOT_FOUND
        );
    }
    assert!(f.received.lock().await.is_empty());
    f.state.discovery = Some(crate::services::mcp::McpService::from_static(&[
        StaticMcpServer {
            name: "kc".into(),
            url: "https://synthetic-mismatch.invalid".into(),
        },
    ]));
    assert_eq!(
        f.request(
            &f.user_token,
            "/v1/mcp/proxy/kc/mcp",
            http::Method::POST,
            Some(list_body())
        )
        .await
        .status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert!(f.received.lock().await.is_empty());
}
#[tokio::test]
async fn expired_scope_has_no_dispatch_and_delayed_response_is_cancelled() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let captured = calls.clone();
    let app = Router::new().route(
        "/mcp",
        any(move || {
            let calls = captured.clone();
            async move {
                calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                "too late"
            }
        }),
    );
    let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let now = Utc::now().timestamp();
    assert!(
        forward_participating(
            &client,
            &origin,
            http::Method::POST,
            http::HeaderMap::new(),
            axum::body::Bytes::new(),
            now
        )
        .await
        .is_err()
    );
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    assert_eq!(
        forward_participating(
            &client,
            &origin,
            http::Method::POST,
            http::HeaderMap::new(),
            axum::body::Bytes::new(),
            now + 1
        )
        .await
        .unwrap()
        .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    task.abort();
}

#[tokio::test]
async fn redirect_cannot_forward_context_to_another_origin() {
    let hits = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let captured = hits.clone();
    let sink_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let sink_url = format!("http://{}/mcp", sink_listener.local_addr().unwrap());
    let sink = Router::new().route(
        "/mcp",
        any(move || {
            let hits = captured.clone();
            async move {
                hits.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                "leaked"
            }
        }),
    );
    let sink_task = tokio::spawn(async move { axum::serve(sink_listener, sink).await.unwrap() });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let redirect = Router::new().route(
        "/mcp",
        any(move || {
            let url = sink_url.clone();
            async move { axum::response::Redirect::temporary(&url) }
        }),
    );
    let task = tokio::spawn(async move { axum::serve(listener, redirect).await.unwrap() });
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let mut headers = http::HeaderMap::new();
    headers.insert(
        CONTEXT_HEADER,
        http::HeaderValue::from_static("synthetic-context"),
    );
    headers.insert(
        "authorization",
        http::HeaderValue::from_static("Bearer synthetic-service-token"),
    );
    assert_eq!(
        forward_participating(
            &client,
            &origin,
            http::Method::POST,
            headers,
            axum::body::Bytes::new(),
            Utc::now().timestamp() + 30
        )
        .await
        .unwrap()
        .status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), 0);
    task.abort();
    sink_task.abort();
}
#[tokio::test]
async fn sixty_five_collections_fail_before_http_dispatch() {
    let f = fixture().await;
    let first = f.collection("synthetic", vec![]).await;
    let model: i64 = sqlx::query_scalar("SELECT embedding_model_id FROM organizations WHERE id=?")
        .bind(first)
        .fetch_one(&f.db.pool)
        .await
        .unwrap();
    for _ in 0..64 {
        sqlx::query(
            "INSERT INTO organizations(organization_name,embedding_model_id) VALUES('synthetic',?)",
        )
        .bind(model)
        .execute(&f.db.pool)
        .await
        .unwrap();
    }
    assert_eq!(
        f.request(
            &f.admin_token,
            "/v1/mcp/proxy/kc/mcp",
            http::Method::POST,
            Some(list_body())
        )
        .await
        .status(),
        StatusCode::PAYLOAD_TOO_LARGE
    );
    assert!(f.received.lock().await.is_empty());
    assert_eq!(f.usage().await, 1);
}

#[tokio::test]
async fn real_gateway_http_accepts_sixteen_kib_header_budget_and_rejects_aggregate_overflow() {
    let f = fixture().await;
    let app = gateway_routes(f.state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!(
        "http://{}/v1/mcp/proxy/kc/mcp",
        listener.local_addr().unwrap()
    );
    let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let response = reqwest::Client::new()
        .post(&url)
        .bearer_auth(&f.user_token)
        .header("x-synthetic-padding", "a".repeat(16384))
        .json(&list_body())
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(f.last_claims().await.collections.is_empty());
    let count = f.received.lock().await.len();
    let response = reqwest::Client::new()
        .post(&url)
        .bearer_auth(&f.user_token)
        .header("x-synthetic-padding", "a".repeat(32768))
        .json(&list_body())
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(f.received.lock().await.len(), count);
    task.abort();
}

#[tokio::test]
async fn unauthorized_and_deleted_accounts_never_dispatch_or_account() {
    let f = fixture().await;
    for token in ["malformed", ""] {
        assert_eq!(
            f.request(
                token,
                "/v1/mcp/proxy/kc/mcp",
                http::Method::POST,
                Some(list_body())
            )
            .await
            .status(),
            StatusCode::UNAUTHORIZED
        );
    }
    sqlx::query("DELETE FROM users WHERE id=?")
        .bind(f.user)
        .execute(&f.db.pool)
        .await
        .unwrap();
    assert_eq!(
        f.request(
            &f.user_token,
            "/v1/mcp/proxy/kc/mcp",
            http::Method::POST,
            Some(list_body())
        )
        .await
        .status(),
        StatusCode::UNAUTHORIZED
    );
    assert!(f.received.lock().await.is_empty());
    assert_eq!(f.usage().await, 0);
}

#[tokio::test]
async fn completed_response_body_cannot_be_delivered_after_expiry() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let app = Router::new().route(
        "/mcp",
        any(|| async { Json(json!({"result":"synthetic result"})) }),
    );
    let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let response = forward_participating(
        &client,
        &origin,
        http::Method::POST,
        http::HeaderMap::new(),
        axum::body::Bytes::new(),
        Utc::now().timestamp() + 2,
    )
    .await
    .unwrap();
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    assert!(
        to_bytes(response.into_body(), context::BODY_LIMIT)
            .await
            .is_err()
    );
    task.abort();
}

#[tokio::test]
async fn upstream_errors_cannot_echo_private_context() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let app = Router::new().route(
        "/mcp",
        any(|| async {
            (
                StatusCode::SERVICE_UNAVAILABLE,
                "PRIVATE-TOKEN-CATALOG-MARKER",
            )
        }),
    );
    let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let response = forward_participating(
        &client,
        &origin,
        http::Method::POST,
        http::HeaderMap::new(),
        axum::body::Bytes::new(),
        Utc::now().timestamp() + 30,
    )
    .await
    .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let bytes = to_bytes(response.into_body(), context::BODY_LIMIT)
        .await
        .unwrap();
    assert!(!String::from_utf8_lossy(&bytes).contains("PRIVATE-TOKEN-CATALOG-MARKER"));
    task.abort();
}

#[tokio::test]
async fn authorization_snapshot_rechecks_role_after_initial_authentication() {
    let f = fixture().await;
    let collection = f.collection("private-admin-only", vec![]).await;
    let req = Request::builder()
        .uri("/v1/mcp/proxy/kc/mcp")
        .header("authorization", format!("Bearer {}", f.user_token))
        .body(Body::empty())
        .unwrap();
    let (parts, _) = req.into_parts();
    let authenticated = authorize_request(&parts, &f.db, AccessRequirement::Any)
        .await
        .unwrap();
    assert_eq!(authenticated.user.role, UserRole::User);
    sqlx::query("UPDATE users SET role='admin' WHERE id=?")
        .bind(f.user)
        .execute(&f.db.pool)
        .await
        .unwrap();
    let snapshot =
        f.db.collection_context_snapshot(authenticated.user.id, false)
            .await
            .unwrap();
    let claims = f.state.targets.0["kc"]
        .claims(
            snapshot,
            authenticated.request_id,
            false,
            Utc::now().timestamp(),
        )
        .unwrap();
    assert_eq!(claims.role, "admin");
    assert_eq!(claims.collections[0].collection_id, collection);
    assert_eq!(f.usage().await, 1);
    sqlx::query("UPDATE users SET role='user',expiry=? WHERE id=?")
        .bind(Utc::now() - Duration::days(1))
        .bind(f.user)
        .execute(&f.db.pool)
        .await
        .unwrap();
    let snapshot =
        f.db.collection_context_snapshot(authenticated.user.id, false)
            .await
            .unwrap();
    assert!(
        f.state.targets.0["kc"]
            .claims(
                snapshot,
                authenticated.request_id,
                false,
                Utc::now().timestamp()
            )
            .is_err()
    );
    assert_eq!(f.usage().await, 1);
}

#[tokio::test]
async fn duplicate_bearer_headers_never_dispatch_or_account() {
    let f = fixture().await;
    let mut request = Request::builder()
        .uri("/v1/mcp/proxy/kc/mcp")
        .method(http::Method::POST)
        .body(Body::from(list_body().to_string()))
        .unwrap();
    let value = http::HeaderValue::from_str(&format!("Bearer {}", f.user_token)).unwrap();
    request.headers_mut().append("authorization", value.clone());
    request.headers_mut().append("authorization", value);
    let response = gateway_routes(f.state.clone())
        .oneshot(request)
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(f.usage().await, 0);
    assert!(f.received.lock().await.is_empty());
}

// Own fixture tasks even if assertions fail.
struct OwnedServer(tokio::task::JoinHandle<()>);
impl Drop for OwnedServer {
    fn drop(&mut self) {
        self.0.abort();
    }
}
async fn fix1_upstream(
    status: StatusCode,
    headers: http::HeaderMap,
    body: &'static str,
    method: http::Method,
) -> Response {
    use axum::response::IntoResponse;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let app = Router::new().route(
        "/mcp",
        any(move || {
            let headers = headers.clone();
            async move {
                let mut response = Response::new(Body::from(body));
                *response.status_mut() = status;
                *response.headers_mut() = headers;
                response
            }
        }),
    );
    let _task = OwnedServer(tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap()
    }));
    forward_participating(
        &reqwest::Client::new(),
        &origin,
        method,
        http::HeaderMap::new(),
        axum::body::Bytes::new(),
        Utc::now().timestamp() + 30,
    )
    .await
    .unwrap_or_else(IntoResponse::into_response)
}
#[tokio::test]
async fn fix1_transport_statuses_are_sanitized_and_allow_is_validated() {
    for method in [http::Method::GET, http::Method::DELETE] {
        for status in [
            StatusCode::METHOD_NOT_ALLOWED,
            StatusCode::PAYLOAD_TOO_LARGE,
            StatusCode::UNAUTHORIZED,
            StatusCode::FORBIDDEN,
        ] {
            let mut headers = http::HeaderMap::new();
            headers.insert("allow", http::HeaderValue::from_static("POST"));
            headers.insert(
                "authorization",
                http::HeaderValue::from_static("Bearer PRIVATE-MARKER"),
            );
            headers.insert(
                "set-cookie",
                http::HeaderValue::from_static("PRIVATE-MARKER"),
            );
            let response = fix1_upstream(status, headers, "PRIVATE-MARKER", method.clone()).await;
            assert_eq!(response.status(), status);
            assert_eq!(response.headers()["cache-control"], "private, no-store");
            assert!(!response.headers().contains_key("authorization"));
            assert!(!response.headers().contains_key("set-cookie"));
            if status == StatusCode::METHOD_NOT_ALLOWED {
                assert_eq!(response.headers()["allow"], "POST");
            }
            let bytes = to_bytes(response.into_body(), context::BODY_LIMIT)
                .await
                .unwrap();
            assert!(!String::from_utf8_lossy(&bytes).contains("PRIVATE-MARKER"));
            assert_eq!(
                serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()["error"]["code"],
                "collection_context_upstream_rejected"
            );
        }
    }
    for allow in ["POST, PRIVATE-MARKER", "POST /internal", "", "GET, GET"] {
        let mut headers = http::HeaderMap::new();
        headers.insert("allow", http::HeaderValue::from_str(allow).unwrap());
        let response = fix1_upstream(
            StatusCode::METHOD_NOT_ALLOWED,
            headers,
            "private",
            http::Method::GET,
        )
        .await;
        assert!(!response.headers().contains_key("allow"));
    }
}
#[tokio::test]
async fn fix1_json_media_types_sse_and_empty_acknowledgments() {
    for (status, media, body, expected) in [
        (
            StatusCode::OK,
            Some("application/json"),
            "{}",
            StatusCode::OK,
        ),
        (
            StatusCode::OK,
            Some("Application/JSON; charset=utf-8"),
            "{}",
            StatusCode::OK,
        ),
        (
            StatusCode::OK,
            Some("text/event-stream"),
            "data: {}\n\n",
            StatusCode::SERVICE_UNAVAILABLE,
        ),
        (
            StatusCode::OK,
            Some("text/event-stream"),
            "",
            StatusCode::SERVICE_UNAVAILABLE,
        ),
        (
            StatusCode::OK,
            Some("application/json-malformed"),
            "{}",
            StatusCode::SERVICE_UNAVAILABLE,
        ),
        (
            StatusCode::OK,
            Some("text/plain"),
            "{}",
            StatusCode::SERVICE_UNAVAILABLE,
        ),
        (StatusCode::OK, None, "{}", StatusCode::SERVICE_UNAVAILABLE),
        (
            StatusCode::OK,
            Some("application/json"),
            "not json",
            StatusCode::SERVICE_UNAVAILABLE,
        ),
        (StatusCode::ACCEPTED, None, "", StatusCode::ACCEPTED),
        (StatusCode::NO_CONTENT, None, "", StatusCode::NO_CONTENT),
    ] {
        let mut headers = http::HeaderMap::new();
        if let Some(media) = media {
            headers.insert("content-type", http::HeaderValue::from_static(media));
        }
        headers.insert(
            "mcp-session-id",
            http::HeaderValue::from_static("synthetic-session"),
        );
        headers.insert(
            "set-cookie",
            http::HeaderValue::from_static("PRIVATE-MARKER"),
        );
        let response = fix1_upstream(status, headers, body, http::Method::POST).await;
        assert_eq!(response.status(), expected, "media={media:?} body={body}");
        assert_eq!(response.headers()["cache-control"], "private, no-store");
        assert!(!response.headers().contains_key("set-cookie"));
        if expected.is_success() {
            assert_eq!(response.headers()["mcp-session-id"], "synthetic-session");
        }
    }
}

// Raw HTTP avoids framework-added Content-Type and preserves duplicate fields.
async fn fix2_raw_upstream(status: u16, fields: &str, body: &str) -> Response {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let wire = format!(
        "HTTP/1.1 {status} fixture\r\nContent-Length: {}\r\n{fields}\r\n{body}",
        body.len()
    );
    let _server = OwnedServer(tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        let mut byte = [0];
        while !request.ends_with(b"\r\n\r\n") {
            socket.read_exact(&mut byte).await.unwrap();
            request.push(byte[0]);
            assert!(request.len() < context::HEADER_LIMIT);
        }
        socket.write_all(wire.as_bytes()).await.unwrap();
        socket.shutdown().await.unwrap();
    }));
    forward_participating(
        &reqwest::Client::new(),
        &origin,
        http::Method::GET,
        http::HeaderMap::new(),
        axum::body::Bytes::new(),
        Utc::now().timestamp() + 30,
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn fix2_connection_content_type_cannot_validate_nonempty_json() {
    let response = fix2_raw_upstream(
        200,
        "Content-Type: application/json\r\nConnection: Content-Type\r\n",
        "{}",
    )
    .await;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(response.headers()["cache-control"], "private, no-store");
}

#[tokio::test]
async fn fix2_ambiguous_invalid_and_sse_media_are_rejected_on_empty_acks() {
    for status in [202, 204] {
        for fields in [
            "Content-Type: text/event-stream\r\nContent-Type: text/event-stream\r\n",
            "Content-Type: application/json\r\nContent-Type: application/json\r\n",
            "Content-Type: text/event-stream\r\nConnection: content-type\r\n",
            "Content-Type: text/event-stream\r\n",
            "Content-Type: application/json, text/event-stream\r\n",
            "Content-Type: \r\n",
            "Content-Type: application/json; charset=\r\n",
            "Content-Type: text/plain\r\n",
        ] {
            let response = fix2_raw_upstream(status, fields, "").await;
            assert_eq!(
                response.status(),
                StatusCode::SERVICE_UNAVAILABLE,
                "{status} {fields:?}"
            );
            assert_eq!(response.headers()["cache-control"], "private, no-store");
            let bytes = to_bytes(response.into_body(), context::BODY_LIMIT)
                .await
                .unwrap();
            assert_eq!(
                serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()["error"]["code"],
                "collection_context_upstream_transport_unsupported"
            );
        }
    }
}

#[tokio::test]
async fn fix2_absent_media_acks_and_normal_json_keep_status_and_privacy() {
    for (status, fields, body) in [
        (202, "", ""),
        (204, "", ""),
        (
            202,
            "Content-Type: application/json\r\nConnection: content-type\r\n",
            "",
        ),
        (
            204,
            "Content-Type: application/json\r\nConnection: content-type\r\n",
            "",
        ),
        (
            200,
            "Content-Type: Application/JSON; charset=utf-8\r\n",
            "{}",
        ),
    ] {
        let fields = format!(
            "{fields}Mcp-Session-Id: synthetic-session\r\nMcp-Protocol-Version: 2025-03-26\r\nSet-Cookie: PRIVATE-MARKER\r\nAuthorization: PRIVATE-MARKER\r\nX-Private: PRIVATE-MARKER\r\nCache-Control: public\r\n"
        );
        let response = fix2_raw_upstream(status, &fields, body).await;
        assert_eq!(response.status().as_u16(), status);
        assert_eq!(response.headers()["cache-control"], "private, no-store");
        assert_eq!(response.headers()["mcp-session-id"], "synthetic-session");
        assert_eq!(response.headers()["mcp-protocol-version"], "2025-03-26");
        for name in ["set-cookie", "authorization", "x-private", "connection"] {
            assert!(!response.headers().contains_key(name));
        }
        assert_eq!(
            response.headers().contains_key("content-type"),
            status == 200
        );
        assert_eq!(
            to_bytes(response.into_body(), context::BODY_LIMIT)
                .await
                .unwrap()
                .as_ref(),
            body.as_bytes()
        );
    }
}

#[tokio::test]
async fn fix2_safe_allow_respects_connection_and_ambiguity() {
    for (fields, expected) in [
        ("Allow: GET, POST, DELETE\r\n", Some("GET, POST, DELETE")),
        ("Allow: POST\r\nConnection: allow\r\n", None),
        ("Allow: POST\r\nAllow: GET\r\n", None),
        ("Allow: POST, PRIVATE-MARKER\r\n", None),
        ("Allow: GET, GET\r\n", None),
    ] {
        let fields = format!("{fields}Set-Cookie: PRIVATE-MARKER\r\n");
        let response = fix2_raw_upstream(405, &fields, "PRIVATE-MARKER").await;
        assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
        assert_eq!(response.headers()["cache-control"], "private, no-store");
        assert_eq!(
            response.headers().get("allow").map(|v| v.to_str().unwrap()),
            expected
        );
        assert!(!response.headers().contains_key("set-cookie"));
        let bytes = to_bytes(response.into_body(), context::BODY_LIMIT)
            .await
            .unwrap();
        assert!(!String::from_utf8_lossy(&bytes).contains("PRIVATE-MARKER"));
    }
}

// Diagnostic acceptance gate: blocked until the server owns cancellable writes.
// Uses real TCP backpressure and counts writes accepted by the OS after exp;
// client read timing alone would not establish the user-space delivery boundary.
struct ObservedIo {
    inner: tokio::net::TcpStream,
    exp: i64,
    pending: Arc<std::sync::atomic::AtomicUsize>,
    late: Arc<std::sync::atomic::AtomicUsize>,
}
impl tokio::io::AsyncRead for ObservedIo {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.inner).poll_read(cx, buf)
    }
}
impl tokio::io::AsyncWrite for ObservedIo {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        let result = std::pin::Pin::new(&mut self.inner).poll_write(cx, buf);
        match &result {
            std::task::Poll::Pending => {
                self.pending
                    .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            }
            std::task::Poll::Ready(Ok(n)) if Utc::now().timestamp() >= self.exp => {
                self.late.fetch_add(*n, std::sync::atomic::Ordering::SeqCst);
            }
            _ => {}
        }
        result
    }
    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.inner).poll_flush(cx)
    }
    fn poll_shutdown(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}
struct ObservedListener {
    inner: tokio::net::TcpListener,
    exp: i64,
    pending: Arc<std::sync::atomic::AtomicUsize>,
    late: Arc<std::sync::atomic::AtomicUsize>,
}
impl axum::serve::Listener for ObservedListener {
    type Io = ObservedIo;
    type Addr = std::net::SocketAddr;
    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        let (inner, addr) = self.inner.accept().await.unwrap();
        (
            ObservedIo {
                inner,
                exp: self.exp,
                pending: self.pending.clone(),
                late: self.late.clone(),
            },
            addr,
        )
    }
    fn local_addr(&self) -> std::io::Result<Self::Addr> {
        self.inner.local_addr()
    }
}
#[tokio::test]
#[ignore = "SHD-1077 finding 1: route Body cannot cancel Hyper-owned pending socket writes; explicit blocker probe"]
async fn fix1_pending_socket_writes_stop_at_expiry() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let upstream = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", upstream.local_addr().unwrap());
    let payload =
        json!({"jsonrpc":"2.0","id":1,"result":"s".repeat(context::BODY_LIMIT-128)}).to_string();
    let app = Router::new().route(
        "/mcp",
        any(move || {
            let body = payload.clone();
            async move { ([("content-type", "application/json")], body) }
        }),
    );
    let _upstream = OwnedServer(tokio::spawn(async move {
        axum::serve(upstream, app).await.unwrap()
    }));
    let exp = Utc::now().timestamp() + 3;
    let app = Router::new().route(
        "/",
        any(move || {
            let origin = origin.clone();
            async move {
                forward_participating(
                    &reqwest::Client::new(),
                    &origin,
                    http::Method::POST,
                    http::HeaderMap::new(),
                    axum::body::Bytes::new(),
                    exp,
                )
                .await
                .unwrap()
            }
        }),
    );
    let socket = tokio::net::TcpSocket::new_v4().unwrap();
    socket.set_send_buffer_size(4096).unwrap();
    socket.bind("127.0.0.1:0".parse().unwrap()).unwrap();
    let listener = socket.listen(8).unwrap();
    let addr = listener.local_addr().unwrap();
    let pending = Arc::new(AtomicUsize::new(0));
    let late = Arc::new(AtomicUsize::new(0));
    let listener = ObservedListener {
        inner: listener,
        exp,
        pending: pending.clone(),
        late: late.clone(),
    };
    let _gateway = OwnedServer(tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap()
    }));
    let socket = tokio::net::TcpSocket::new_v4().unwrap();
    socket.set_recv_buffer_size(4096).unwrap();
    let mut client = socket.connect(addr).await.unwrap();
    client
        .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut prefix = [0; 512];
    assert!(
        tokio::time::timeout(std::time::Duration::from_secs(1), client.read(&mut prefix))
            .await
            .unwrap()
            .unwrap()
            > 0
    );
    // Stop reading while both TCP buffers fill, then resume only after expiry.
    let wait_ms = (exp * 1000 - Utc::now().timestamp_millis() + 100).max(0) as u64;
    tokio::time::sleep(std::time::Duration::from_millis(wait_ms)).await;
    assert!(
        pending.load(Ordering::SeqCst) > 0,
        "fixture must establish real socket backpressure"
    );
    let mut rest = Vec::new();
    tokio::time::timeout(
        std::time::Duration::from_secs(10),
        client.read_to_end(&mut rest),
    )
    .await
    .unwrap()
    .unwrap();
    eprintln!(
        "backpressure pending polls={}, post-expiry OS-accepted bytes={}",
        pending.load(Ordering::SeqCst),
        late.load(Ordering::SeqCst)
    );
    assert_eq!(
        late.load(Ordering::SeqCst),
        0,
        "pending user-space writes must be cancelled at exp; OS/client-owned bytes are excluded"
    );
}
