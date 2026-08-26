//! The shaide_server integration-test harness.
//!
//! Boots the **real** `api_router()` - real middleware, real extractors, real serde, real
//! sqlx/SQLite - against a fresh temporary database per test. The only things faked are the
//! process boundaries: the upstream model provider (see [`fake_upstream`]) is a local server the
//! database points a model row at, so nothing in the server knows it is under test.
//!
//! See `crates/shaide/tests/README.md` for how to write a test on top of this.

#![allow(dead_code, unused_imports)]

pub mod env;
pub mod fake_upstream;
pub mod sse;

use std::{net::SocketAddr, sync::Arc};

use axum::{
    Router,
    body::{Body, Bytes},
    extract::Request,
    http::{HeaderMap, HeaderValue, Method, StatusCode, header},
};
use chrono::{Duration, Utc};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use shaide::services::{auth::get_auth_service, health::HealthState};
use shaide_db::{
    DbConn, InsertModelDAO,
    models::{NativeFimModeDao, SetModelLimitsDao},
};
use temp_testdir::TempDir;
use tokio::{net::TcpListener, sync::OnceCell, task::JoinHandle};
use tower::ServiceExt;

pub use fake_upstream::{FakeUpstream, RecordedRequest, UpstreamResponse};
pub use sse::{SseEvent, SseStream, SseTranscript};

/// The chat model every server starts with, already pointed at the fake upstream.
pub const DEFAULT_MODEL: &str = "shaide-test-model";

/// The password every harness-created user gets, the admin included. Tests that need to drive
/// `POST /v1/login` can log in with it.
pub const TEST_PASSWORD: &str = "integration-test-password";

/// Argon2 is deliberately expensive, and a server is booted per test. Hashing the shared test
/// password once per binary keeps that cost off every `TestServer::start()` while still leaving
/// the password verifiable, so `/v1/login` stays exercisable.
async fn test_password_hash() -> &'static String {
    static PASSWORD_HASH: OnceCell<String> = OnceCell::const_new();
    PASSWORD_HASH
        .get_or_init(|| async {
            get_auth_service()
                .hash_password(TEST_PASSWORD.to_owned())
                .await
                .expect("the test password should hash")
        })
        .await
}

/// Reading the CPU inventory takes long enough to notice once per test, and the value is
/// immutable, so the whole binary shares one.
fn health_state() -> Arc<HealthState> {
    static HEALTH_STATE: std::sync::LazyLock<Arc<HealthState>> =
        std::sync::LazyLock::new(|| Arc::new(HealthState::new()));
    Arc::clone(&HEALTH_STATE)
}

/// A user created through the harness: a row in `users` plus a signed access token for it, which
/// is what the `AuthUser` extractor validates.
#[derive(Debug, Clone)]
pub struct TestUser {
    pub id: i64,
    pub username: String,
    /// The plaintext password behind the user, for driving `POST /v1/login`.
    pub password: String,
    /// A ready-made JWT for the user, so most tests never have to log in.
    pub token: String,
}

struct ServedApp {
    address: SocketAddr,
    server: JoinHandle<()>,
}

/// One server under test: its own database, its own fake upstream, no state shared with any other
/// [`TestServer`] in the process.
pub struct TestServer {
    db: DbConn,
    router: Router,
    upstream: FakeUpstream,
    admin_id: i64,
    admin_token: String,
    http: reqwest::Client,
    served: OnceCell<ServedApp>,
    // Declared last: the database pool above must drop before the directory holding it.
    _db_dir: TempDir,
}

impl TestServer {
    /// Boots a server with a fresh database, a fake upstream and [`DEFAULT_MODEL`] seeded against
    /// it. Use [`TestServer::without_default_model`] when a test wants to define its own models.
    pub async fn start() -> Self {
        let server = Self::without_default_model().await;
        let model = server.upstream_model(DEFAULT_MODEL);
        server.seed_model(model).await;
        server
    }

