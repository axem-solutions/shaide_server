//! Control Panel-compatible public metadata and authenticated KC forwarding.
use crate::{
    error::ShaideError,
    middlewares::Principal,
    services::knowledge_center::{KcClient, MAX_UPLOAD_BYTES},
};
use axum::{
    Extension, Json, Router,
    body::Body,
    extract::{OriginalUri, Path, State},
    routing,
};
use http::{HeaderMap, Method, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use shaide_common::api::organizations::*;
use shaide_db::{
    DbConn, UserRole,
    files::{FileStatus, InsertFileDao},
    organizations::{InsertOrganizationDao, UpdateOrganizationDao},
};

#[derive(Clone)]
pub(crate) struct Integration(Option<KcClient>);
impl Integration {
    fn client(&self) -> Result<&KcClient, ShaideError> {
        self.0.as_ref().ok_or_else(|| {
            reject(
                StatusCode::SERVICE_UNAVAILABLE,
                "knowledge_center_unavailable",
            )
        })
    }
}
fn reject(status: StatusCode, code: &str) -> ShaideError {
    ShaideError::request_rejection(status, code.replace('_', " "), code)
}
fn admin(p: &Principal) -> Result<(), ShaideError> {
    if p.role == UserRole::Admin {
        Ok(())
    } else {
        Err(reject(StatusCode::FORBIDDEN, "admin_required"))
    }
}
fn query_digest(query: &str) -> String {
    use sha2::Digest;
    format!("{:x}", sha2::Sha256::digest(query.as_bytes()))
}
fn valid_hash(hash: &str) -> bool {
    hash.len() == 64
        && hash
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn collection_id(name: &str) -> Result<i64, ShaideError> {
    let id = name
        .strip_prefix("collection-")
        .and_then(|s| s.parse::<i64>().ok())
        .filter(|id| *id > 0)
        .ok_or_else(|| reject(StatusCode::BAD_REQUEST, "invalid_collection"))?;
    if collection_name(id) != name {
        return Err(reject(StatusCode::BAD_REQUEST, "invalid_collection"));
    }
    Ok(id)
}
async fn upload_access(db: &DbConn, p: &Principal, id: i64) -> Result<(), ShaideError> {
    if id <= 0 {
        return Err(reject(StatusCode::BAD_REQUEST, "invalid_collection"));
    }
    let org = db.get_organization(id).await?;
    if p.role != UserRole::Admin
        && (!org.can_users_upload || !db.is_organization_member(id, p.user.id).await?)
    {
        return Err(reject(StatusCode::FORBIDDEN, "collection_upload_forbidden"));
    }
    Ok(())
}
#[utoipa::path(get, path="/v1/rag/organization-collection",tag="rag",responses((status=200,description="Acknowledged operation",body=OrganizationsResponse),(status=409,description="Conflict or reconciliation required"),(status=503,description="KC unavailable or uncertain outcome")),security(("bearer_token"=[])))]
pub(crate) async fn list(
    p: Principal,
    State(db): State<DbConn>,
) -> Result<Json<OrganizationsResponse>, ShaideError> {
    let orgs = if p.role == UserRole::Admin {
        db.list_organizations().await?
    } else {
        db.list_organizations_for_user(p.user.id).await?
    };
    let mut organization = Vec::new();
    for o in orgs {
        let files = db
            .list_files(o.id)
            .await?
            .into_iter()
            .map(|f| OrganizationFileResponse {
                hash: f.hash,
                name: f.file_name,
                size: f.size,
                mime_type: f.mime_type,
                status: f.status.into(),
                uploaded_at: f.uploaded_at,
            })
            .collect();
        let users = if p.role == UserRole::Admin {
            Some(db.list_organization_member_ids(o.id).await?)
        } else {
            None
        };
        organization.push(OrganizationResponse {
            id: o.id,
            name: o.name,
            description: o.description,
            can_users_upload: o.can_users_upload,
            users,
            files,
            embedding_model_id: o.embedding_model_id,
            collection_name: collection_name(o.id),
            bucket_name: collection_name(o.id),
            created_at: o.created_at,
            updated_at: o.updated_at,
        });
    }
    Ok(Json(OrganizationsResponse { organization }))
}
fn validate_name(name: &str) -> Result<(), ShaideError> {
    if name.trim().is_empty() || name.len() > 255 || name.chars().any(char::is_control) {
        Err(reject(StatusCode::BAD_REQUEST, "invalid_name"))
    } else {
        Ok(())
    }
}
#[utoipa::path(post, path="/v1/rag/organization-collection",tag="rag",request_body=CreateOrganizationRequest,responses((status=200,description="Acknowledged operation",body=CreateOrganizationResponse),(status=409,description="Conflict or reconciliation required"),(status=503,description="KC unavailable or uncertain outcome")),security(("bearer_token"=[])))]
pub(crate) async fn create(
    p: Principal,
    State(db): State<DbConn>,
    Extension(kc): Extension<Integration>,
    Json(r): Json<CreateOrganizationRequest>,
) -> Result<Json<CreateOrganizationResponse>, ShaideError> {
    admin(&p)?;
    validate_name(&r.organization_name)?;
    if r.organization_description.len() > 8192 {
        return Err(reject(StatusCode::BAD_REQUEST, "description_too_long"));
    }
    let kc = kc.client()?;
    let model = db.get_embedding_model(r.embedding_model_id).await?;
    if model.vector_size <= 0 {
        return Err(reject(
            StatusCode::BAD_REQUEST,
            "invalid_embedding_dimensions",
        ));
    }
    let token = uuid::Uuid::new_v4().to_string();
    let id = db
        .create_reserved_organization(
            InsertOrganizationDao {
                name: r.organization_name,
                description: r.organization_description,
                can_users_upload: r.can_users_upload,
                embedding_model_id: r.embedding_model_id,
                user_ids: r.users,
            },
            &token,
        )
        .await?;
    let result = kc
        .collection_ack(
            Method::PUT,
            &format!("/internal/v1/collections/{}", collection_name(id)),
            Some(json!({"vector_size":model.vector_size})),
        )
        .await;
    if let Err(e) = result {
        db.quarantine_collection_operation(id, &token, e.code)
            .await?;
        return Err(e.public());
    }
    db.finish_collection_operation(id, &token, false).await?;
    Ok(Json(CreateOrganizationResponse { id }))
}
#[utoipa::path(delete, path="/v1/rag/organization-collection",tag="rag",request_body=DeleteOrganizationRequest,responses((status=200,description="Acknowledged operation"),(status=409,description="Conflict or reconciliation required"),(status=503,description="KC unavailable or uncertain outcome")),security(("bearer_token"=[])))]
pub(crate) async fn delete_collection(
    p: Principal,
    State(db): State<DbConn>,
    Extension(kc): Extension<Integration>,
    Json(r): Json<DeleteOrganizationRequest>,
) -> Result<Json<Value>, ShaideError> {
    admin(&p)?;
    let kc = kc.client()?;
    let token = uuid::Uuid::new_v4().to_string();
    db.claim_collection_operation(r.id, &token, "delete", "{}")
        .await?;
    if let Err(e) = kc
        .collection_ack(
            Method::DELETE,
            &format!("/internal/v1/collections/{}", collection_name(r.id)),
            None,
        )
        .await
    {
        db.quarantine_collection_operation(r.id, &token, e.code)
            .await?;
        return Err(e.public());
    }
    db.finish_collection_operation(r.id, &token, true).await?;
    Ok(Json(json!({})))
}
#[utoipa::path(patch, path="/v1/rag/organization-collection",tag="rag",request_body=UpdateOrganizationRequest,responses((status=200,description="Acknowledged operation"),(status=409,description="Conflict or reconciliation required"),(status=503,description="KC unavailable or uncertain outcome")),security(("bearer_token"=[])))]
pub(crate) async fn update(
    p: Principal,
    State(db): State<DbConn>,
    Extension(kc): Extension<Integration>,
    Json(r): Json<UpdateOrganizationRequest>,
) -> Result<Json<Value>, ShaideError> {
    admin(&p)?;
    if let Some(name) = &r.name {
        validate_name(name)?;
    }
    if r.description.as_ref().is_some_and(|v| v.len() > 8192) {
        return Err(reject(StatusCode::BAD_REQUEST, "description_too_long"));
    }
    // Only a pure restriction bypasses operation exclusion; mixed updates and
    // grants still require the durable collection fence below.
    if r.can_users_upload == Some(false)
        && r.name.is_none()
        && r.description.is_none()
        && r.embedding_model_id.is_none()
    {
        db.disable_organization_uploads(r.organization_id).await?;
        return Ok(Json(json!({})));
    }
    let token = uuid::Uuid::new_v4().to_string();
    let payload =
        serde_json::to_string(&r).map_err(|_| reject(StatusCode::BAD_REQUEST, "invalid_update"))?;
    db.claim_collection_operation(r.organization_id, &token, "update", &payload)
        .await?;
    // Inspect under the durable collection fence; new reservations cannot race this read.
    let old = db.get_organization(r.organization_id).await?;
    if let Some(model_id) = r
        .embedding_model_id
        .filter(|id| *id != old.embedding_model_id)
    {
        if !db.list_files(r.organization_id).await?.is_empty() {
            db.finish_collection_operation(r.organization_id, &token, false)
                .await?;
            return Err(reject(
                StatusCode::CONFLICT,
                "populated_collection_model_change_requires_reconciliation",
            ));
        }
        let model = match db.get_embedding_model(model_id).await {
            Ok(m) => m,
            Err(e) => {
                db.finish_collection_operation(r.organization_id, &token, false)
                    .await?;
                return Err(e.into());
            }
        };
        if model.vector_size <= 0 {
            db.finish_collection_operation(r.organization_id, &token, false)
                .await?;
            return Err(reject(
                StatusCode::BAD_REQUEST,
                "invalid_embedding_dimensions",
            ));
        }
        let kc = match kc.client() {
            Ok(kc) => kc,
            Err(e) => {
                db.finish_collection_operation(r.organization_id, &token, false)
                    .await?;
                return Err(e);
            }
        };
        let path = format!(
            "/internal/v1/collections/{}",
            collection_name(r.organization_id)
        );
        let result = async {
            kc.collection_ack(Method::DELETE, &path, None).await?;
            kc.collection_ack(
                Method::PUT,
                &path,
                Some(json!({"vector_size":model.vector_size})),
            )
            .await
        }
        .await;
        if let Err(e) = result {
            db.quarantine_collection_operation(r.organization_id, &token, e.code)
                .await?;
            return Err(e.public());
        }
    }
    let result = db
        .update_organization(
            r.organization_id,
            UpdateOrganizationDao {
                name: r.name,
                description: r.description,
                can_users_upload: r.can_users_upload,
                embedding_model_id: r.embedding_model_id,
            },
        )
        .await;
    // A failed local update leaves the fenced payload for explicit settlement.
    result?;
    db.finish_collection_operation(r.organization_id, &token, false)
        .await?;
    Ok(Json(json!({})))
}
#[utoipa::path(patch, path="/v1/rag/organization-collection/membership",tag="rag",request_body=OrganizationMembershipRequest,responses((status=200,description="Acknowledged operation"),(status=409,description="Conflict or reconciliation required"),(status=503,description="KC unavailable or uncertain outcome")),security(("bearer_token"=[])))]
pub(crate) async fn add_members(
    p: Principal,
    State(db): State<DbConn>,
    Json(r): Json<OrganizationMembershipRequest>,
) -> Result<Json<Value>, ShaideError> {
    membership(p, db, r, true).await
}
#[utoipa::path(delete, path="/v1/rag/organization-collection/membership",tag="rag",request_body=OrganizationMembershipRequest,responses((status=200,description="Acknowledged operation"),(status=409,description="Conflict or reconciliation required"),(status=503,description="KC unavailable or uncertain outcome")),security(("bearer_token"=[])))]
pub(crate) async fn remove_members(
    p: Principal,
    State(db): State<DbConn>,
    Json(r): Json<OrganizationMembershipRequest>,
) -> Result<Json<Value>, ShaideError> {
    membership(p, db, r, false).await
}
async fn membership(
    p: Principal,
    db: DbConn,
    r: OrganizationMembershipRequest,
    add: bool,
) -> Result<Json<Value>, ShaideError> {
    admin(&p)?;
    if r.user_ids.len() > 1024 {
        return Err(reject(StatusCode::BAD_REQUEST, "too_many_members"));
    }
    if !add {
        // This existing metadata transaction uses BEGIN IMMEDIATE, serializing
        // revocation with admission without releasing any remote-work fence.
        db.remove_organization_members(r.organization_id, &r.user_ids)
            .await?;
        return Ok(Json(json!({})));
    }
    let token = uuid::Uuid::new_v4().to_string();
    db.claim_collection_operation(r.organization_id, &token, "membership", "{}")
        .await?;
    let result = db
        .add_organization_members(r.organization_id, &r.user_ids)
        .await;
    db.finish_collection_operation(r.organization_id, &token, false)
        .await?;
    result?;
    Ok(Json(json!({})))
}
#[derive(Deserialize, Serialize, utoipa::ToSchema)]
pub struct PresignRequest {
    organization_id: i64,
    file_name: String,
    content_type: String,
}
#[derive(Deserialize, Serialize, utoipa::ToSchema)]
pub struct PresignResponse {
    presigned_url: String,
}
#[utoipa::path(post, path="/v1/object-storage/presigned-url",tag="rag",request_body=PresignRequest,responses((status=200,description="Acknowledged operation",body=PresignResponse),(status=409,description="Conflict or reconciliation required"),(status=503,description="KC unavailable or uncertain outcome")),security(("bearer_token"=[])))]
pub(crate) async fn presign(
    p: Principal,
    State(db): State<DbConn>,
    Extension(kc): Extension<Integration>,
    Json(r): Json<PresignRequest>,
) -> Result<Json<PresignResponse>, ShaideError> {
    upload_access(&db, &p, r.organization_id).await?;
    validate_name(&r.file_name)?;
    if r.file_name.contains(['/', '\\'])
        || r.file_name == "."
        || r.file_name == ".."
        || r.file_name.trim() != r.file_name
    {
        return Err(reject(StatusCode::BAD_REQUEST, "invalid_file_name"));
    }
    if !matches!(
        r.content_type.as_str(),
        "application/pdf"
            | "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
            | "image/png"
            | "image/jpeg"
    ) || r.content_type.len() > 255
        || !r.content_type.contains('/')
        || http::HeaderValue::from_str(&r.content_type).is_err()
    {
        return Err(reject(StatusCode::BAD_REQUEST, "invalid_content_type"));
    }
    let kc = kc.client()?;
    // A server-issued immutable identity, intentionally independent of document contents.
    let hash = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    let token = uuid::Uuid::new_v4().to_string();
    db.reserve_upload(
        InsertFileDao {
            hash: hash.clone(),
            organization_id: r.organization_id,
            file_name: r.file_name,
            mime_type: r.content_type.clone(),
        },
        p.user.id,
        &token,
        300,
    )
    .await?;
    let url = match kc
        .presign(&collection_name(r.organization_id), &hash, &r.content_type)
        .await
    {
        Ok(url) => url,
        Err(e) => {
            db.quarantine_file_operation(r.organization_id, &hash, e.code)
                .await?;
            return Err(e.public());
        }
    };
    db.bind_reservation_query(
        r.organization_id,
        &hash,
        &query_digest(
            url.split_once('?')
                .ok_or_else(|| {
                    reject(
                        StatusCode::SERVICE_UNAVAILABLE,
                        "invalid_presign_acknowledgement",
                    )
                })?
                .1,
        ),
    )
    .await?;
    Ok(Json(PresignResponse { presigned_url: url }))
}
#[utoipa::path(put, path="/s3/{collection}/{file_hash}",tag="rag",params(("collection"=String,Path,description="Canonical collection name"),("file_hash"=String,Path,description="Server-issued file identity")),responses((status=200,description="Acknowledged operation"),(status=409,description="Conflict or reconciliation required"),(status=503,description="KC unavailable or uncertain outcome")))]
pub(crate) async fn upload(
    State(db): State<DbConn>,
    Extension(kc): Extension<Integration>,
    Path((collection, hash)): Path<(String, String)>,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
    body: Body,
) -> Result<StatusCode, ShaideError> {
    let id = collection_id(&collection)?;
    if !valid_hash(&hash) {
        return Err(reject(StatusCode::BAD_REQUEST, "invalid_file_hash"));
    }
    let query = uri
        .query()
        .filter(|q| !q.is_empty() && q.len() <= 4096)
        .ok_or_else(|| reject(StatusCode::BAD_REQUEST, "signed_query_required"))?;
    let file = db.get_file(id, &hash).await?;
    if !db
        .reservation_query_matches(id, &hash, &query_digest(query))
        .await?
    {
        return Err(reject(StatusCode::FORBIDDEN, "invalid_upload_reservation"));
    }
    let mime = headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| reject(StatusCode::BAD_REQUEST, "content_type_required"))?;
    if mime != file.mime_type {
        return Err(reject(StatusCode::BAD_REQUEST, "content_type_mismatch"));
    }
    if headers.get_all("content-type").iter().count() != 1
        || headers.get_all("content-length").iter().count() != 1
    {
        return Err(reject(
            StatusCode::LENGTH_REQUIRED,
            "content_length_required",
        ));
    }
    if let Some(length) = headers.get("content-length") {
        let size = length
            .to_str()
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
            .ok_or_else(|| reject(StatusCode::BAD_REQUEST, "invalid_content_length"))?;
        if size == 0 || size > MAX_UPLOAD_BYTES {
            return Err(reject(StatusCode::PAYLOAD_TOO_LARGE, "upload_size_limit"));
        }
    }
    if headers.contains_key("content-encoding") {
        return Err(reject(
            StatusCode::BAD_REQUEST,
            "content_encoding_not_supported",
        ));
    }
    let kc = kc.client()?;
    db.start_upload(id, &hash).await?;
    let status = match kc.upload(&collection, &hash, query, &headers, body).await {
        Ok(status) => status,
        Err(e) => {
            db.quarantine_file_operation(id, &hash, e.code).await?;
            return Err(e.public());
        }
    };
    db.transition_file_operation(id, &hash, "uploading", "uploaded")
        .await?;
    if db.upload_event_received(id, &hash).await? {
        process(&db, kc, id, &hash).await?;
    }
    Ok(status)
}
#[derive(Deserialize, Serialize, utoipa::ToSchema)]
pub struct StorageEvent {
    #[serde(rename = "Records")]
    records: Vec<StorageRecord>,
}
#[derive(Deserialize, Serialize, utoipa::ToSchema)]
pub struct StorageRecord {
    #[serde(rename = "eventName")]
    event_name: String,
    s3: StorageEntity,
}
#[derive(Deserialize, Serialize, utoipa::ToSchema)]
pub struct StorageEntity {
    bucket: StorageBucket,
    object: StorageObject,
}
#[derive(Deserialize, Serialize, utoipa::ToSchema)]
pub struct StorageBucket {
    name: String,
}
#[derive(Deserialize, Serialize, utoipa::ToSchema)]
pub struct StorageObject {
    key: String,
    #[serde(default)]
    size: Option<i64>,
}
#[derive(Deserialize)]
struct ObjectInfo {
    exists: bool,
    size: i64,
    content_type: String,
}
#[derive(Deserialize)]
struct IngestResult {
    status: String,
    pages: i64,
    chunks: i64,
    size: i64,
    mime_type: String,
}
#[utoipa::path(post, path="/v1/object-storage/event",tag="rag",security(("storage_notification_token"=[])),request_body=StorageEvent,responses((status=200,description="Acknowledged operation"),(status=409,description="Conflict or reconciliation required"),(status=503,description="KC unavailable or uncertain outcome")))]
pub(crate) async fn event(
    State(db): State<DbConn>,
    Extension(kc): Extension<Integration>,
    headers: HeaderMap,
    Json(r): Json<StorageEvent>,
) -> Result<Json<Value>, ShaideError> {
    let kc = kc.client()?;
    if !kc.authentic_notification(&headers) {
        return Err(reject(
            StatusCode::UNAUTHORIZED,
            "invalid_storage_notification",
        ));
    }
    if r.records.is_empty() || r.records.len() > 16 {
        return Err(reject(
            StatusCode::BAD_REQUEST,
            "invalid_storage_notification",
        ));
    }
    for record in r.records {
        if record.event_name != "s3:ObjectCreated:Put" {
            return Err(reject(StatusCode::BAD_REQUEST, "unsupported_storage_event"));
        }
        let id = collection_id(&record.s3.bucket.name)?;
        let hash = record.s3.object.key;
        if !valid_hash(&hash) {
            return Err(reject(StatusCode::BAD_REQUEST, "invalid_file_hash"));
        }
        let file = db.get_file(id, &hash).await?;
        // Never trust notification size/type. KC alone reads storage metadata.
        let object = object_info(&db, kc, id, &hash).await?;
        if !object.exists
            || object.size <= 0
            || object.size as u64 > MAX_UPLOAD_BYTES as u64
            || object.content_type != file.mime_type
        {
            return Err(reject(StatusCode::BAD_REQUEST, "storage_object_mismatch"));
        }
        if file.status == FileStatus::Processed {
            continue;
        }
        let state = db.record_upload_event(id, &hash).await?;
        if state == "uploaded" {
            process(&db, kc, id, &hash).await?;
        }
    }
    Ok(Json(json!({})))
}
async fn object_info(
    _db: &DbConn,
    kc: &KcClient,
    id: i64,
    hash: &str,
) -> Result<ObjectInfo, ShaideError> {
    kc.json(
        Method::GET,
        &format!(
            "/internal/v1/storage/objects/{}/{hash}",
            collection_name(id)
        ),
        None,
    )
    .await
    .map_err(|e| e.public())
}
async fn process(db: &DbConn, kc: &KcClient, id: i64, hash: &str) -> Result<(), ShaideError> {
    if !db.claim_ingestion(id, hash).await? {
        return Ok(());
    }
    // Any cancellation after the claim leaves an ingestion fence, even before a call starts.
    let file = db.get_file(id, hash).await?;
    let org = db.get_organization(id).await?;
    let model = db.get_embedding_model(org.embedding_model_id).await?;
    let collection = collection_name(id);
    let result:Result<IngestResult,_>=kc.json(Method::POST,"/internal/v1/ingest",Some(json!({"collection":collection,"bucket":collection,"key":hash,"file_hash":hash,"file_name":file.file_name,"embedding_model":{"name":model.name,"vector_size":model.vector_size}}))).await;
    let result = match result {
        Ok(r) => r,
        Err(e) => {
            db.quarantine_file_operation(id, hash, e.code).await?;
            return Err(e.public());
        }
    };
    if result.status != "processed"
        || result.pages <= 0
        || result.chunks <= 0
        || result.size <= 0
        || result.size as u64 > MAX_UPLOAD_BYTES as u64
        || result.mime_type != file.mime_type
    {
        db.quarantine_file_operation(id, hash, "invalid_ingest_acknowledgement")
            .await?;
        return Err(reject(
            StatusCode::SERVICE_UNAVAILABLE,
            "invalid_ingest_acknowledgement",
        ));
    }
    db.complete_ingestion(id, hash, result.size, result.pages)
        .await?;
    Ok(())
}
#[utoipa::path(delete, path="/v1/rag/organization-collection/file",tag="rag",request_body=DeleteOrganizationFileRequest,responses((status=200,description="Acknowledged operation"),(status=409,description="Conflict or reconciliation required"),(status=503,description="KC unavailable or uncertain outcome")),security(("bearer_token"=[])))]
pub(crate) async fn delete_file(
    p: Principal,
    State(db): State<DbConn>,
    Extension(kc): Extension<Integration>,
    Json(r): Json<DeleteOrganizationFileRequest>,
) -> Result<Json<Value>, ShaideError> {
    upload_access(&db, &p, r.organization_id).await?;
    if !valid_hash(&r.file_hash) {
        return Err(reject(StatusCode::BAD_REQUEST, "invalid_file_hash"));
    }
    let kc = kc.client()?;
    let token = uuid::Uuid::new_v4().to_string();
    db.claim_file_delete(r.organization_id, &r.file_hash, &token, p.user.id)
        .await?;
    let result: Result<Value, _> = kc
        .json(
            Method::DELETE,
            &format!(
                "/internal/v1/collections/{}/files/{}",
                collection_name(r.organization_id),
                r.file_hash
            ),
            None,
        )
        .await;
    match result {
        Ok(v) if v["deleted"] == true => {}
        Ok(_) => {
            db.quarantine_file_operation(
                r.organization_id,
                &r.file_hash,
                "invalid_delete_acknowledgement",
            )
            .await?;
            return Err(reject(
                StatusCode::SERVICE_UNAVAILABLE,
                "invalid_delete_acknowledgement",
            ));
        }
        Err(e) => {
            db.quarantine_file_operation(r.organization_id, &r.file_hash, e.code)
                .await?;
            return Err(e.public());
        }
    }
    db.complete_file_delete(r.organization_id, &r.file_hash, &token)
        .await?;
    Ok(Json(json!({})))
}
#[utoipa::path(delete, path="/v1/rag/delete-stale-files",tag="rag",responses((status=200,description="Acknowledged operation"),(status=409,description="Conflict or reconciliation required"),(status=503,description="KC unavailable or uncertain outcome")),security(("bearer_token"=[])))]
pub(crate) async fn stale_files(
    p: Principal,
    State(db): State<DbConn>,
) -> Result<Json<Value>, ShaideError> {
    admin(&p)?;
    let count:i64=sqlx::query_scalar("SELECT (SELECT COUNT(*) FROM file_operations WHERE state='recovery_required' OR expires_at<unixepoch()) + (SELECT COUNT(*) FROM collection_operations WHERE kind='legacy_recovery' OR state='recovery_required')").fetch_one(&db.pool).await.map_err(shaide_db::error::ShaideDBError::from)?;
    if count > 0 {
        return Err(reject(
            StatusCode::CONFLICT,
            "stale_uploads_require_reconciliation",
        ));
    }
    Ok(Json(json!({})))
}
pub fn knowledge_center_router(db: DbConn, kc: Option<KcClient>) -> Router {
    Router::new()
        .route(
            "/v1/rag/organization-collection",
            routing::get(list)
                .post(create)
                .patch(update)
                .delete(delete_collection),
        )
        .route(
            "/v1/rag/organization-collection/membership",
            routing::patch(add_members).delete(remove_members),
        )
        .route(
            "/v1/rag/organization-collection/file",
            routing::delete(delete_file),
        )
        .route("/v1/rag/delete-stale-files", routing::delete(stale_files))
        .route("/v1/object-storage/presigned-url", routing::post(presign))
        .route("/v1/object-storage/event", routing::post(event))
        .route("/s3/{collection}/{file_hash}", routing::put(upload))
        .layer(Extension(Integration(kc)))
        .with_state(db)
}
