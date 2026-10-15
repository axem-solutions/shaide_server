use shaide_db::{
    DbConn, embedding_models::InsertEmbeddingModelDao, organizations::InsertOrganizationDao,
};
use temp_testdir::TempDir;

#[tokio::test]
async fn durable_collection_claim_is_exclusive_and_survives_new_connection() {
    let temp = TempDir::default();
    let path = temp.join("lifecycle.sqlite");
    let db = DbConn::new(&path).await.unwrap();
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
            name: "test".into(),
            description: "".into(),
            can_users_upload: true,
            embedding_model_id: model,
            user_ids: vec![],
        })
        .await
        .unwrap();
    sqlx::query("INSERT INTO collection_operations (organization_id, token, kind, payload) VALUES (?, 'first', 'create', '{}')").bind(id).execute(&db.pool).await.unwrap();
    let other = DbConn::new(&path).await.unwrap();
    assert!(sqlx::query("INSERT INTO collection_operations (organization_id, token, kind, payload) VALUES (?, 'second', 'delete', '{}')").bind(id).execute(&other.pool).await.is_err());
    let token: String =
        sqlx::query_scalar("SELECT token FROM collection_operations WHERE organization_id=?")
            .bind(id)
            .fetch_one(&other.pool)
            .await
            .unwrap();
    assert_eq!(token, "first");
}