    /// Boots a server with no models seeded at all.
    pub async fn without_default_model() -> Self {
        env::test_env();

        let db_dir = TempDir::default();
        let db = DbConn::new(&db_dir.join("shaide-server.sqlite"))
            .await
            .expect("test database should be created and migrated");

        // The same row `shaide::ensure_admin` writes on a real boot, but reusing the shared
        // password hash instead of paying for Argon2 once per test.
        let admin_id = db
            .create_admin(
                shaide::ADMIN_USERNAME.to_owned(),
                test_password_hash().await.clone(),
            )
            .await
            .expect("admin user should be created");
        let admin_token = get_auth_service()
            .issue_access_token(admin_id)
            .expect("an access token should be issued for the admin");

        let upstream = FakeUpstream::start().await;
        let router = shaide::with_api_middleware(
            shaide::api_router(health_state(), db.clone())
                .await
                .layer(tower_http::cors::CorsLayer::permissive()),
        );

        Self {
            db,
            router,
            upstream,
            admin_id,
            admin_token,
            http: reqwest::Client::new(),
            served: OnceCell::new(),
            _db_dir: db_dir,
        }
    }

    /// The database behind the server — for seeding rows a route cannot create, and for asserting
    /// what a request actually wrote.
    pub fn db(&self) -> &DbConn {
        &self.db
    }

    pub fn upstream(&self) -> &FakeUpstream {
        &self.upstream
    }

    /// The admin user's id — the row `shaide::ADMIN_USERNAME` owns in this server's database.
    pub fn admin_id(&self) -> i64 {
        self.admin_id
    }

    /// A signed access token for the admin.
    pub fn admin_token(&self) -> &str {
        &self.admin_token
    }

    /// Mints an access token for any user id, the way `POST /v1/login` does.
    pub fn access_token(&self, user_id: i64) -> String {
        get_auth_service()
            .issue_access_token(user_id)
            .expect("an access token should be issued")
    }

    // ---- request/response mode (`tower::ServiceExt::oneshot`) ----

    /// A client authenticated as the admin.
    pub fn admin(&self) -> ApiClient {
        self.client(Some(self.admin_token.clone()))
    }

    /// A client authenticated as `user`.
    pub fn user(&self, user: &TestUser) -> ApiClient {
        self.client(Some(user.token.clone()))
    }

    /// A client that sends no `Authorization` header.
    pub fn anonymous(&self) -> ApiClient {
        self.client(None)
    }

    /// A client with an arbitrary bearer token — for the negative auth cases.
    pub fn with_token(&self, token: impl Into<String>) -> ApiClient {
        self.client(Some(token.into()))
    }

    fn client(&self, token: Option<String>) -> ApiClient {
        ApiClient {
            router: self.router.clone(),
            token,
        }
    }

    // ---- streaming mode (real socket on an ephemeral port) ----

    /// A client that talks to the server over a real TCP connection. Needed whenever the response
    /// is streamed: `oneshot` hands back a whole response, so SSE can only be observed here.
    ///
    /// The listener starts on first use and lives as long as the [`TestServer`].
    pub async fn live_admin(&self) -> LiveClient {
        self.live(Some(self.admin_token.clone())).await
    }

    pub async fn live_user(&self, user: &TestUser) -> LiveClient {
        self.live(Some(user.token.clone())).await
    }

    pub async fn live_anonymous(&self) -> LiveClient {
        self.live(None).await
    }

    async fn live(&self, token: Option<String>) -> LiveClient {
        LiveClient {
            base_url: format!("http://{}", self.address().await),
            http: self.http.clone(),
            token,
        }
    }

    /// The address the server is listening on, starting the listener if it is not up yet.
    pub async fn address(&self) -> SocketAddr {
        self.served
            .get_or_init(|| async {
                let listener = TcpListener::bind(("127.0.0.1", 0))
                    .await
                    .expect("test server should bind an ephemeral port");
                let address = listener
                    .local_addr()
                    .expect("bound listener should report its address");
                let router = self.router.clone();
                let server = tokio::spawn(async move {
                    let _ = axum::serve(
                        listener,
                        router.into_make_service_with_connect_info::<SocketAddr>(),
                    )
                    .await;
                });
                ServedApp { address, server }
            })
            .await
            .address
    }

