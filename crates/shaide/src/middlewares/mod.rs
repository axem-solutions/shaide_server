pub mod error_response;

use axum::{
    body::Body,
    extract::{FromRequestParts, Request},
    http::HeaderValue,
    middleware::Next,
    response::Response,
};
use chrono::Utc;
use http::{HeaderMap, header::AUTHORIZATION, request::Parts};
use shaide_db::{DbConn, UserDAO, UserRole, api_usage::InsertApiUsageDao};
use tracing::{debug, error};

use crate::{error::ShaideError, services::auth::get_auth_service};

// TODO: no idea why we need this tbh
pub async fn forward_headers_middleware(mut request: Request<Body>, next: Next) -> Response<Body> {
    // Preserve the original host and protocol for proxied requests
    // This ensures the control panel can determine the public-facing URL

    // Detect the actual protocol from the incoming request before borrowing headers mutably
    let proto = if request.uri().scheme_str() == Some("https") {
        HeaderValue::from_static("https")
    } else {
        HeaderValue::from_static("http")
    };

    let headers = request.headers_mut();

    // Only add X-Forwarded-Host if not already present (preserve existing if set by ingress)
    if !headers.contains_key("x-forwarded-host")
        && let Some(host) = headers.get("host").cloned()
    {
        headers.insert("x-forwarded-host", host);
    }

    // Only add X-Forwarded-Proto if not already present
    if !headers.contains_key("x-forwarded-proto") {
        headers.insert("x-forwarded-proto", proto);
    }

    next.run(request).await
}

fn get_bearer_value(headers: &HeaderMap<HeaderValue>) -> Option<String> {
    let header_value = headers.get(AUTHORIZATION)?.to_str().ok()?;
    let (schema, token) = header_value.split_once(' ')?;
    if schema.eq_ignore_ascii_case("bearer") {
        Some(token.to_owned())
    } else {
        None
    }
}

pub(crate) fn get_bearer_value_or_error(parts: &Parts) -> Result<String, ShaideError> {
    let bearer_token = get_bearer_value(&parts.headers);
    if let Some(bearer_token) = bearer_token {
        Ok(bearer_token)
    } else {
        error!(
            method = %parts.method,
            uri = %parts.uri,
            "Request rejected with no bearer token"
        );
        Err(ShaideError::unauthorized(
            "Bearer token not set".to_string(),
        ))
    }
}

#[derive(Clone, Copy)]
pub(crate) enum AccessRequirement {
    User,
    Admin,
    Any,
}

pub(crate) struct AuthorizedRequest {
    pub user: UserDAO,
    pub request_id: i64,
}

pub(crate) fn ensure_access(
    user: &UserDAO,
    requirement: AccessRequirement,
) -> Result<(), ShaideError> {
    let role_allowed = match requirement {
        AccessRequirement::User => user.role == UserRole::User,
        AccessRequirement::Admin => user.role == UserRole::Admin,
        AccessRequirement::Any => true,
    };
    if !role_allowed {
        return Err(ShaideError::unauthorized(
            "Authenticated user does not have the required role".to_owned(),
        ));
    }
    if user.role == UserRole::User && user.expiry <= Utc::now() {
        return Err(ShaideError::unauthorized("Account has expired".to_owned()));
    }
    Ok(())
}

pub(crate) async fn authorize_request(
    parts: &Parts,
    db: &DbConn,
    requirement: AccessRequirement,
) -> Result<AuthorizedRequest, ShaideError> {
    let bearer_token = get_bearer_value_or_error(parts)?;
    let user_id = get_auth_service().validate_access_token(&bearer_token)?;
    let user = db.get_user_by_id(user_id).await?;
    ensure_access(&user, requirement)?;

    let request_id = db
        .insert_api_usage(InsertApiUsageDao {
            route: parts.uri.to_string(),
            user: user.id,
        })
        .await?;
    debug!(
        request_id,
        user_id = user.id,
        role = ?user.role,
        method = %parts.method,
        uri = %parts.uri,
        "Authenticated request"
    );
    Ok(AuthorizedRequest { user, request_id })
}

