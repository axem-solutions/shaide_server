use shaide_db::{
    DbConn,
    embedding_models::InsertEmbeddingModelDao,
    error::{Resource, ShaideDBError},
    files::{FileClaim, FileStatus, InsertFileDao},
    organizations::InsertOrganizationDao,
};
use temp_testdir::TempDir;

async fn test_db(temp_dir: &TempDir) -> DbConn {
    DbConn::new(&temp_dir.join("shaide-test.sqlite"))
        .await
        .expect("test database should be created")
}

/// Creates two organizations sharing one embedding model.
async fn two_organizations(db: &DbConn) -> (i64, i64) {
    let model = db
        .insert_embedding_model(InsertEmbeddingModelDao {
            url: "https://example.com/embeddings".to_owned(),
            name: "embedder".to_owned(),
            vector_size: 8,
            platform: None,
            api_schema: None,
            daily_input_token_limit: None,
        })
        .await
        .expect("embedding model should be created");
    let mut ids = Vec::new();
    for name in ["First", "Second"] {
        ids.push(
            db.create_organization(InsertOrganizationDao {
                name: name.to_owned(),
                description: String::new(),
                can_users_upload: true,
                embedding_model_id: model,
                user_ids: vec![],
            })
            .await
            .expect("organization should be created"),
        );
    }
    (ids[0], ids[1])
}

fn file(organization_id: i64, hash: &str) -> InsertFileDao {
    InsertFileDao {
        hash: hash.to_owned(),
        organization_id,
        file_name: format!("{hash}.pdf"),
        mime_type: "application/pdf".to_owned(),
    }
}

#[tokio::test]
async fn inserted_file_starts_uploading_in_its_organization() {
    let temp_dir = TempDir::default();
    let db = test_db(&temp_dir).await;
    let (first, _) = two_organizations(&db).await;

    let inserted = db
        .insert_file(file(first, "hash-a"))
        .await
        .expect("file should be inserted");

    assert_eq!(inserted.hash, "hash-a");
    assert_eq!(inserted.organization_id, first);
    assert_eq!(inserted.file_name, "hash-a.pdf");
    assert_eq!(inserted.mime_type, "application/pdf");
    assert_eq!(inserted.status, FileStatus::Uploading);
    assert_eq!(inserted.size, None);
    assert_eq!(inserted.pages, None);
    assert_eq!(inserted.error_code, None);
    assert_eq!(db.get_file(first, "hash-a").await.unwrap(), inserted);
}

#[tokio::test]
async fn files_are_only_visible_in_their_own_organization() {
    let temp_dir = TempDir::default();
    let db = test_db(&temp_dir).await;
    let (first, second) = two_organizations(&db).await;
    // Listed oldest first; inserted in that order since timestamps have
    // one-second resolution.
    db.insert_file(file(first, "hash-a")).await.unwrap();
    db.insert_file(file(first, "hash-b")).await.unwrap();
    db.insert_file(file(second, "hash-c")).await.unwrap();

    let hashes = |files: Vec<shaide_db::files::FileDao>| {
        files.into_iter().map(|file| file.hash).collect::<Vec<_>>()
    };
    assert_eq!(
        hashes(db.list_files(first).await.unwrap()),
        vec!["hash-a", "hash-b"]
    );
    assert_eq!(hashes(db.list_files(second).await.unwrap()), vec!["hash-c"]);

    // Another organization's hash is reported as missing, not exposed.
    for result in [
        db.get_file(second, "hash-a").await,
        db.delete_file(second, "hash-a").await,
    ] {
        assert!(
            matches!(result, Err(ShaideDBError::NotFound(Resource::File))),
            "unexpected result: {result:?}"
        );
    }
    assert_eq!(db.get_file(first, "hash-a").await.unwrap().hash, "hash-a");

    assert!(matches!(
        db.list_files(second + 100).await,
        Err(ShaideDBError::NotFound(Resource::Organization))
    ));
}

#[tokio::test]
async fn duplicate_hash_and_missing_organization_are_rejected() {
    let temp_dir = TempDir::default();
    let db = test_db(&temp_dir).await;
    let (first, second) = two_organizations(&db).await;
    db.insert_file(file(first, "hash-a")).await.unwrap();

    for duplicate in [file(first, "hash-a"), file(second, "hash-a")] {
        let error = db
            .insert_file(duplicate)
            .await
            .expect_err("a hash is stored once");
        assert!(
            matches!(error, ShaideDBError::InsertFailedOnConflict(_)),
            "unexpected error: {error:?}"
        );
    }
    assert_eq!(
        db.get_file(first, "hash-a").await.unwrap().organization_id,
        first
    );

    let error = db
        .insert_file(file(second + 100, "hash-z"))
        .await
        .expect_err("the organization must exist");
    assert!(
        matches!(error, ShaideDBError::NotFound(Resource::Organization)),
        "unexpected error: {error:?}"
    );
}