    // ---- fixtures ----

    /// Creates a non-admin user with a unique username, [`TEST_PASSWORD`] as its password, and a
    /// ready-made access token.
    pub async fn create_user(&self) -> TestUser {
        self.create_user_with_username(&format!("test-user-{}", uuid::Uuid::new_v4()))
            .await
    }

    pub async fn create_user_with_username(&self, username: &str) -> TestUser {
        self.create_user_with_expiry(username, Utc::now() + Duration::days(1))
            .await
    }

    /// Creates a user whose account expires at `expiry`.
    pub async fn create_user_with_expiry(
        &self,
        username: &str,
        expiry: chrono::DateTime<Utc>,
    ) -> TestUser {
        let id = self
            .db
            .create_user(
                username.to_owned(),
                test_password_hash().await.clone(),
                expiry,
            )
            .await
            .expect("test user should be created");
        TestUser {
            id,
            username: username.to_owned(),
            password: TEST_PASSWORD.to_owned(),
            token: self.access_token(id),
        }
    }

    /// A model definition pointed at this server's fake upstream. Adjust the returned DAO and pass
    /// it to [`TestServer::seed_model`] to seed a model with, say, vision limits.
    pub fn upstream_model(&self, name: &str) -> InsertModelDAO {
        InsertModelDAO {
            name: name.to_owned(),
            variant: name.to_owned(),
            chat_completions_endpoint: self.upstream.chat_completions_url(),
            completions_endpoint: Some(self.upstream.url("/v1/completions")),
            responses_endpoint: Some(self.upstream.url("/v1/responses")),
            api_schema: "open_ai".to_owned(),
            daily_input_token_limit: None,
            daily_output_token_limit: None,
            supports_images: false,
            reasoning_effort_values: Vec::new().into(),
            max_images_per_request: None,
            max_image_bytes: None,
            max_image_width_px: None,
            max_image_height_px: None,
            max_generated_tokens: 512,
            context_size: 32_768,
            // "axem" is the plain OpenAI-compatible provider: it posts straight to
            // `chat_completions_endpoint` with no cloud credentials in the way.
            platform: Some("axem".to_owned()),
            native_fim_mode: None::<NativeFimModeDao>,
            fim_prompt_template: None,
        }
    }

    pub async fn seed_model(&self, model: InsertModelDAO) -> i64 {
        self.db
            .create_model(model)
            .await
            .expect("test model should be created")
    }

    /// Sets a model's daily token limits — the governance knob the limit tests drive.
    pub async fn set_model_limits(
        &self,
        name: &str,
        daily_input_token_limit: Option<i64>,
        daily_output_token_limit: Option<i64>,
    ) {
        self.db
            .set_model_limits(SetModelLimitsDao {
                name: name.to_owned(),
                daily_input_token_limit,
                daily_output_token_limit,
            })
            .await
            .expect("model limits should be set");
    }

    /// Scripts the fake upstream's next chat completion response.
    pub fn upstream_chat(&self, response: UpstreamResponse) {
        self.upstream.push("/v1/chat/completions", response);
    }

    /// Scripts a well-behaved streamed answer from the upstream for [`DEFAULT_MODEL`].
    pub fn upstream_says(&self, chunks: &[&str]) {
        self.upstream_chat(UpstreamResponse::sse(SseTranscript::happy_path(
            DEFAULT_MODEL,
            chunks,
        )));
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        // Stop serving before the fields drop: the temporary directory goes with them, and the
        // listener must not be handling a request out of a database that is being deleted.
        if let Some(served) = self.served.get() {
            served.server.abort();
        }
    }
}

/// Request/response client: drives the router directly through `tower`, no socket involved.
#[derive(Clone)]
pub struct ApiClient {
    router: Router,
    token: Option<String>,
}

impl ApiClient {
    pub async fn get(&self, path: &str) -> TestResponse {
        self.send(self.request(Method::GET, path).body(Body::empty()).unwrap())
            .await
    }