#[derive(Debug, Clone, Copy)]
pub struct Admin;

impl FromRequestParts<DbConn> for Admin {
    type Rejection = ShaideError;

    async fn from_request_parts(parts: &mut Parts, db: &DbConn) -> Result<Self, Self::Rejection> {
        authorize_request(parts, db, AccessRequirement::Admin).await?;
        Ok(Admin)
    }
}

#[derive(Debug, Clone)]
pub struct AuthUser {
    pub user: UserDAO,
    pub request_id: i64,
}

impl FromRequestParts<DbConn> for AuthUser {
    type Rejection = ShaideError;

    async fn from_request_parts(parts: &mut Parts, db: &DbConn) -> Result<Self, Self::Rejection> {
        let authorized = authorize_request(parts, db, AccessRequirement::User).await?;
        Ok(AuthUser {
            user: authorized.user,
            request_id: authorized.request_id,
        })
    }
}

/// An authenticated account of either role, loaded from the current database.
/// Regular users must be active; administrators have no account expiry.
/// Endpoints with different behavior for users and admins can authorize once
/// and branch on
/// `role` without invoking another extractor or recording duplicate usage.
#[derive(Clone)]
#[allow(
    dead_code,
    reason = "identity fields are available to handlers that need authenticated account details"
)]
pub struct Principal {
    pub user: UserDAO,
    pub role: UserRole,
    pub request_id: i64,
}

impl std::fmt::Debug for Principal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Principal")
            .field("user_id", &self.user.id)
            .field("role", &self.role)
            .field("request_id", &self.request_id)
            .finish()
    }
}

impl FromRequestParts<DbConn> for Principal {
    type Rejection = ShaideError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &DbConn,
    ) -> Result<Self, Self::Rejection> {
        let AuthorizedRequest { user, request_id } =
            authorize_request(parts, state, AccessRequirement::Any).await?;
        Ok(Principal {
            role: user.role,
            user,
            request_id,
        })
    }
}

#[cfg(test)]
mod tests {
    use axum::{
        Json, Router,
        body::{Body, to_bytes},
        routing,
    };
    use chrono::{Duration, Utc};
    use http::{Method, Request, StatusCode, header::AUTHORIZATION};
    use shaide_db::DbConn;
    use temp_testdir::TempDir;
    use tower::ServiceExt;

    use crate::services::auth::{AuthService, init_test_auth_service};

    #[test]
    fn principal_debug_excludes_password_hash() {
        const SYNTHETIC_HASH: &str = "SYNTHETIC-NOT-A-CREDENTIAL-SHD-1070-HASH";
        let mut exposures = Vec::new();
        for role in [shaide_db::UserRole::User, shaide_db::UserRole::Admin] {
            let principal = super::Principal {
                user: shaide_db::UserDAO {
                    id: 1070,
                    username: "synthetic-debug-user".to_owned(),
                    password_hash: SYNTHETIC_HASH.to_owned(),
                    role,
                    expiry: Utc::now(),
                },
                role,
                request_id: 4242,
            };
            for (style, output) in [
                ("ordinary", format!("{principal:?}")),
                ("pretty", format!("{principal:#?}")),
            ] {
                if output.contains(SYNTHETIC_HASH) || output.contains("password") {
                    exposures.push(format!("{role:?} {style}"));
                }
                // Ignore pretty-print whitespace while checking useful identity details.
                let compact: String = output.chars().filter(|c| !c.is_whitespace()).collect();
                assert!(compact.contains("1070"), "user ID missing from Debug");
                assert!(
                    compact.contains(&format!("role:{role:?}")),
                    "role missing from Debug"
                );
                assert!(
                    compact.contains("request_id:4242"),
                    "request ID missing from Debug"
                );
            }
        }
        assert!(
            exposures.is_empty(),
            "Principal Debug exposes the synthetic password hash or password field: {exposures:?}"
        );
    }

