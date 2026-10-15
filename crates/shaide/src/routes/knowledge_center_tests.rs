use crate::{
    routes::knowledge_center::knowledge_center_router,
    services::auth::init_test_auth_service,
    services::knowledge_center::{KcClient, KcConfig},
};
use axum::{
    Json, Router,
    body::{Body, to_bytes},
    extract::Request,
    response::IntoResponse,
};
use http::{Method, StatusCode};
use serde_json::{Value, json};
use shaide_db::{
    DbConn, embedding_models::InsertEmbeddingModelDao, organizations::InsertOrganizationDao,
};
use std::sync::{Arc, Mutex};
use temp_testdir::TempDir;
use tower::ServiceExt;

type RecordedCall = (Method, String, http::HeaderMap, Vec<u8>);
#[derive(Clone, Default)]
struct Calls(
    Arc<Mutex<Vec<RecordedCall>>>,
    Arc<Mutex<&'static str>>,
    Arc<tokio::sync::Notify>,
    Arc<tokio::sync::Notify>,
    Arc<tokio::sync::Notify>,
    Arc<Mutex<Option<(Method, Value)>>>,
);
async fn fake_kc(
    axum::extract::State(calls): axum::extract::State<Calls>,
    request: Request,
) -> axum::response::Response {
    let (parts, body) = request.into_parts();
    let bytes = to_bytes(body, 1024 * 1024).await.unwrap();
    calls.0.lock().unwrap().push((
        parts.method.clone(),
        parts.uri.to_string(),
        parts.headers.clone(),
        bytes.to_vec(),
    ));
    if parts.headers.get("authorization").unwrap()
        != "Bearer synthetic-internal-token-32-bytes-long"
    {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    if parts.uri.path().starts_with("/internal/v1/collections/")
        && let Some((method, body)) = calls.5.lock().unwrap().as_ref()
        && method == parts.method
    {
        return Json(body.clone()).into_response();
    }
    let fault = *calls.1.lock().unwrap();
    if (fault == "upload_failure" && parts.uri.path().starts_with("/internal/v1/storage/upload/"))
        || (fault == "ingest_failure" && parts.uri.path() == "/internal/v1/ingest")
    {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error_code":"storage_unavailable"})),
        )
            .into_response();
    }
    if fault == "embedding_quota" && parts.uri.path() == "/internal/v1/ingest" {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({"error_code":"embedding_quota"})),
        )
            .into_response();
    }
    if fault == "collection_failure" && parts.uri.path().starts_with("/internal/v1/collections/") {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error":"storage_unavailable"})),
        )
            .into_response();
    }
    if fault == "empty_ingest" && parts.uri.path() == "/internal/v1/ingest" {
        return Json(json!({"status":"processed","pages":0,"chunks":0,"size":9,"mime_type":"application/pdf"})).into_response();
    }
    if fault == "upload_delay" && parts.uri.path().starts_with("/internal/v1/storage/upload/") {
        let accepted = calls.clone();
        let remote = tokio::spawn(async move {
            accepted.2.notify_one();
            accepted.3.notified().await;
            accepted.4.notify_one();
        });
        remote.await.unwrap();
    }
    match parts.uri.path() {
        "/internal/v1/storage/presigned-url"=>{let v:Value=serde_json::from_slice(&bytes).unwrap();Json(json!({"presigned_url":format!("/s3/{}/{}?X-Amz-Expires=300&X-Amz-Signature=synthetic",v["collection"].as_str().unwrap(),v["key"].as_str().unwrap())})).into_response()},
        "/internal/v1/ingest"=>Json(json!({"status":"processed","pages":1,"chunks":1,"size":9,"mime_type":"application/pdf"})).into_response(),
        p if p.starts_with("/internal/v1/storage/objects/")=>Json(json!({"exists":true,"size":9,"content_type":"application/pdf"})).into_response(),
        p if p.starts_with("/internal/v1/storage/upload/")=>StatusCode::OK.into_response(),
        p if p.contains("/files/")=>Json(json!({"deleted":true})).into_response(),
        _=>Json(json!({})).into_response()
    }
}
struct Fixture {
    _temp: TempDir,
    db: DbConn,
    app: Router,
    calls: Calls,
    task: tokio::task::JoinHandle<()>,
    admin: String,
    member: String,
    stranger: String,
    id: i64,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}