#[tokio::test]
async fn deleted_file_is_returned_once() {
    let temp_dir = TempDir::default();
    let db = test_db(&temp_dir).await;
    let (first, _) = two_organizations(&db).await;
    let inserted = db.insert_file(file(first, "hash-a")).await.unwrap();

    assert_eq!(db.delete_file(first, "hash-a").await.unwrap(), inserted);
    assert!(matches!(
        db.delete_file(first, "hash-a").await,
        Err(ShaideDBError::NotFound(Resource::File))
    ));
    assert!(db.list_files(first).await.unwrap().is_empty());
}

#[tokio::test]
async fn deleting_an_organization_deletes_its_file_metadata_only() {
    let temp_dir = TempDir::default();
    let db = test_db(&temp_dir).await;
    let (first, second) = two_organizations(&db).await;
    db.insert_file(file(first, "hash-b")).await.unwrap();
    db.insert_file(file(first, "hash-a")).await.unwrap();
    db.insert_file(file(second, "hash-c")).await.unwrap();

    let deleted = db.delete_organization(first).await.unwrap();

    assert_eq!(deleted.file_hashes, vec!["hash-a", "hash-b"]);
    assert!(matches!(
        db.get_file(first, "hash-a").await,
        Err(ShaideDBError::NotFound(Resource::File))
    ));
    assert_eq!(db.list_files(second).await.unwrap().len(), 1);
}

#[tokio::test]
async fn processing_claim_succeeds_only_once() {
    let temp_dir = TempDir::default();
    let db = test_db(&temp_dir).await;
    let (first, second) = two_organizations(&db).await;
    db.insert_file(file(first, "hash-a")).await.unwrap();

    let claim = db.claim_file_for_processing(first, "hash-a").await.unwrap();
    let FileClaim::Claimed(claimed) = claim else {
        panic!("an uploading file should be claimed, got {claim:?}");
    };
    assert_eq!(claimed.status, FileStatus::Processing);

    assert_eq!(
        db.claim_file_for_processing(first, "hash-a").await.unwrap(),
        FileClaim::NotClaimable(FileStatus::Processing)
    );
    assert!(matches!(
        db.claim_file_for_processing(second, "hash-a").await,
        Err(ShaideDBError::NotFound(Resource::File))
    ));
}

// Duplicate storage events can arrive at the same time; exactly one may ingest.
#[tokio::test]
async fn concurrent_processing_claims_have_one_winner() {
    let temp_dir = TempDir::default();
    let db = test_db(&temp_dir).await;
    let (first, _) = two_organizations(&db).await;
    db.insert_file(file(first, "hash-a")).await.unwrap();

    let claims = join_claims((0..8).map(|_| {
        let db = db.clone();
        tokio::spawn(async move { db.claim_file_for_processing(first, "hash-a").await })
    }))
    .await;

    let winners = claims
        .iter()
        .filter(|claim| matches!(claim, FileClaim::Claimed(_)))
        .count();
    assert_eq!(winners, 1, "claims: {claims:?}");
    assert!(
        claims.iter().all(|claim| matches!(
            claim,
            FileClaim::Claimed(_) | FileClaim::NotClaimable(FileStatus::Processing)
        )),
        "claims: {claims:?}"
    );
}

async fn join_claims(
    handles: impl Iterator<Item = tokio::task::JoinHandle<Result<FileClaim, ShaideDBError>>>,
) -> Vec<FileClaim> {
    let mut claims = Vec::new();
    for handle in handles.collect::<Vec<_>>() {
        claims.push(
            handle
                .await
                .expect("claim task should finish")
                .expect("claim should not fail"),
        );
    }
    claims
}

#[tokio::test]
async fn claimed_file_is_completed_with_its_extracted_details() {
    let temp_dir = TempDir::default();
    let db = test_db(&temp_dir).await;
    let (first, _) = two_organizations(&db).await;
    db.insert_file(file(first, "hash-a")).await.unwrap();

    let not_claimed = db
        .mark_file_processed(first, "hash-a", Some(1024), Some(3))
        .await
        .expect_err("an unclaimed file cannot be processed");
    assert!(
        matches!(
            not_claimed,
            ShaideDBError::FileStatusConflict {
                current: FileStatus::Uploading
            }
        ),
        "unexpected error: {not_claimed:?}"
    );

    db.claim_file_for_processing(first, "hash-a").await.unwrap();
    let processed = db
        .mark_file_processed(first, "hash-a", Some(1024), Some(3))
        .await
        .expect("a claimed file should complete");

    assert_eq!(processed.status, FileStatus::Processed);
    assert_eq!(processed.size, Some(1024));
    assert_eq!(processed.pages, Some(3));
    assert_eq!(processed.error_code, None);
    assert_eq!(db.get_file(first, "hash-a").await.unwrap(), processed);
    assert_eq!(
        db.claim_file_for_processing(first, "hash-a").await.unwrap(),
        FileClaim::NotClaimable(FileStatus::Processed)
    );
}