    pub async fn delete(&self, path: &str) -> TestResponse {
        self.send(
            self.request(Method::DELETE, path)
                .body(Body::empty())
                .unwrap(),
        )
        .await
    }

    pub async fn post<T: Serialize>(&self, path: &str, body: &T) -> TestResponse {
        self.json_request(Method::POST, path, body).await
    }

    pub async fn patch<T: Serialize>(&self, path: &str, body: &T) -> TestResponse {
        self.json_request(Method::PATCH, path, body).await
    }

    pub async fn delete_with_body<T: Serialize>(&self, path: &str, body: &T) -> TestResponse {
        self.json_request(Method::DELETE, path, body).await
    }

    /// Escape hatch for a request the helpers above cannot express (odd headers, raw bodies).
    /// The `Authorization` header is still added when the client has a token.
    pub async fn send(&self, request: Request<Body>) -> TestResponse {
        let response = self
            .router
            .clone()
            .oneshot(request)
            .await
            .expect("the router is infallible");
        let status = response.status();
        let headers = response.headers().clone();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body should be readable");
        TestResponse {
            status,
            headers,
            body,
        }
    }

    pub fn request(&self, method: Method, path: &str) -> axum::http::request::Builder {
        let builder = Request::builder().method(method).uri(path);
        match &self.token {
            Some(token) => builder.header(header::AUTHORIZATION, format!("Bearer {token}")),
            None => builder,
        }
    }

    async fn json_request<T: Serialize>(
        &self,
        method: Method,
        path: &str,
        body: &T,
    ) -> TestResponse {
        let request = self
            .request(method, path)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                serde_json::to_vec(body).expect("request body should serialize"),
            ))
            .unwrap();
        self.send(request).await
    }
}

/// A complete response, body already read.
#[derive(Debug, Clone)]
pub struct TestResponse {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: Bytes,
}

impl TestResponse {
    pub fn header(&self, name: header::HeaderName) -> Option<&HeaderValue> {
        self.headers.get(name)
    }

    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    pub fn json_value(&self) -> Value {
        serde_json::from_slice(&self.body)
            .unwrap_or_else(|error| panic!("response should be JSON ({error}): {}", self.text()))
    }

    /// The body deserialized into the type the server declares it returns — which is the point:
    /// a shape change breaks the test here rather than silently at a client.
    pub fn json<T: DeserializeOwned>(&self) -> T {
        serde_json::from_slice(&self.body).unwrap_or_else(|error| {
            panic!(
                "response should deserialize into {} ({error}): {}",
                std::any::type_name::<T>(),
                self.text()
            )
        })
    }

    #[track_caller]
    pub fn assert_status(self, expected: StatusCode) -> Self {
        assert_eq!(
            self.status,
            expected,
            "unexpected status, body was: {}",
            self.text()
        );
        self
    }

    #[track_caller]
    pub fn assert_ok(self) -> Self {
        self.assert_status(StatusCode::OK)
    }
}

/// Client that talks over a real socket. Use it whenever the response is streamed.
#[derive(Clone)]
pub struct LiveClient {
    base_url: String,
    http: reqwest::Client,
    token: Option<String>,
}

impl LiveClient {
    pub fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base_url)
    }

    pub async fn get(&self, path: &str) -> reqwest::Response {
        self.authorize(self.http.get(self.url(path)))
            .send()
            .await
            .expect("request to the test server should succeed")
    }

    pub async fn post<T: Serialize>(&self, path: &str, body: &T) -> reqwest::Response {
        self.authorize(self.http.post(self.url(path)).json(body))
            .send()
            .await
            .expect("request to the test server should succeed")
    }

    /// POSTs `body` and returns the response as a stream to read events off.
    pub async fn sse<T: Serialize>(&self, path: &str, body: &T) -> SseStream {
        SseStream::new(self.post(path, body).await)
    }

    fn authorize(&self, builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.token {
            Some(token) => builder.bearer_auth(token),
            None => builder,
        }
    }
}
