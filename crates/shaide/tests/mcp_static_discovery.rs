//! End-to-end checks of static MCP discovery (`MCP_SERVERS`).
//!
//! Every test starts the real `shaide` binary in an isolated temporary root
//! with a synthetic admin and user, no kubeconfig, and a disposable loopback
//! HTTP fixture standing in for the MCP servers.

use std::{
    net::{SocketAddr, TcpListener as StdTcpListener},
    path::Path,
    process::{Child, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use axum::{Json, Router, extract::Request};
use serde_json::{Value, json};
use temp_testdir::TempDir;

const ADMIN_PASSWORD: &str = "synthetic-admin-password";
const JWT_SECRET: &str = "synthetic-jwt-secret-for-mcp-tests-0123456789";
const USER_NAME: &str = "synthetic-mcp-user";
const USER_PASSWORD: &str = "synthetic-user-password";
const STARTUP_TIMEOUT: Duration = Duration::from_secs(180);

/// Records every request it receives and echoes what the proxy forwarded.
struct Fixture {
    addr: SocketAddr,
    hits: Arc<AtomicUsize>,
}

impl Fixture {
    async fn start() -> Self {
        let hits = Arc::new(AtomicUsize::new(0));
        let counter = hits.clone();
        let app = Router::new().fallback(move |req: Request| {
            let counter = counter.clone();
            async move {
                counter.fetch_add(1, Ordering::SeqCst);
                let header = |name: &str| {
                    req.headers()
                        .get(name)
                        .map(|v| v.to_str().unwrap().to_owned())
                };
                Json(json!({
                    "path": req.uri().path(),
                    "query": req.uri().query(),
                    "authorization": header("authorization"),
                    "x_mcp_authorization": header("x-mcp-authorization"),
                }))
            }
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        Self { addr, hits }
    }

    fn url(&self, path: &str) -> String {
        format!("http://{}{path}", self.addr)
    }

    fn hits(&self) -> usize {
        self.hits.load(Ordering::SeqCst)
    }
}

fn free_port() -> u16 {
    StdTcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// Builds a `shaide` command with only synthetic configuration. The working
/// directory is the temporary root so no `.env` file is picked up, and `HOME`
/// points there so no kubeconfig can be found.
fn server_command(root: &Path, port: u16, mcp_servers: Option<&str>) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_shaide"));
    command
        .current_dir(root)
        .env_clear()
        .env("HOME", root)
        .env("shaide_ROOT", root.join("shaide"))
        .env("HOST", "127.0.0.1")
        .env("PORT", port.to_string())
        .env("ADMIN_PASSWORD", ADMIN_PASSWORD)
        .env("JWT_SECRET", JWT_SECRET)
        .env("SHAIDE_SERVER_UI_FQDN", "127.0.0.1")
        .env("SHAIDE_SERVER_UI_PORT", "9")
        .env("NO_COLOR", "1")
        .stdout(Stdio::from(
            std::fs::File::create(root.join("stdout.log")).unwrap(),
        ))
        .stderr(Stdio::from(
            std::fs::File::create(root.join("stderr.log")).unwrap(),
        ));
    if let Some(mcp_servers) = mcp_servers {
        command.env("MCP_SERVERS", mcp_servers);
    }
    command
}

fn output(root: &Path) -> String {
    let stdout = std::fs::read_to_string(root.join("stdout.log")).unwrap_or_default();
    let stderr = std::fs::read_to_string(root.join("stderr.log")).unwrap_or_default();
    format!("{stdout}{stderr}")
}

struct TestServer {
    child: Child,
    base: String,
    client: reqwest::Client,
    root: TempDir,
}

impl Drop for TestServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl TestServer {
    async fn start(mcp_servers: Option<&str>) -> Self {
        let root = TempDir::default();
        let port = free_port();
        let child = server_command(&root, port, mcp_servers).spawn().unwrap();
        let mut server = Self {
            child,
            base: format!("http://127.0.0.1:{port}"),
            client: reqwest::Client::new(),
            root,
        };
        let deadline = Instant::now() + STARTUP_TIMEOUT;
        loop {
            if let Some(status) = server.child.try_wait().unwrap() {
                panic!("server exited with {status}:\n{}", output(&server.root));
            }
            if let Ok(response) = server.client.get(server.url("/v1/health")).send().await
                && response.status().is_success()
            {
                return server;
            }
            assert!(Instant::now() < deadline, "server did not become ready");
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }

    fn is_running(&mut self) -> bool {
        self.child.try_wait().unwrap().is_none()
    }

    async fn login(&self, username: &str, password: &str) -> String {
        let response = self
            .client
            .post(self.url("/v1/login"))
            .json(&json!({"username": username, "password": password}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200, "login of {username} failed");
        let body: Value = response.json().await.unwrap();
        body["access_token"].as_str().unwrap().to_owned()
    }

    /// Creates a synthetic non-admin user and returns its access token.
    async fn user_token(&self) -> String {
        let admin = self.login("admin", ADMIN_PASSWORD).await;
        let expiry = chrono::Utc::now() + chrono::Duration::days(1);
        let response = self
            .client
            .post(self.url("/v1/user"))
            .bearer_auth(admin)
            .json(&json!({"username": USER_NAME, "password": USER_PASSWORD, "expiry": expiry}))
            .send()
            .await
            .unwrap();
        assert!(response.status().is_success(), "user creation failed");
        self.login(USER_NAME, USER_PASSWORD).await
    }

    async fn list_mcps(&self, token: Option<&str>) -> reqwest::Response {
        let mut request = self.client.get(self.url("/v1/list-mcps"));
        if let Some(token) = token {
            request = request.bearer_auth(token);
        }
        request.send().await.expect("list-mcps request failed")
    }
}

fn sorted_servers(body: &Value) -> Vec<(String, String, String)> {
    let mut servers: Vec<_> = body["servers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| {
            (
                s["name"].as_str().unwrap().to_owned(),
                s["status"].as_str().unwrap().to_owned(),
                s["url"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    servers.sort();
    servers
}

#[tokio::test(flavor = "multi_thread")]
async fn static_servers_are_listed_without_kubernetes() {
    let fixture = Fixture::start().await;
    let config = json!([
        {"name": "alpha", "url": fixture.url("/alpha-base/")},
        {"name": "beta", "url": fixture.url("")},
    ])
    .to_string();
    let server = TestServer::start(Some(&config)).await;

    let unauthenticated = server.list_mcps(None).await;
    assert_eq!(unauthenticated.status(), 401);

    let token = server.user_token().await;
    let response = server.list_mcps(Some(&token)).await;
    assert_eq!(response.status(), 200);
    let body: Value = response.json().await.unwrap();
    assert_eq!(
        sorted_servers(&body),
        vec![
            (
                "alpha".to_owned(),
                "Running".to_owned(),
                fixture.url("/alpha-base")
            ),
            ("beta".to_owned(), "Running".to_owned(), fixture.url("")),
        ]
    );
    assert_eq!(fixture.hits(), 0, "listing must not contact the servers");
}

async fn proxy_get(
    server: &TestServer,
    path: &str,
    token: Option<&str>,
    mcp_authorization: Option<&str>,
) -> reqwest::Response {
    let mut request = server.client.get(server.url(path));
    if let Some(token) = token {
        request = request.bearer_auth(token);
    }
    if let Some(value) = mcp_authorization {
        request = request.header("x-mcp-authorization", value);
    }
    request.send().await.expect("proxy request failed")
}

#[tokio::test(flavor = "multi_thread")]
async fn proxy_targets_only_configured_static_servers() {
    let fixture = Fixture::start().await;
    let config = json!([
        {"name": "alpha", "url": fixture.url("/alpha-base/")},
        {"name": "beta", "url": fixture.url("")},
    ])
    .to_string();
    let server = TestServer::start(Some(&config)).await;
    let token = server.user_token().await;

    // The base path is kept, and the caller's shaide token is not forwarded.
    let response = proxy_get(
        &server,
        "/v1/mcp/proxy/alpha/sse?session=1",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(response.status(), 200);
    let echoed: Value = response.json().await.unwrap();
    assert_eq!(echoed["path"], "/alpha-base/sse");
    assert_eq!(echoed["query"], "session=1");
    assert_eq!(echoed["authorization"], Value::Null);

    // `x-mcp-authorization` still becomes the downstream `Authorization`.
    let response = proxy_get(
        &server,
        "/v1/mcp/proxy/beta/messages",
        Some(&token),
        Some("Bearer synthetic-downstream-token"),
    )
    .await;
    assert_eq!(response.status(), 200);
    let echoed: Value = response.json().await.unwrap();
    assert_eq!(echoed["path"], "/messages");
    assert_eq!(echoed["authorization"], "Bearer synthetic-downstream-token");
    assert_eq!(echoed["x_mcp_authorization"], Value::Null);
    assert_eq!(fixture.hits(), 2);

    // Unknown ids, caller-supplied URLs and unauthenticated callers never
    // reach a server.
    for path in [
        "/v1/mcp/proxy/gamma/sse",
        "/v1/mcp/proxy/http%3A%2F%2F127.0.0.1%3A1/sse",
    ] {
        let response = proxy_get(&server, path, Some(&token), None).await;
        assert_eq!(response.status(), 404, "{path}");
    }
    let response = proxy_get(&server, "/v1/mcp/proxy/alpha/sse", None, None).await;
    assert_eq!(response.status(), 401);
    assert_eq!(fixture.hits(), 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn invalid_static_configuration_stops_startup_without_leaking_secrets() {
    let root = TempDir::default();
    let config = json!([
        {"name": "alpha", "url": "http://operator:synthetic-leak-marker@127.0.0.1:1/"},
    ])
    .to_string();
    let mut child = server_command(&root, free_port(), Some(&config))
        .spawn()
        .unwrap();
    let deadline = Instant::now() + STARTUP_TIMEOUT;
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!(
                "server started with invalid MCP_SERVERS:\n{}",
                output(&root)
            );
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    };
    assert!(!status.success());
    let output = output(&root);
    assert!(output.contains("MCP_SERVERS is invalid"), "{output}");
    assert!(output.contains("credentials"), "{output}");
    assert!(!output.contains("synthetic-leak-marker"), "{output}");
}

#[tokio::test(flavor = "multi_thread")]
async fn empty_static_configuration_disables_mcp_without_kubernetes() {
    let server = TestServer::start(Some("[]")).await;
    let token = server.user_token().await;

    let response = server.list_mcps(Some(&token)).await;
    assert_eq!(response.status(), 200);
    let body: Value = response.json().await.unwrap();
    assert_eq!(body, json!({"servers": []}));

    let response = proxy_get(&server, "/v1/mcp/proxy/alpha/sse", Some(&token), None).await;
    assert_eq!(response.status(), 404);
}

#[tokio::test(flavor = "multi_thread")]
async fn unset_static_configuration_keeps_ordinary_routes_working() {
    let mut server = TestServer::start(None).await;
    let token = server.user_token().await;

    // Without MCP_SERVERS the server keeps using Kubernetes discovery, which
    // cannot start here. That must stay confined to the MCP request.
    let _ = server
        .client
        .get(server.url("/v1/list-mcps"))
        .bearer_auth(&token)
        .send()
        .await;

    let response = server
        .client
        .get(server.url("/v1/models"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let response = server.client.get(server.url("/v1/health")).send().await;
    assert_eq!(response.unwrap().status(), 200);
    assert!(server.is_running());
}