#[tokio::test]
async fn failures_keep_the_stage_and_error_code() {
    let temp_dir = TempDir::default();
    let db = test_db(&temp_dir).await;
    let (first, _) = two_organizations(&db).await;
    db.insert_file(file(first, "upload")).await.unwrap();
    db.insert_file(file(first, "ingest")).await.unwrap();
    db.claim_file_for_processing(first, "ingest").await.unwrap();

    let upload_failed = db
        .mark_file_failed(first, "upload", "object_missing")
        .await
        .unwrap();
    let ingest_failed = db
        .mark_file_failed(first, "ingest", "embedding_quota")
        .await
        .unwrap();

    assert_eq!(upload_failed.status, FileStatus::UploadingFailed);
    assert_eq!(upload_failed.error_code.as_deref(), Some("object_missing"));
    assert_eq!(ingest_failed.status, FileStatus::ProcessingFailed);
    assert_eq!(ingest_failed.error_code.as_deref(), Some("embedding_quota"));

    let already_failed = db
        .mark_file_failed(first, "ingest", "other")
        .await
        .expect_err("a failed file cannot fail again");
    assert!(
        matches!(
            already_failed,
            ShaideDBError::FileStatusConflict {
                current: FileStatus::ProcessingFailed
            }
        ),
        "unexpected error: {already_failed:?}"
    );
    assert!(matches!(
        db.mark_file_failed(first, "missing", "other").await,
        Err(ShaideDBError::NotFound(Resource::File))
    ));
    assert!(matches!(
        db.mark_file_processed(first, "missing", None, None).await,
        Err(ShaideDBError::NotFound(Resource::File))
    ));
}

#[tokio::test]
async fn schema_rejects_invalid_file_rows() {
    let temp_dir = TempDir::default();
    let db = test_db(&temp_dir).await;
    let (first, _) = two_organizations(&db).await;

    let invalid_rows = [
        ("unknown status", "ready", None, None),
        ("negative size", "processed", Some(-1), None),
        (
            "error code without failure",
            "processed",
            None,
            Some("oops"),
        ),
    ];
    for (case, status, size, error_code) in invalid_rows {
        let result = sqlx::query(
            "INSERT INTO files (hash, organization_id, file_name, mime_type, status, size, error_code)
             VALUES (?, ?, 'name', 'text/plain', ?, ?, ?)",
        )
        .bind(case)
        .bind(first)
        .bind(status)
        .bind(size)
        .bind(error_code)
        .execute(&db.pool)
        .await;
        assert!(result.is_err(), "{case} should be rejected");
    }
    let orphan = sqlx::query(
        "INSERT INTO files (hash, organization_id, file_name, mime_type) VALUES ('orphan', ?, 'n', 't')",
    )
    .bind(first + 100)
    .execute(&db.pool)
    .await;
    assert!(orphan.is_err(), "a file needs an existing organization");
    assert!(db.list_files(first).await.unwrap().is_empty());
}

// Documents without pages (plain text) and unknown sizes stay NULL rather than
// turning into zeros or empty strings.
#[tokio::test]
async fn processed_file_without_details_keeps_them_empty() {
    let temp_dir = TempDir::default();
    let db = test_db(&temp_dir).await;
    let (first, _) = two_organizations(&db).await;
    db.insert_file(file(first, "hash-a")).await.unwrap();
    db.claim_file_for_processing(first, "hash-a").await.unwrap();

    let processed = db
        .mark_file_processed(first, "hash-a", None, None)
        .await
        .unwrap();
    let stored = db.get_file(first, "hash-a").await.unwrap();

    for file in [processed, stored] {
        assert_eq!(file.status, FileStatus::Processed);
        assert_eq!(file.size, None);
        assert_eq!(file.pages, None);
        assert_eq!(file.error_code, None);
    }
}

#[test]
fn stored_statuses_map_to_the_same_api_names() {
    use shaide_common::api::organizations::OrganizationFileStatus;

    for status in [
        FileStatus::Uploading,
        FileStatus::UploadingFailed,
        FileStatus::Processing,
        FileStatus::ProcessingFailed,
        FileStatus::Processed,
    ] {
        let api_status = OrganizationFileStatus::from(status);
        assert_eq!(
            serde_json::to_value(api_status).unwrap(),
            serde_json::json!(status.as_str())
        );
    }
}