#[tokio::test]
async fn file_reservation_fences_collection_mutation_and_uncertain_upload() {
    let temp = TempDir::default();
    let db = DbConn::new(&temp.join("fences.sqlite")).await.unwrap();
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
    let owner = db
        .create_user(
            "synthetic-owner".into(),
            "unused".into(),
            chrono::Utc::now() + chrono::Duration::days(1),
        )
        .await
        .unwrap();
    let id = db
        .create_organization(InsertOrganizationDao {
            name: "test".into(),
            description: "".into(),
            can_users_upload: true,
            embedding_model_id: model,
            user_ids: vec![owner],
        })
        .await
        .unwrap();
    let hash = "a".repeat(64);
    db.reserve_upload(
        shaide_db::files::InsertFileDao {
            hash: hash.clone(),
            organization_id: id,
            file_name: "synthetic.txt".into(),
            mime_type: "text/plain".into(),
        },
        owner,
        "reservation",
        300,
    )
    .await
    .unwrap();
    db.confirm_reservation(id, &hash).await.unwrap();
    assert!(
        db.claim_collection_operation(id, "delete", "delete", "{}")
            .await
            .is_err()
    );
    let other = DbConn::new(&temp.join("fences.sqlite")).await.unwrap();
    let (a, b) = tokio::join!(db.start_upload(id, &hash), other.start_upload(id, &hash));
    assert_ne!(a.is_ok(), b.is_ok());
    other
        .quarantine_file_operation(id, &hash, "upload_outcome_unknown")
        .await
        .unwrap();
    sqlx::query("UPDATE file_operations SET expires_at=0")
        .execute(&other.pool)
        .await
        .unwrap();
    assert!(db.start_upload(id, &hash).await.is_err());
    assert!(
        db.claim_file_delete(id, &hash, "delete", owner)
            .await
            .is_err()
    );
    assert!(
        db.claim_collection_operation(id, "delete", "delete", "{}")
            .await
            .is_err()
    );
}
#[tokio::test]
async fn reservation_and_ingestion_recheck_current_owner_access() {
    let temp = TempDir::default();
    let db = DbConn::new(&temp.join("owner.sqlite")).await.unwrap();
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
    let owner = db
        .create_user(
            "synthetic-owner".into(),
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
    let id = db
        .create_organization(InsertOrganizationDao {
            name: "synthetic".into(),
            description: "".into(),
            can_users_upload: true,
            embedding_model_id: model,
            user_ids: vec![owner],
        })
        .await
        .unwrap();
    let file = shaide_db::files::InsertFileDao {
        hash: "b".repeat(64),
        organization_id: id,
        file_name: "synthetic.pdf".into(),
        mime_type: "application/pdf".into(),
    };
    assert!(
        db.reserve_upload(file.clone(), stranger, "denied", 300)
            .await
            .is_err()
    );
    assert!(db.list_files(id).await.unwrap().is_empty());
    db.reserve_upload(file.clone(), owner, "owner", 300)
        .await
        .unwrap();
    db.bind_reservation_query(id, &file.hash, "fingerprint")
        .await
        .unwrap();
    assert!(
        db.reservation_query_matches(id, &file.hash, "fingerprint")
            .await
            .unwrap()
    );
    assert!(
        !db.reservation_query_matches(id, &file.hash, "altered")
            .await
            .unwrap()
    );
    // Real SQL change simulates revocation in another process, bypassing the public API's own fence.
    db.remove_organization_members(id, &[owner]).await.unwrap();
    assert!(db.start_upload(id, &file.hash).await.is_err());
    db.add_organization_members(id, &[owner]).await.unwrap();
    db.start_upload(id, &file.hash).await.unwrap();
    db.transition_file_operation(id, &file.hash, "uploading", "uploaded")
        .await
        .unwrap();
    db.remove_organization_members(id, &[owner]).await.unwrap();
    assert!(db.record_upload_event(id, &file.hash).await.is_err());
    assert!(db.claim_ingestion(id, &file.hash).await.is_err());
    db.add_organization_members(id, &[owner]).await.unwrap();
    db.record_upload_event(id, &file.hash).await.unwrap();
    assert!(db.claim_ingestion(id, &file.hash).await.unwrap());
    assert!(!db.claim_ingestion(id, &file.hash).await.unwrap());
    db.complete_ingestion(id, &file.hash, 9, 1).await.unwrap();
    // A concurrent callback may finish before the public upload checks the event flag.
    assert!(!db.upload_event_received(id, &file.hash).await.unwrap());
}
#[tokio::test]
async fn migration_preserves_legacy_rows_and_quarantines_unsettled_work() {
    use sqlx::{Connection, Executor};
    let temp = TempDir::default();
    let path = temp.join("legacy.sqlite");
    let mut conn =
        sqlx::SqliteConnection::connect(&format!("sqlite://{}?mode=rwc", path.display()))
            .await
            .unwrap();
    conn.execute(include_str!("../migrations/0001_initial_schema.up.sql"))
        .await
        .unwrap();
    conn.execute(include_str!(
        "../migrations/0002_embedding_usage_limits.up.sql"
    ))
    .await
    .unwrap();
    conn.execute(include_str!(
        "../migrations/0003_organizations_and_files.up.sql"
    ))
    .await
    .unwrap();
    sqlx::query("INSERT INTO embedding_models(name,url,vector_size) VALUES('synthetic','http://invalid.example',8)").execute(&mut conn).await.unwrap();
    sqlx::query("INSERT INTO organizations(id,organization_name,embedding_model_id) VALUES(1,'unsettled',1),(2,'settled',1)").execute(&mut conn).await.unwrap();
    sqlx::query("INSERT INTO files(hash,organization_id,file_name,mime_type,status) VALUES('legacy-upload',1,'synthetic.pdf','application/pdf','uploading'),('legacy-ready',2,'synthetic.pdf','application/pdf','processed')").execute(&mut conn).await.unwrap();
    conn.execute(include_str!("../migrations/0004_upload_operations.up.sql"))
        .await
        .unwrap();
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM files")
        .fetch_one(&mut conn)
        .await
        .unwrap();
    assert_eq!(count, 2);
    let locked: Vec<i64> = sqlx::query_scalar(
        "SELECT organization_id FROM collection_operations WHERE state='recovery_required'",
    )
    .fetch_all(&mut conn)
    .await
    .unwrap();
    assert_eq!(locked, vec![1]);
    conn.execute(include_str!(
        "../migrations/0004_upload_operations.down.sql"
    ))
    .await
    .unwrap();
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM files")
        .fetch_one(&mut conn)
        .await
        .unwrap();
    assert_eq!(count, 2);
    conn.execute(include_str!("../migrations/0004_upload_operations.up.sql"))
        .await
        .unwrap();
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM collection_operations")
        .fetch_one(&mut conn)
        .await
        .unwrap();
    assert_eq!(count, 1);
}