    async fn principal_details(principal: super::Principal) -> Json<serde_json::Value> {
        Json(serde_json::json!({
            "user_id": principal.user.id,
            "role": match principal.role {
                shaide_db::UserRole::User => "user",
                shaide_db::UserRole::Admin => "admin",
            },
            "db_role": String::from(principal.user.role),
            "username": principal.user.username,
            "request_id": principal.request_id,
        }))
    }

    /// Synthetic users and tokens in a throwaway database.
    struct Fixture {
        _temp_dir: TempDir,
        db: DbConn,
        user_id: i64,
        admin_id: i64,
        user_token: String,
        admin_token: String,
        /// A valid token for an already-expired account.
        expired_account_token: String,
        /// A token past its own expiry.
        expired_token: String,
        /// A token signed with another secret.
        foreign_token: String,
        /// A valid token for a user ID that does not exist.
        unknown_user_token: String,
    }

    async fn fixture() -> Fixture {
        let auth = init_test_auth_service();
        let temp_dir = TempDir::default();
        let db = DbConn::new(&temp_dir.join("shaide-test.sqlite"))
            .await
            .expect("test database should be created");
        let user_id = db
            .create_user(
                "user".to_owned(),
                String::new(),
                Utc::now() + Duration::days(1),
            )
            .await
            .unwrap();
        let expired_id = db
            .create_user(
                "expired".to_owned(),
                String::new(),
                Utc::now() - Duration::days(1),
            )
            .await
            .unwrap();
        let admin_id = db
            .create_admin("admin".to_owned(), String::new())
            .await
            .unwrap();
        let token = |id| auth.issue_access_token(id, None).unwrap().0;
        Fixture {
            _temp_dir: temp_dir,
            user_token: token(user_id),
            admin_token: token(admin_id),
            expired_account_token: token(expired_id),
            expired_token: auth
                .issue_access_token(user_id, Some(Utc::now() - Duration::hours(1)))
                .unwrap()
                .0,
            foreign_token: AuthService::new("another-test-only-secret-of-32-bytes!")
                .issue_access_token(user_id, None)
                .unwrap()
                .0,
            unknown_user_token: token(admin_id + 100),
            db,
            user_id,
            admin_id,
        }
    }

    impl Fixture {
        fn app(&self) -> Router {
            Router::new()
                .merge(crate::routes::users::users_router(self.db.clone()))
                .merge(crate::routes::models::model_router(self.db.clone()))
                .merge(crate::routes::embedding::embedding_router(self.db.clone()))
                .merge(
                    Router::new()
                        .route("/test/principal", routing::get(principal_details))
                        .with_state(self.db.clone()),
                )
        }

        /// Every way of failing authentication, as `Authorization` values.
        fn rejected_credentials(&self) -> Vec<(&'static str, Option<String>)> {
            vec![
                ("missing header", None),
                ("non-bearer scheme", Some("Basic dXNlcjpwYXNz".to_owned())),
                ("malformed token", Some("Bearer not-a-jwt".to_owned())),
                ("expired token", Some(bearer(&self.expired_token))),
                ("foreign signature", Some(bearer(&self.foreign_token))),
                ("unknown user", Some(bearer(&self.unknown_user_token))),
                ("expired account", Some(bearer(&self.expired_account_token))),
            ]
        }

