use chrono::{Duration, Utc};
use shaide_db::DbConn;
use temp_testdir::TempDir;

#[tokio::test]
async fn collection_snapshot_zero_and_current_role() {
    let root = TempDir::default();
    let db = DbConn::new(&root.join("snapshot.sqlite")).await.unwrap();
    let user = db
        .create_user(
            "synthetic".into(),
            String::new(),
            Utc::now() + Duration::days(1),
        )
        .await
        .unwrap();
    let snapshot = db.collection_context_snapshot(user, true).await.unwrap();
    assert_eq!(snapshot.user.id, user);
    assert!(snapshot.collections.is_empty());
    sqlx::query("UPDATE users SET role = 'admin' WHERE id = ?")
        .bind(user)
        .execute(&db.pool)
        .await
        .unwrap();
    let snapshot = db.collection_context_snapshot(user, false).await.unwrap();
    assert_eq!(snapshot.user.role, shaide_db::UserRole::Admin);
    assert!(snapshot.collections.is_empty());
}

#[tokio::test]
async fn snapshot_complete_many_admin_membership_and_processed_catalog() {
    use shaide_db::{
        embedding_models::InsertEmbeddingModelDao, files::InsertFileDao,
        organizations::InsertOrganizationDao,
    };
    let root = TempDir::default();
    let db = DbConn::new(&root.join("many.sqlite")).await.unwrap();
    let user = db
        .create_user(
            "first".into(),
            String::new(),
            Utc::now() + Duration::days(1),
        )
        .await
        .unwrap();
    let other = db
        .create_user(
            "other".into(),
            String::new(),
            Utc::now() + Duration::days(1),
        )
        .await
        .unwrap();
    let admin = db
        .create_admin("admin".into(), String::new())
        .await
        .unwrap();
    let mut ids = Vec::new();
    for i in 0..3 {
        let model = db
            .insert_embedding_model(InsertEmbeddingModelDao {
                name: format!("model-{i}"),
                url: "https://synthetic.invalid/embeddings".into(),
                vector_size: 8 + i,
                platform: None,
                api_schema: None,
                daily_input_token_limit: None,
            })
            .await
            .unwrap();
        let id = db
            .create_organization(InsertOrganizationDao {
                name: format!("collection-{i}"),
                description: "synthetic metadata".into(),
                can_users_upload: false,
                embedding_model_id: model,
                user_ids: if i < 2 { vec![user] } else { vec![other] },
            })
            .await
            .unwrap();
        ids.push(id);
    }
    for i in 0..5 {
        let hash = format!("synthetic-{i}");
        db.insert_file(InsertFileDao {
            hash: hash.clone(),
            organization_id: ids[0],
            file_name: format!("doc-{i}.txt"),
            mime_type: "text/plain".into(),
        })
        .await
        .unwrap();
        if i < 4 {
            db.claim_file_for_processing(ids[0], &hash).await.unwrap();
            db.mark_file_processed(ids[0], &hash, Some(12), None)
                .await
                .unwrap();
        }
    }
    sqlx::query("UPDATE files SET uploaded_at = '2026-01-01 00:00:00'")
        .execute(&db.pool)
        .await
        .unwrap();
    let first = db.collection_context_snapshot(user, true).await.unwrap();
    assert_eq!(
        first
            .collections
            .iter()
            .map(|r| r.collection_id)
            .collect::<Vec<_>>(),
        ids[..2]
    );
    assert_eq!(first.collections[0].uploaded_documents, 5);
    assert_eq!(first.collections[0].searchable_documents, 4);
    assert_eq!(
        first.collections[0].document_names,
        ["doc-0.txt", "doc-1.txt", "doc-2.txt"]
    );
    assert_eq!(first.collections[1].model_name.as_deref(), Some("model-1"));
    assert_eq!(first.collections[1].vector_size, Some(9));
    assert_eq!(
        db.collection_context_snapshot(admin, false)
            .await
            .unwrap()
            .collections
            .len(),
        3
    );
    assert_eq!(
        db.collection_context_snapshot(other, false)
            .await
            .unwrap()
            .collections[0]
            .collection_id,
        ids[2]
    );
    db.remove_organization_members(ids[0], &[user])
        .await
        .unwrap();
    assert_eq!(
        db.collection_context_snapshot(user, false)
            .await
            .unwrap()
            .collections
            .len(),
        1
    );
    sqlx::query("UPDATE users SET role='admin' WHERE id=?")
        .bind(user)
        .execute(&db.pool)
        .await
        .unwrap();
    assert_eq!(
        db.collection_context_snapshot(user, false)
            .await
            .unwrap()
            .collections
            .len(),
        3
    );
    sqlx::query("UPDATE users SET role='user' WHERE id=?")
        .bind(user)
        .execute(&db.pool)
        .await
        .unwrap();
    assert_eq!(
        db.collection_context_snapshot(user, false)
            .await
            .unwrap()
            .collections
            .len(),
        1
    );
    db.delete_organization(ids[1]).await.unwrap();
    assert!(
        db.collection_context_snapshot(user, false)
            .await
            .unwrap()
            .collections
            .is_empty()
    );
    sqlx::query("DELETE FROM users WHERE id=?")
        .bind(user)
        .execute(&db.pool)
        .await
        .unwrap();
    assert!(db.collection_context_snapshot(user, false).await.is_err());
}

#[tokio::test]
async fn snapshot_retains_invalid_model_and_caps_only_with_overflow_sentinel() {
    let root = TempDir::default();
    let db = DbConn::new(&root.join("bounds.sqlite")).await.unwrap();
    let admin = db
        .create_admin("admin".into(), String::new())
        .await
        .unwrap();
    let model = db
        .insert_embedding_model(shaide_db::embedding_models::InsertEmbeddingModelDao {
            name: "synthetic".into(),
            url: "https://synthetic.invalid".into(),
            vector_size: 8,
            platform: None,
            api_schema: None,
            daily_input_token_limit: None,
        })
        .await
        .unwrap();
    for _ in 0..66 {
        sqlx::query(
            "INSERT INTO organizations(organization_name,embedding_model_id) VALUES('synthetic',?)",
        )
        .bind(model)
        .execute(&db.pool)
        .await
        .unwrap();
    }
    assert_eq!(
        db.collection_context_snapshot(admin, false)
            .await
            .unwrap()
            .collections
            .len(),
        65
    );
    // Corrupt a reference using only the dedicated synthetic fixture connection.
    let mut conn = db.pool.acquire().await.unwrap();
    sqlx::query("PRAGMA foreign_keys=OFF")
        .execute(&mut *conn)
        .await
        .unwrap();
    sqlx::query("DELETE FROM embedding_models WHERE id=?")
        .bind(model)
        .execute(&mut *conn)
        .await
        .unwrap();
    sqlx::query("PRAGMA foreign_keys=ON")
        .execute(&mut *conn)
        .await
        .unwrap();
    drop(conn);
    let snapshot = db.collection_context_snapshot(admin, false).await.unwrap();
    assert_eq!(snapshot.collections.len(), 65);
    assert!(
        snapshot
            .collections
            .iter()
            .all(|r| r.model_name.is_none() && r.vector_size.is_none())
    );
}