async fn fixture() -> Fixture {
    let calls = Calls::default();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let app = Router::new().fallback(fake_kc).with_state(calls.clone());
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let temp = TempDir::default();
    let db = DbConn::new(&temp.join("facade.sqlite")).await.unwrap();
    let admin = db
        .create_admin("synthetic-admin".into(), "unused".into())
        .await
        .unwrap();
    let member = db
        .create_user(
            "synthetic-member".into(),
            "unused".into(),
            chrono::Utc::now() + chrono::Duration::days(1),
        )
        .await
        .unwrap();
    let stranger = db
        .create_user(
            "synthetic-stranger".into(),
            "unused".into(),
            chrono::Utc::now() + chrono::Duration::days(1),
        )
        .await
        .unwrap();
    let model = db
        .insert_embedding_model(InsertEmbeddingModelDao {
            name: "synthetic".into(),
            url: "http://invalid.example".into(),
            vector_size: 8,
            platform: None,
            api_schema: None,
            daily_input_token_limit: None,
        })
        .await
        .unwrap();
    let id = db
        .create_organization(InsertOrganizationDao {
            name: "synthetic".into(),
            description: "".into(),
            can_users_upload: true,
            embedding_model_id: model,
            user_ids: vec![member],
        })
        .await
        .unwrap();
    let client = KcClient::new(
        KcConfig::parse(
            Some(&format!("http://{address}")),
            Some("synthetic-internal-token-32-bytes-long"),
            Some("synthetic-webhook-token-32-bytes-long"),
            true,
        )
        .unwrap()
        .unwrap(),
    )
    .unwrap();
    let app = knowledge_center_router(db.clone(), Some(client));
    let auth = init_test_auth_service();
    let token = |id| auth.issue_access_token(id, None).unwrap().0;
    Fixture {
        _temp: temp,
        db,
        app,
        calls,
        task,
        admin: token(admin),
        member: token(member),
        stranger: token(stranger),
        id,
    }
}
async fn request(
    app: &Router,
    method: Method,
    path: &str,
    token: Option<&str>,
    body: Value,
) -> (StatusCode, Value) {
    let mut r = http::Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json");
    if let Some(token) = token {
        r = r.header("authorization", format!("Bearer {token}"));
    }
    let response = app
        .clone()
        .oneshot(r.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}
#[tokio::test]
async fn presign_is_scoped_and_bytes_only_reach_authenticated_kc() {
    let f = fixture().await;
    let body = json!({"organization_id":f.id,"file_name":"synthetic.txt","content_type":"application/pdf"});
    assert_eq!(
        request(
            &f.app,
            Method::POST,
            "/v1/object-storage/presigned-url",
            None,
            body.clone()
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        request(
            &f.app,
            Method::POST,
            "/v1/object-storage/presigned-url",
            Some(&f.stranger),
            body.clone()
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert!(f.calls.0.lock().unwrap().is_empty());
    let (status, v) = request(
        &f.app,
        Method::POST,
        "/v1/object-storage/presigned-url",
        Some(&f.member),
        body,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let url = v["presigned_url"].as_str().unwrap();
    let response = f
        .app
        .clone()
        .oneshot(
            http::Request::builder()
                .method("PUT")
                .uri(url)
                .header("content-type", "application/pdf")
                .header("content-length", "9")
                .body(Body::from("synthetic"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        f.app
            .clone()
            .oneshot(
                http::Request::builder()
                    .method("GET")
                    .uri(url)
                    .body(Body::empty())
                    .unwrap()
            )
            .await
            .unwrap()
            .status(),
        StatusCode::METHOD_NOT_ALLOWED
    );
    let calls = f.calls.0.lock().unwrap();
    let presign: Value = serde_json::from_slice(&calls[0].3).unwrap();
    assert_eq!(presign["expires_in_seconds"], 300);
    assert_eq!(calls[1].0, Method::PUT);
    assert_eq!(calls[1].3, b"synthetic");
    assert!(
        calls[1]
            .1
            .starts_with("/internal/v1/storage/upload/collection-")
    );
    assert!(
        calls[1]
            .1
            .ends_with("?X-Amz-Expires=300&X-Amz-Signature=synthetic")
    );
    assert_eq!(
        calls[1].2["authorization"],
        "Bearer synthetic-internal-token-32-bytes-long"
    );
}
#[tokio::test]
async fn collection_listing_preserves_admin_and_member_visibility_without_kc() {
    let f = fixture().await;
    let app = knowledge_center_router(f.db.clone(), None);
    let (status, v) = request(
        &app,
        Method::GET,
        "/v1/rag/organization-collection",
        Some(&f.admin),
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(v["organization"][0]["users"].is_array());
    let (_, v) = request(
        &app,
        Method::GET,
        "/v1/rag/organization-collection",
        Some(&f.member),
        Value::Null,
    )
    .await;
    assert!(v["organization"][0]["users"].is_null());
    let (_, v) = request(
        &app,
        Method::GET,
        "/v1/rag/organization-collection",
        Some(&f.stranger),
        Value::Null,
    )
    .await;
    assert_eq!(v["organization"], json!([]));
    let body = json!({"organization_id":f.id,"file_name":"synthetic.txt","content_type":"application/pdf"});
    assert_eq!(
        request(
            &app,
            Method::POST,
            "/v1/object-storage/presigned-url",
            Some(&f.member),
            body
        )
        .await
        .0,
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert!(f.db.list_files(f.id).await.unwrap().is_empty());
}
async fn reserve(f: &Fixture) -> String {
    let (status,v)=request(&f.app,Method::POST,"/v1/object-storage/presigned-url",Some(&f.member),json!({"organization_id":f.id,"file_name":"synthetic.pdf","content_type":"application/pdf"})).await;
    assert_eq!(status, StatusCode::OK);
    v["presigned_url"].as_str().unwrap().into()
}
async fn put(app: &Router, url: &str) -> StatusCode {
    app.clone()
        .oneshot(
            http::Request::builder()
                .method("PUT")
                .uri(url)
                .header("content-type", "application/pdf")
                .header("content-length", "9")
                .body(Body::from("synthetic"))
                .unwrap(),
        )
        .await
        .unwrap()
        .status()
}
fn notification(id: i64, hash: &str) -> Value {
    json!({"Records":[{"eventName":"s3:ObjectCreated:Put","s3":{"bucket":{"name":format!("collection-{id}")},"object":{"key":hash,"size":999999999}}}]})
}
fn file_hash(url: &str) -> &str {
    url.split('?').next().unwrap().rsplit('/').next().unwrap()
}
#[tokio::test]
async fn forged_query_and_wrong_collection_do_not_consume_reservation() {
    let f = fixture().await;
    let url = reserve(&f).await;
    assert_eq!(
        put(
            &f.app,
            &url.replace("Signature=synthetic", "Signature=forged")
        )
        .await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        put(&f.app, &url.replace("collection-1", "collection-999")).await,
        StatusCode::NOT_FOUND
    );
    assert_eq!(f.calls.0.lock().unwrap().len(), 1);
    assert_eq!(put(&f.app, &url).await, StatusCode::OK);
    assert_eq!(put(&f.app, &url).await, StatusCode::CONFLICT);
    assert_eq!(f.calls.0.lock().unwrap().len(), 2);
}
#[tokio::test]
async fn event_uses_trusted_kc_metadata_and_claims_ingestion_once() {
    let f = fixture().await;
    let url = reserve(&f).await;
    let hash = file_hash(&url);
    assert_eq!(put(&f.app, &url).await, StatusCode::OK);
    let body = notification(f.id, hash);
    assert_eq!(
        request(
            &f.app,
            Method::POST,
            "/v1/object-storage/event",
            None,
            body.clone()
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        request(
            &f.app,
            Method::POST,
            "/v1/object-storage/event",
            Some("forged"),
            body.clone()
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(f.calls.0.lock().unwrap().len(), 2);
    let (a, b) = tokio::join!(
        request(
            &f.app,
            Method::POST,
            "/v1/object-storage/event",
            Some("synthetic-webhook-token-32-bytes-long"),
            body.clone()
        ),
        request(
            &f.app,
            Method::POST,
            "/v1/object-storage/event",
            Some("synthetic-webhook-token-32-bytes-long"),
            body.clone()
        )
    );
    assert_eq!(a.0, StatusCode::OK);
    assert_eq!(b.0, StatusCode::OK);
    let file = f.db.get_file(f.id, hash).await.unwrap();
    assert_eq!(file.status, shaide_db::files::FileStatus::Processed);
    assert_eq!(file.size, Some(9));
    let count = || {
        f.calls
            .0
            .lock()
            .unwrap()
            .iter()
            .filter(|c| c.1 == "/internal/v1/ingest")
            .count()
    };
    assert_eq!(count(), 1);
    assert_eq!(
        request(
            &f.app,
            Method::POST,
            "/v1/object-storage/event",
            Some("synthetic-webhook-token-32-bytes-long"),
            body
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(count(), 1);
    let calls = f.calls.0.lock().unwrap();
    let ingest = &calls
        .iter()
        .find(|c| c.1 == "/internal/v1/ingest")
        .unwrap()
        .3;
    let v: Value = serde_json::from_slice(ingest).unwrap();
    assert_eq!(v["key"], hash);
    assert_eq!(v["file_hash"], hash);
    assert_eq!(
        v["embedding_model"],
        json!({"name":"synthetic","vector_size":8})
    );
}
#[tokio::test]
async fn rejected_or_incomplete_ingestion_is_durable_and_blocks_delete() {
    for fault in ["ingest_failure", "empty_ingest", "embedding_quota"] {
        let f = fixture().await;
        let url = reserve(&f).await;
        assert_eq!(put(&f.app, &url).await, StatusCode::OK);
        *f.calls.1.lock().unwrap() = fault;
        let hash = file_hash(&url);
        assert!(
            !request(
                &f.app,
                Method::POST,
                "/v1/object-storage/event",
                Some("synthetic-webhook-token-32-bytes-long"),
                notification(f.id, hash)
            )
            .await
            .0
            .is_success()
        );
        let other = DbConn::new(&f._temp.join("facade.sqlite")).await.unwrap();
        assert_eq!(
            other.get_file(f.id, hash).await.unwrap().status,
            shaide_db::files::FileStatus::ProcessingFailed
        );
        let state: String =
            sqlx::query_scalar("SELECT state FROM file_operations WHERE file_hash=?")
                .bind(hash)
                .fetch_one(&other.pool)
                .await
                .unwrap();
        assert_eq!(state, "recovery_required");
        assert_eq!(
            request(
                &f.app,
                Method::DELETE,
                "/v1/rag/organization-collection/file",
                Some(&f.member),
                json!({"organization_id":f.id,"file_hash":hash})
            )
            .await
            .0,
            StatusCode::CONFLICT
        );
        assert_eq!(
            request(
                &f.app,
                Method::DELETE,
                "/v1/rag/organization-collection",
                Some(&f.admin),
                json!({"id":f.id})
            )
            .await
            .0,
            StatusCode::CONFLICT
        );
    }
}
#[tokio::test]
async fn event_arriving_before_upload_ack_is_processed_after_ack() {
    let f = fixture().await;
    let url = reserve(&f).await;
    *f.calls.1.lock().unwrap() = "upload_delay";
    let app = f.app.clone();
    let u = url.clone();
    let upload = tokio::spawn(async move { put(&app, &u).await });
    tokio::time::timeout(std::time::Duration::from_secs(30), f.calls.2.notified())
        .await
        .unwrap();
    assert_eq!(
        request(
            &f.app,
            Method::POST,
            "/v1/object-storage/event",
            Some("synthetic-webhook-token-32-bytes-long"),
            notification(f.id, file_hash(&url))
        )
        .await
        .0,
        StatusCode::OK
    );
    f.calls.3.notify_one();
    assert_eq!(upload.await.unwrap(), StatusCode::OK);
    assert_eq!(
        f.db.get_file(f.id, file_hash(&url)).await.unwrap().status,
        shaide_db::files::FileStatus::Processed
    );
}
#[tokio::test]
async fn cancellation_leaves_a_fence_even_when_remote_handler_finishes() {
    let f = fixture().await;
    let url = reserve(&f).await;
    *f.calls.1.lock().unwrap() = "upload_delay";
    let app = f.app.clone();
    let u = url.clone();
    let upload = tokio::spawn(async move { put(&app, &u).await });
    tokio::time::timeout(std::time::Duration::from_secs(30), f.calls.2.notified())
        .await
        .unwrap();
    assert_eq!(f.calls.0.lock().unwrap().len(), 2);
    upload.abort();
    assert!(upload.await.unwrap_err().is_cancelled());
    f.calls.3.notify_one();
    tokio::time::timeout(std::time::Duration::from_secs(30), f.calls.4.notified())
        .await
        .unwrap();
    let other = DbConn::new(&f._temp.join("facade.sqlite")).await.unwrap();
    assert!(other.start_upload(f.id, file_hash(&url)).await.is_err());
    assert_eq!(
        request(
            &f.app,
            Method::DELETE,
            "/v1/rag/organization-collection",
            Some(&f.admin),
            json!({"id":f.id})
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
}
#[tokio::test]
async fn collection_crud_membership_and_file_delete_follow_the_kc_contract() {
    let f = fixture().await;
    let body = json!({"organization_name":"synthetic-created","organization_description":"owned fixture","users":[],"can_users_upload":false,"embedding_model_id":1});
    assert_eq!(
        request(
            &f.app,
            Method::POST,
            "/v1/rag/organization-collection",
            Some(&f.member),
            body.clone()
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let (status, v) = request(
        &f.app,
        Method::POST,
        "/v1/rag/organization-collection",
        Some(&f.admin),
        body,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let id = v["id"].as_i64().unwrap();
    assert_eq!(
        request(
            &f.app,
            Method::PATCH,
            "/v1/rag/organization-collection",
            Some(&f.admin),
            json!({"organization_id":id,"name":"renamed"})
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        request(
            &f.app,
            Method::PATCH,
            "/v1/rag/organization-collection/membership",
            Some(&f.admin),
            json!({"organization_id":id,"user_ids":[2,2]})
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        f.db.list_organization_member_ids(id).await.unwrap(),
        vec![2]
    );
    assert_eq!(
        request(
            &f.app,
            Method::DELETE,
            "/v1/rag/organization-collection/membership",
            Some(&f.admin),
            json!({"organization_id":id,"user_ids":[2]})
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        request(
            &f.app,
            Method::DELETE,
            "/v1/rag/organization-collection",
            Some(&f.admin),
            json!({"id":id})
        )
        .await
        .0,
        StatusCode::OK
    );
    assert!(f.db.get_organization(id).await.is_err());
    let url = reserve(&f).await;
    assert_eq!(put(&f.app, &url).await, StatusCode::OK);
    let hash = file_hash(&url);
    assert_eq!(
        request(
            &f.app,
            Method::POST,
            "/v1/object-storage/event",
            Some("synthetic-webhook-token-32-bytes-long"),
            notification(f.id, hash)
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        request(
            &f.app,
            Method::DELETE,
            "/v1/rag/organization-collection/file",
            Some(&f.stranger),
            json!({"organization_id":f.id,"file_hash":hash})
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(
            &f.app,
            Method::DELETE,
            "/v1/rag/organization-collection/file",
            Some(&f.member),
            json!({"organization_id":f.id,"file_hash":hash})
        )
        .await
        .0,
        StatusCode::OK
    );
    assert!(f.db.get_file(f.id, hash).await.is_err());
    assert!(
        f.calls
            .0
            .lock()
            .unwrap()
            .iter()
            .any(|c| c.0 == Method::DELETE
                && c.1 == format!("/internal/v1/collections/collection-{}/files/{hash}", f.id))
    );
}
#[tokio::test]
async fn upload_limits_filename_policy_and_remote_error_never_succeed_silently() {
    let f = fixture().await;
    for (name, mime) in [
        ("../escape.pdf", "application/pdf"),
        ("synthetic.exe", "application/octet-stream"),
        ("bad\nname.pdf", "application/pdf"),
    ] {
        assert_eq!(
            request(
                &f.app,
                Method::POST,
                "/v1/object-storage/presigned-url",
                Some(&f.member),
                json!({"organization_id":f.id,"file_name":name,"content_type":mime})
            )
            .await
            .0,
            StatusCode::BAD_REQUEST
        );
    }
    assert!(f.db.list_files(f.id).await.unwrap().is_empty());
    let url = reserve(&f).await;
    let response = f
        .app
        .clone()
        .oneshot(
            http::Request::builder()
                .method("PUT")
                .uri(&url)
                .header("content-type", "application/pdf")
                .header("content-length", (16 * 1024 * 1024 + 1).to_string())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(f.calls.0.lock().unwrap().len(), 1);
    *f.calls.1.lock().unwrap() = "upload_failure";
    assert_eq!(put(&f.app, &url).await, StatusCode::SERVICE_UNAVAILABLE);
    let file = f.db.get_file(f.id, file_hash(&url)).await.unwrap();
    assert_eq!(file.status, shaide_db::files::FileStatus::UploadingFailed);
    assert_eq!(file.error_code.as_deref(), Some("storage_unavailable"));
    assert_eq!(put(&f.app, &url).await, StatusCode::CONFLICT);
}

#[tokio::test]
async fn model_replacement_preserves_populated_data_and_updates_only_empty_collections() {
    let f = fixture().await;
    let model =
        f.db.insert_embedding_model(InsertEmbeddingModelDao {
            name: "synthetic-replacement".into(),
            url: "http://invalid.example".into(),
            vector_size: 16,
            platform: None,
            api_schema: None,
            daily_input_token_limit: None,
        })
        .await
        .unwrap();
    assert_eq!(
        request(
            &f.app,
            Method::PATCH,
            "/v1/rag/organization-collection",
            Some(&f.admin),
            json!({"organization_id":f.id,"embedding_model_id":model})
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        f.db.get_organization(f.id)
            .await
            .unwrap()
            .embedding_model_id,
        model
    );
    let url = reserve(&f).await;
    assert_eq!(put(&f.app, &url).await, StatusCode::OK);
    assert_eq!(
        request(
            &f.app,
            Method::POST,
            "/v1/object-storage/event",
            Some("synthetic-webhook-token-32-bytes-long"),
            notification(f.id, file_hash(&url))
        )
        .await
        .0,
        StatusCode::OK
    );
    let before = f.calls.0.lock().unwrap().len();
    assert_eq!(
        request(
            &f.app,
            Method::PATCH,
            "/v1/rag/organization-collection",
            Some(&f.admin),
            json!({"organization_id":f.id,"embedding_model_id":1})
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(f.calls.0.lock().unwrap().len(), before);
    assert_eq!(
        f.db.get_organization(f.id)
            .await
            .unwrap()
            .embedding_model_id,
        model
    );
    assert_eq!(
        f.db.get_file(f.id, file_hash(&url)).await.unwrap().status,
        shaide_db::files::FileStatus::Processed
    );
}
#[tokio::test]
async fn uncertain_collection_delete_keeps_metadata_and_a_durable_fence() {
    let f = fixture().await;
    *f.calls.1.lock().unwrap() = "collection_failure";
    assert_eq!(
        request(
            &f.app,
            Method::DELETE,
            "/v1/rag/organization-collection",
            Some(&f.admin),
            json!({"id":f.id})
        )
        .await
        .0,
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert!(f.db.get_organization(f.id).await.is_ok());
    let other = DbConn::new(&f._temp.join("facade.sqlite")).await.unwrap();
    let state: String =
        sqlx::query_scalar("SELECT state FROM collection_operations WHERE organization_id=?")
            .bind(f.id)
            .fetch_one(&other.pool)
            .await
            .unwrap();
    assert_eq!(state, "recovery_required");
    assert_eq!(
        request(
            &f.app,
            Method::PATCH,
            "/v1/rag/organization-collection",
            Some(&f.admin),
            json!({"organization_id":f.id,"name":"unsafe retry"})
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
}

async fn fix1_collection_ack_case(phase: &str) {
    for body in [
        Value::Null,
        json!([]),
        json!([{}]),
        json!(false),
        json!(7),
        json!("ok"),
        json!({"extra":true}),
        json!({"status":"failed"}),
        json!({}),
    ] {
        let f = fixture().await;
        let valid = body == json!({});
        let method = if phase == "create" || phase == "replace_put" {
            Method::PUT
        } else {
            Method::DELETE
        };
        *f.calls.5.lock().unwrap() = Some((method, body.clone()));
        let mut id = f.id;
        let mut old_files = vec![];
        let (verb, payload) = match phase {
            "create" => (
                Method::POST,
                json!({"organization_name":"ack-fixture","organization_description":"synthetic","can_users_upload":true,"embedding_model_id":1,"users":[]}),
            ),
            "delete" => {
                // Settled file metadata must survive an invalid collection acknowledgement.
                sqlx::query("INSERT INTO files(hash,organization_id,file_name,mime_type,status) VALUES(?,?,'synthetic.pdf','application/pdf','processed')")
                    .bind("a".repeat(64)).bind(id).execute(&f.db.pool).await.unwrap();
                old_files = f.db.list_files(id).await.unwrap();
                (Method::DELETE, json!({"id":id}))
            }
            _ => {
                let model =
                    f.db.insert_embedding_model(InsertEmbeddingModelDao {
                        name: "ack-replacement".into(),
                        url: "http://invalid.example".into(),
                        vector_size: 16,
                        platform: None,
                        api_schema: None,
                        daily_input_token_limit: None,
                    })
                    .await
                    .unwrap();
                (
                    Method::PATCH,
                    json!({"organization_id":id,"embedding_model_id":model}),
                )
            }
        };
        let (status, response) = request(
            &f.app,
            verb,
            "/v1/rag/organization-collection",
            Some(&f.admin),
            payload,
        )
        .await;
        assert_eq!(
            status,
            if valid {
                StatusCode::OK
            } else {
                StatusCode::SERVICE_UNAVAILABLE
            },
            "phase={phase} body={body}"
        );
        if phase == "create" {
            id = if valid {
                response["id"].as_i64().unwrap()
            } else {
                f.db.list_organizations()
                    .await
                    .unwrap()
                    .into_iter()
                    .find(|o| o.name == "ack-fixture")
                    .unwrap()
                    .id
            };
        }
        let operation: Option<(String, String, Option<String>)> = sqlx::query_as(
            "SELECT kind,state,error_code FROM collection_operations WHERE organization_id=?",
        )
        .bind(id)
        .fetch_optional(&f.db.pool)
        .await
        .unwrap();
        if valid {
            assert!(operation.is_none());
            if phase == "delete" {
                assert!(f.db.get_organization(id).await.is_err());
                let remaining: i64 =
                    sqlx::query_scalar("SELECT COUNT(*) FROM files WHERE organization_id=?")
                        .bind(id)
                        .fetch_one(&f.db.pool)
                        .await
                        .unwrap();
                assert_eq!(remaining, 0);
            }
            if phase.starts_with("replace") {
                assert_eq!(
                    f.db.get_organization(id).await.unwrap().embedding_model_id,
                    2
                );
            }
        } else {
            let (kind, state, code) = operation.unwrap();
            assert_eq!(
                kind,
                if phase.starts_with("replace") {
                    "update"
                } else {
                    phase
                }
            );
            assert_eq!(state, "recovery_required");
            assert_eq!(code.as_deref(), Some("kc_outcome_unknown"));
            assert_eq!(response["error"]["code"], "kc_outcome_unknown");
            assert_eq!(
                f.db.get_organization(id).await.unwrap().embedding_model_id,
                1
            );
            assert_eq!(f.db.list_files(id).await.unwrap(), old_files);
            assert!(
                f.db.claim_collection_operation(id, "retry", "delete", "{}")
                    .await
                    .is_err()
            );
        }
        let calls = f.calls.0.lock().unwrap();
        assert_eq!(
            calls.len(),
            if phase == "replace_put" || (phase == "replace_delete" && valid) {
                2
            } else {
                1
            }
        );
        if phase.starts_with("replace") {
            assert_eq!(calls[0].0, Method::DELETE);
            if calls.len() == 2 {
                assert_eq!(calls[1].0, Method::PUT);
            }
        }
    }
}
#[tokio::test]
async fn fix1_create_requires_empty_collection_ack() {
    fix1_collection_ack_case("create").await;
}
#[tokio::test]
async fn fix1_delete_requires_empty_collection_ack() {
    fix1_collection_ack_case("delete").await;
}
#[tokio::test]
async fn fix1_replacement_delete_requires_empty_collection_ack() {
    fix1_collection_ack_case("replace_delete").await;
}
#[tokio::test]
async fn fix1_replacement_put_requires_empty_collection_ack() {
    fix1_collection_ack_case("replace_put").await;
}

async fn fix1_file_fences(f: &Fixture) -> Vec<String> {
    sqlx::query_scalar("SELECT json_array(file_hash,organization_id,owner_id,token,state,expires_at,query_digest,event_received,error_code,created_at) FROM file_operations WHERE organization_id=? ORDER BY file_hash")
        .bind(f.id).fetch_all(&f.db.pool).await.unwrap()
}
async fn fix1_collection_fences(f: &Fixture) -> Vec<String> {
    sqlx::query_scalar("SELECT json_array(organization_id,token,kind,payload,state,error_code,created_at) FROM collection_operations WHERE organization_id=? ORDER BY organization_id")
        .bind(f.id).fetch_all(&f.db.pool).await.unwrap()
}
async fn fix1_restrict_access_case(remove_member: bool) {
    for state in [
        "unused",
        "expired",
        "uncertain",
        "uploaded",
        "collection_recovery",
        "collection_active",
    ] {
        let f = fixture().await;
        let url = reserve(&f).await;
        let hash = file_hash(&url);
        let owner: i64 =
            sqlx::query_scalar("SELECT owner_id FROM file_operations WHERE file_hash=?")
                .bind(hash)
                .fetch_one(&f.db.pool)
                .await
                .unwrap();
        if state == "expired" {
            sqlx::query("UPDATE file_operations SET expires_at=0 WHERE file_hash=?")
                .bind(hash)
                .execute(&f.db.pool)
                .await
                .unwrap();
        }
        if state == "uncertain" {
            f.db.quarantine_file_operation(f.id, hash, "upload_outcome_unknown")
                .await
                .unwrap();
        }
        if state == "uploaded" {
            assert_eq!(put(&f.app, &url).await, StatusCode::OK);
        }
        if state.starts_with("collection_") {
            sqlx::query("INSERT INTO collection_operations(organization_id,token,kind,payload,state) VALUES(?,'retained','update','{}',?)").bind(f.id).bind(if state=="collection_active" {"active"} else {"recovery_required"}).execute(&f.db.pool).await.unwrap();
        }
        let files = f.db.list_files(f.id).await.unwrap();
        let fences = fix1_file_fences(&f).await;
        let collections = fix1_collection_fences(&f).await;
        let (method, path, payload) = if remove_member {
            (
                Method::DELETE,
                "/v1/rag/organization-collection/membership",
                json!({"organization_id":f.id,"user_ids":[owner]}),
            )
        } else {
            (
                Method::PATCH,
                "/v1/rag/organization-collection",
                json!({"organization_id":f.id,"can_users_upload":false}),
            )
        };
        assert_eq!(
            request(
                &f.app,
                method.clone(),
                path,
                Some(&f.member),
                payload.clone()
            )
            .await
            .0,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            request(&f.app, method, path, Some(&f.admin), payload)
                .await
                .0,
            StatusCode::OK,
            "state={state} remove_member={remove_member}"
        );
        if remove_member {
            assert!(!f.db.is_organization_member(f.id, owner).await.unwrap());
        } else {
            assert!(!f.db.get_organization(f.id).await.unwrap().can_users_upload);
        }
        let before = f.calls.0.lock().unwrap().len();
        assert_ne!(put(&f.app, &url).await, StatusCode::OK);
        assert!(f.db.start_upload(f.id, hash).await.is_err());
        assert!(f.db.claim_ingestion(f.id, hash).await.is_err());
        assert_ne!(
            request(
                &f.app,
                Method::POST,
                "/v1/object-storage/event",
                Some("synthetic-webhook-token-32-bytes-long"),
                notification(f.id, hash)
            )
            .await
            .0,
            StatusCode::OK
        );
        assert!(!f.calls.0.lock().unwrap()[before..].iter().any(
            |c| c.1 == "/internal/v1/ingest" || c.1.starts_with("/internal/v1/storage/upload/")
        ));
        assert_eq!(request(&f.app,Method::POST,"/v1/object-storage/presigned-url",Some(&f.member),json!({"organization_id":f.id,"file_name":"later.pdf","content_type":"application/pdf"})).await.0,StatusCode::FORBIDDEN);
        for (verb, payload) in [
            (
                Method::PATCH,
                json!({"organization_id":f.id,"can_users_upload":true}),
            ),
            (
                Method::PATCH,
                json!({"organization_id":f.id,"embedding_model_id":1}),
            ),
            (
                Method::PATCH,
                json!({"organization_id":f.id,"can_users_upload":false,"embedding_model_id":1}),
            ),
            (
                Method::PATCH,
                json!({"organization_id":f.id,"can_users_upload":false,"name":"mixed update"}),
            ),
            (
                Method::PATCH,
                json!({"organization_id":f.id,"name":"not authorized by restriction"}),
            ),
            (Method::DELETE, json!({"id":f.id})),
        ] {
            assert_eq!(
                request(
                    &f.app,
                    verb,
                    "/v1/rag/organization-collection",
                    Some(&f.admin),
                    payload
                )
                .await
                .0,
                StatusCode::CONFLICT
            );
        }
        assert_eq!(
            request(
                &f.app,
                Method::PATCH,
                "/v1/rag/organization-collection/membership",
                Some(&f.admin),
                json!({"organization_id":f.id,"user_ids":[owner]})
            )
            .await
            .0,
            StatusCode::CONFLICT
        );
        assert!(
            f.db.claim_file_delete(f.id, hash, "unsafe", owner)
                .await
                .is_err()
        );
        assert_eq!(f.db.list_files(f.id).await.unwrap(), files);
        let after = fix1_file_fences(&f).await;
        assert_eq!(after, fences);
        let after = fix1_collection_fences(&f).await;
        assert_eq!(after, collections);
    }
}
#[tokio::test]
async fn fix1_public_member_revocation_retains_fences() {
    fix1_restrict_access_case(true).await;
}
#[tokio::test]
async fn fix1_public_upload_disable_retains_fences() {
    fix1_restrict_access_case(false).await;
}
#[tokio::test]
async fn fix1_stale_files_conflicts_on_ordinary_collection_recovery() {
    let f = fixture().await;
    assert_eq!(
        request(
            &f.app,
            Method::DELETE,
            "/v1/rag/delete-stale-files",
            Some(&f.admin),
            json!({})
        )
        .await
        .0,
        StatusCode::OK
    );
    for kind in ["create", "delete", "update"] {
        f.db.claim_collection_operation(f.id, "recovery", kind, "{\"retained\":true}")
            .await
            .unwrap();
        f.db.quarantine_collection_operation(f.id, "recovery", "kc_outcome_unknown")
            .await
            .unwrap();
        let before = f.db.get_organization(f.id).await.unwrap();
        let (status, body) = request(
            &f.app,
            Method::DELETE,
            "/v1/rag/delete-stale-files",
            Some(&f.admin),
            json!({}),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT, "kind={kind}");
        assert_eq!(
            body["error"]["code"],
            "stale_uploads_require_reconciliation"
        );
        assert_eq!(f.db.get_organization(f.id).await.unwrap(), before);
        let operation: (String, String, String, String) = sqlx::query_as(
            "SELECT token,kind,payload,state FROM collection_operations WHERE organization_id=?",
        )
        .bind(f.id)
        .fetch_one(&f.db.pool)
        .await
        .unwrap();
        assert_eq!(
            operation,
            (
                "recovery".into(),
                kind.into(),
                "{\"retained\":true}".into(),
                "recovery_required".into()
            )
        );
        // Only fixture cleanup: stale route must leave the operation untouched.
        sqlx::query("DELETE FROM collection_operations WHERE organization_id=?")
            .bind(f.id)
            .execute(&f.db.pool)
            .await
            .unwrap();
    }
    assert!(f.calls.0.lock().unwrap().is_empty());
}