        async fn status(
            &self,
            method: Method,
            uri: &str,
            authorization: Option<String>,
            body: Option<serde_json::Value>,
        ) -> StatusCode {
            let mut request = Request::builder().method(method).uri(uri);
            if let Some(authorization) = authorization {
                request = request.header(AUTHORIZATION, authorization);
            }
            let request = match body {
                Some(body) => request
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string())),
                None => request.body(Body::empty()),
            }
            .unwrap();
            self.app().oneshot(request).await.unwrap().status()
        }

        async fn principal(&self, token: &str) -> serde_json::Value {
            let response = self
                .app()
                .oneshot(
                    Request::builder()
                        .uri("/test/principal")
                        .header(AUTHORIZATION, bearer(token))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            let details: serde_json::Value =
                serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap())
                    .unwrap();
            let request_id = details["request_id"].as_i64().unwrap();
            let recorded: (i64, String) =
                sqlx::query_as("SELECT user, route FROM api_usage WHERE id = ?")
                    .bind(request_id)
                    .fetch_one(&self.db.pool)
                    .await
                    .unwrap();
            assert_eq!(recorded.0, details["user_id"].as_i64().unwrap());
            assert_eq!(recorded.1, "/test/principal");
            details
        }

        async fn expect_status_and_usage(
            &self,
            method: Method,
            uri: &str,
            token: &str,
            body: Option<serde_json::Value>,
            expected: StatusCode,
            accepted: bool,
        ) {
            let before = self.usage_rows().await;
            assert_eq!(
                self.status(method, uri, Some(bearer(token)), body).await,
                expected
            );
            assert_eq!(self.usage_rows().await, before + i64::from(accepted));
        }

        async fn usage_rows(&self) -> i64 {
            sqlx::query_scalar("SELECT COUNT(*) FROM api_usage")
                .fetch_one(&self.db.pool)
                .await
                .unwrap()
        }
    }

    fn bearer(token: &str) -> String {
        format!("Bearer {token}")
    }

    fn unknown_embedding_model() -> Option<serde_json::Value> {
        Some(serde_json::json!({"model": "no-such-model", "input": "text"}))
    }

    // Characterization of the existing `Admin` endpoint `GET /v1/users`.
    #[tokio::test]
    async fn admin_endpoint_accepts_only_admins() {
        let fixture = fixture().await;
        let get_users =
            |authorization| fixture.status(Method::GET, "/v1/users", authorization, None);

        assert_eq!(
            get_users(Some(bearer(&fixture.admin_token))).await,
            StatusCode::OK
        );
        assert_eq!(
            get_users(Some(bearer(&fixture.user_token))).await,
            StatusCode::UNAUTHORIZED
        );
        for (case, authorization) in fixture.rejected_credentials() {
            assert_eq!(
                get_users(authorization).await,
                StatusCode::UNAUTHORIZED,
                "{case}"
            );
        }
    }

    // Characterization of the existing `AuthUser` endpoint `POST /v1/embeddings`.
    // An unknown model is rejected after authentication, without a provider call.
    #[tokio::test]
    async fn user_endpoint_accepts_only_active_users() {
        let fixture = fixture().await;
        let embed = |authorization| {
            fixture.status(
                Method::POST,
                "/v1/embeddings",
                authorization,
                unknown_embedding_model(),
            )
        };

        assert_eq!(
            embed(Some(bearer(&fixture.user_token))).await,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            embed(Some(bearer(&fixture.admin_token))).await,
            StatusCode::UNAUTHORIZED
        );
        for (case, authorization) in fixture.rejected_credentials() {
            assert_eq!(
                embed(authorization).await,
                StatusCode::UNAUTHORIZED,
                "{case}"
            );
        }
    }

    // Characterization of the existing any-role endpoint `GET /v1/embedding_models`.
    #[tokio::test]
    async fn any_role_endpoint_accepts_users_and_admins() {
        let fixture = fixture().await;
        let list = |authorization| {
            fixture.status(Method::GET, "/v1/embedding_models", authorization, None)
        };

        assert_eq!(
            list(Some(bearer(&fixture.user_token))).await,
            StatusCode::OK
        );
        assert_eq!(
            list(Some(bearer(&fixture.admin_token))).await,
            StatusCode::OK
        );
        for (case, authorization) in fixture.rejected_credentials() {
            assert_eq!(
                list(authorization).await,
                StatusCode::UNAUTHORIZED,
                "{case}"
            );
        }
    }

    // Characterization: each authenticated request is recorded once; rejected
    // ones are not recorded.
    #[tokio::test]
    async fn only_authenticated_requests_are_recorded_once() {
        let fixture = fixture().await;
        for (_, authorization) in fixture.rejected_credentials() {
            fixture
                .status(Method::GET, "/v1/embedding_models", authorization, None)
                .await;
        }
        fixture
            .status(
                Method::GET,
                "/v1/users",
                Some(bearer(&fixture.user_token)),
                None,
            )
            .await;
        assert_eq!(fixture.usage_rows().await, 0);

        fixture
            .status(
                Method::GET,
                "/v1/embedding_models",
                Some(bearer(&fixture.user_token)),
                None,
            )
            .await;
        fixture
            .status(
                Method::GET,
                "/v1/users",
                Some(bearer(&fixture.admin_token)),
                None,
            )
            .await;
        assert_eq!(fixture.usage_rows().await, 2);
    }

    #[tokio::test]
    async fn principal_exposes_the_authenticated_identity_and_role_once() {
        let fixture = fixture().await;
        let request = |token: &str| {
            Request::builder()
                .uri("/test/principal")
                .header(AUTHORIZATION, bearer(token))
                .body(Body::empty())
                .unwrap()
        };

        let user_response = fixture
            .app()
            .oneshot(request(&fixture.user_token))
            .await
            .unwrap();
        let user: serde_json::Value = serde_json::from_slice(
            &to_bytes(user_response.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(user["user_id"], fixture.user_id);
        assert_eq!(user["role"], "user");
        assert!(user["request_id"].as_i64().unwrap() > 0);

        let admin_response = fixture
            .app()
            .oneshot(request(&fixture.admin_token))
            .await
            .unwrap();
        let admin: serde_json::Value = serde_json::from_slice(
            &to_bytes(admin_response.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(admin["user_id"], fixture.admin_id);
        assert_eq!(admin["role"], "admin");
        assert!(admin["request_id"].as_i64().unwrap() > 0);
        assert_ne!(user["request_id"], admin["request_id"]);
        assert_eq!(fixture.usage_rows().await, 2);
    }
    // Characterization: the same token follows DB promotion and demotion.
    #[tokio::test]
    async fn retained_token_follows_current_database_role_and_permissions() {
        let fixture = fixture().await;
        let token = &fixture.user_token;
        for (role, admin_status, user_status) in [
            ("user", StatusCode::UNAUTHORIZED, StatusCode::NOT_FOUND),
            ("admin", StatusCode::OK, StatusCode::UNAUTHORIZED),
            ("user", StatusCode::UNAUTHORIZED, StatusCode::NOT_FOUND),
        ] {
            sqlx::query("UPDATE users SET role = ?, username = ? WHERE id = ?")
                .bind(role)
                .bind(format!("current-{role}"))
                .bind(fixture.user_id)
                .execute(&fixture.db.pool)
                .await
                .unwrap();
            let before = fixture.usage_rows().await;
            let principal = fixture.principal(token).await;
            assert_eq!(principal["user_id"], fixture.user_id);
            assert_eq!(principal["role"], role);
            assert_eq!(principal["db_role"], role);
            assert_eq!(principal["username"], format!("current-{role}"));
            assert_eq!(fixture.usage_rows().await, before + 1);
            fixture
                .expect_status_and_usage(
                    Method::GET,
                    "/v1/users",
                    token,
                    None,
                    admin_status,
                    role == "admin",
                )
                .await;
            fixture
                .expect_status_and_usage(
                    Method::POST,
                    "/v1/embeddings",
                    token,
                    unknown_embedding_model(),
                    user_status,
                    role == "user",
                )
                .await;
            fixture
                .expect_status_and_usage(
                    Method::GET,
                    "/v1/models",
                    token,
                    None,
                    StatusCode::OK,
                    true,
                )
                .await;
        }
    }

    // Account validity is also read again after token issuance.
    #[tokio::test]
    async fn retained_tokens_follow_database_expiry_and_deletion() {
        let fixture = fixture().await;
        sqlx::query("UPDATE users SET expiry = ? WHERE id IN (?, ?)")
            .bind(Utc::now() - Duration::days(1))
            .bind(fixture.user_id)
            .bind(fixture.admin_id)
            .execute(&fixture.db.pool)
            .await
            .unwrap();
        for uri in [
            "/test/principal",
            "/v1/models",
            "/v1/embedding_models",
            "/v1/users",
        ] {
            fixture
                .expect_status_and_usage(
                    Method::GET,
                    uri,
                    &fixture.user_token,
                    None,
                    StatusCode::UNAUTHORIZED,
                    false,
                )
                .await;
            fixture
                .expect_status_and_usage(
                    Method::GET,
                    uri,
                    &fixture.admin_token,
                    None,
                    StatusCode::OK,
                    true,
                )
                .await;
        }
        fixture
            .expect_status_and_usage(
                Method::POST,
                "/v1/embeddings",
                &fixture.user_token,
                unknown_embedding_model(),
                StatusCode::UNAUTHORIZED,
                false,
            )
            .await;
        fixture
            .expect_status_and_usage(
                Method::POST,
                "/v1/embeddings",
                &fixture.admin_token,
                unknown_embedding_model(),
                StatusCode::UNAUTHORIZED,
                false,
            )
            .await;
        sqlx::query("DELETE FROM users WHERE id IN (?, ?)")
            .bind(fixture.user_id)
            .bind(fixture.admin_id)
            .execute(&fixture.db.pool)
            .await
            .unwrap();
        for token in [&fixture.user_token, &fixture.admin_token] {
            for uri in [
                "/test/principal",
                "/v1/models",
                "/v1/embedding_models",
                "/v1/users",
            ] {
                fixture
                    .expect_status_and_usage(
                        Method::GET,
                        uri,
                        token,
                        None,
                        StatusCode::UNAUTHORIZED,
                        false,
                    )
                    .await;
            }
            fixture
                .expect_status_and_usage(
                    Method::POST,
                    "/v1/embeddings",
                    token,
                    unknown_embedding_model(),
                    StatusCode::UNAUTHORIZED,
                    false,
                )
                .await;
        }
    }

    #[tokio::test]
    async fn model_list_response_shapes_and_rejections_are_preserved() {
        let fixture = fixture().await;
        for token in [&fixture.user_token, &fixture.admin_token] {
            for (uri, expected) in [
                (
                    "/v1/models",
                    serde_json::json!({"models": [], "object": "list", "data": []}),
                ),
                (
                    "/v1/models?client_version=test",
                    serde_json::json!({"models": []}),
                ),
                ("/v1/embedding_models", serde_json::json!({"models": []})),
            ] {
                let before = fixture.usage_rows().await;
                let response = fixture
                    .app()
                    .oneshot(
                        Request::builder()
                            .uri(uri)
                            .header(AUTHORIZATION, bearer(token))
                            .body(Body::empty())
                            .unwrap(),
                    )
                    .await
                    .unwrap();
                assert_eq!(response.status(), StatusCode::OK);
                let body: serde_json::Value = serde_json::from_slice(
                    &to_bytes(response.into_body(), usize::MAX).await.unwrap(),
                )
                .unwrap();
                assert_eq!(body, expected);
                assert_eq!(fixture.usage_rows().await, before + 1);
            }
        }
        for (case, credentials) in fixture.rejected_credentials() {
            for uri in [
                "/test/principal",
                "/v1/models",
                "/v1/models?client_version=test",
                "/v1/embedding_models",
            ] {
                let before = fixture.usage_rows().await;
                assert_eq!(
                    fixture
                        .status(Method::GET, uri, credentials.clone(), None)
                        .await,
                    StatusCode::UNAUTHORIZED,
                    "{case}"
                );
                assert_eq!(fixture.usage_rows().await, before);
            }
        }
    }
}
