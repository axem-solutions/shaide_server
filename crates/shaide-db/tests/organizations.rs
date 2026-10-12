use chrono::{Duration, Utc};
use shaide_db::{
    DbConn,
    embedding_models::InsertEmbeddingModelDao,
    error::{Resource, ShaideDBError},
    organizations::{InsertOrganizationDao, UpdateOrganizationDao},
};
use temp_testdir::TempDir;

async fn test_db(temp_dir: &TempDir) -> DbConn {
    DbConn::new(&temp_dir.join("shaide-test.sqlite"))
        .await
        .expect("test database should be created")
}

async fn create_user(db: &DbConn, username: &str) -> i64 {
    db.create_user(
        username.to_owned(),
        "test-password-hash".to_owned(),
        Utc::now() + Duration::days(1),
    )
    .await
    .expect("user should be created")
}

async fn create_embedding_model(db: &DbConn, name: &str) -> i64 {
    db.insert_embedding_model(InsertEmbeddingModelDao {
        url: "https://example.com/embeddings".to_owned(),
        name: name.to_owned(),
        vector_size: 8,
        platform: None,
        api_schema: None,
        daily_input_token_limit: None,
    })
    .await
    .expect("embedding model should be created")
}

fn organization(name: &str, embedding_model_id: i64, user_ids: Vec<i64>) -> InsertOrganizationDao {
    InsertOrganizationDao {
        name: name.to_owned(),
        description: format!("{name} description"),
        can_users_upload: true,
        embedding_model_id,
        user_ids,
    }
}

async fn count(db: &DbConn, table: &str) -> i64 {
    // Test-only: `table` is always a literal table name from these tests.
    sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT COUNT(*) FROM {table}")))
        .fetch_one(&db.pool)
        .await
        .expect("rows should be countable")
}

#[tokio::test]
async fn organization_is_created_with_its_members() {
    let temp_dir = TempDir::default();
    let db = test_db(&temp_dir).await;
    let alice = create_user(&db, "alice").await;
    let bob = create_user(&db, "bob").await;
    let model = create_embedding_model(&db, "embedder").await;

    // A repeated user ID is one membership, not a conflict.
    let id = db
        .create_organization(organization("Research", model, vec![bob, alice, bob]))
        .await
        .expect("organization should be created");

    let created = db
        .get_organization(id)
        .await
        .expect("organization should be readable");
    assert_eq!(created.id, id);
    assert_eq!(created.name, "Research");
    assert_eq!(created.description, "Research description");
    assert!(created.can_users_upload);
    assert_eq!(created.embedding_model_id, model);
    assert_eq!(
        db.list_organization_member_ids(id).await.unwrap(),
        vec![alice, bob]
    );
}

#[tokio::test]
async fn organization_with_a_missing_user_is_not_created() {
    let temp_dir = TempDir::default();
    let db = test_db(&temp_dir).await;
    let alice = create_user(&db, "alice").await;
    let model = create_embedding_model(&db, "embedder").await;

    let error = db
        .create_organization(organization("Research", model, vec![alice, alice + 100]))
        .await
        .expect_err("a missing member should reject the organization");

    assert!(
        matches!(error, ShaideDBError::MissingReference(Resource::User)),
        "unexpected error: {error:?}"
    );
    assert_eq!(count(&db, "organizations").await, 0);
    assert_eq!(count(&db, "organization_memberships").await, 0);
}

#[tokio::test]
async fn organization_with_a_missing_embedding_model_is_not_created() {
    let temp_dir = TempDir::default();
    let db = test_db(&temp_dir).await;
    let alice = create_user(&db, "alice").await;

    let error = db
        .create_organization(organization("Research", 404, vec![alice]))
        .await
        .expect_err("a missing embedding model should reject the organization");

    assert!(
        matches!(
            error,
            ShaideDBError::MissingReference(Resource::EmbeddingModel)
        ),
        "unexpected error: {error:?}"
    );
    assert_eq!(count(&db, "organizations").await, 0);
}

#[tokio::test]
async fn missing_organization_is_reported_as_not_found() {
    let temp_dir = TempDir::default();
    let db = test_db(&temp_dir).await;

    let error = db
        .get_organization(1)
        .await
        .expect_err("no organization exists yet");

    assert!(
        matches!(error, ShaideDBError::NotFound(Resource::Organization)),
        "unexpected error: {error:?}"
    );
}

#[tokio::test]
async fn users_list_only_their_organizations_while_all_are_listed_for_admins() {
    let temp_dir = TempDir::default();
    let db = test_db(&temp_dir).await;
    let alice = create_user(&db, "alice").await;
    let bob = create_user(&db, "bob").await;
    let carol = create_user(&db, "carol").await;
    let model = create_embedding_model(&db, "embedder").await;
    let shared = db
        .create_organization(organization("Shared", model, vec![alice, bob]))
        .await
        .unwrap();
    let bobs = db
        .create_organization(organization("Bob only", model, vec![bob]))
        .await
        .unwrap();
    let empty = db
        .create_organization(organization("Empty", model, vec![]))
        .await
        .unwrap();

    let ids = |organizations: Vec<shaide_db::organizations::OrganizationDao>| {
        organizations
            .into_iter()
            .map(|organization| organization.id)
            .collect::<Vec<_>>()
    };
    assert_eq!(
        ids(db.list_organizations().await.unwrap()),
        vec![shared, bobs, empty]
    );
    assert_eq!(
        ids(db.list_organizations_for_user(alice).await.unwrap()),
        vec![shared]
    );
    assert_eq!(
        ids(db.list_organizations_for_user(bob).await.unwrap()),
        vec![shared, bobs]
    );
    assert!(
        db.list_organizations_for_user(carol)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(db.is_organization_member(shared, alice).await.unwrap());
    assert!(!db.is_organization_member(bobs, alice).await.unwrap());
    assert!(!db.is_organization_member(empty, carol).await.unwrap());
}

#[tokio::test]
async fn organization_update_changes_only_the_given_fields() {
    let temp_dir = TempDir::default();
    let db = test_db(&temp_dir).await;
    let alice = create_user(&db, "alice").await;
    let first_model = create_embedding_model(&db, "first").await;
    let second_model = create_embedding_model(&db, "second").await;
    let id = db
        .create_organization(organization("Research", first_model, vec![alice]))
        .await
        .unwrap();

    let update = db
        .update_organization(
            id,
            UpdateOrganizationDao {
                name: Some("Renamed".to_owned()),
                description: None,
                can_users_upload: Some(false),
                embedding_model_id: Some(second_model),
            },
        )
        .await
        .expect("organization should be updated");

    assert_eq!(update.previous.name, "Research");
    assert_eq!(update.previous.embedding_model_id, first_model);
    assert_eq!(update.current, db.get_organization(id).await.unwrap());
    assert_eq!(update.current.name, "Renamed");
    assert_eq!(update.current.description, "Research description");
    assert!(!update.current.can_users_upload);
    assert_eq!(update.current.embedding_model_id, second_model);
    assert!(update.current.updated_at >= update.previous.updated_at);
    assert_eq!(update.current.created_at, update.previous.created_at);
    assert_eq!(
        db.list_organization_member_ids(id).await.unwrap(),
        vec![alice]
    );
}

#[tokio::test]
async fn organization_update_rejects_missing_targets() {
    let temp_dir = TempDir::default();
    let db = test_db(&temp_dir).await;
    let model = create_embedding_model(&db, "embedder").await;
    let id = db
        .create_organization(organization("Research", model, vec![]))
        .await
        .unwrap();
    let unchanged = db.get_organization(id).await.unwrap();

    let missing_model = db
        .update_organization(
            id,
            UpdateOrganizationDao {
                name: Some("Renamed".to_owned()),
                embedding_model_id: Some(model + 100),
                ..UpdateOrganizationDao::default()
            },
        )
        .await
        .expect_err("a missing embedding model should be rejected");
    assert!(
        matches!(
            missing_model,
            ShaideDBError::MissingReference(Resource::EmbeddingModel)
        ),
        "unexpected error: {missing_model:?}"
    );
    assert_eq!(db.get_organization(id).await.unwrap(), unchanged);

    let missing_organization = db
        .update_organization(id + 100, UpdateOrganizationDao::default())
        .await
        .expect_err("a missing organization should be reported");
    assert!(
        matches!(
            missing_organization,
            ShaideDBError::NotFound(Resource::Organization)
        ),
        "unexpected error: {missing_organization:?}"
    );
}

#[tokio::test]
async fn organization_delete_removes_memberships_but_keeps_users_and_models() {
    let temp_dir = TempDir::default();
    let db = test_db(&temp_dir).await;
    let alice = create_user(&db, "alice").await;
    let model = create_embedding_model(&db, "embedder").await;
    let deleted_id = db
        .create_organization(organization("Deleted", model, vec![alice]))
        .await
        .unwrap();
    let kept_id = db
        .create_organization(organization("Kept", model, vec![alice]))
        .await
        .unwrap();

    let deleted = db
        .delete_organization(deleted_id)
        .await
        .expect("organization should be deleted");

    assert_eq!(deleted.organization.id, deleted_id);
    assert!(deleted.file_hashes.is_empty());
    assert!(matches!(
        db.get_organization(deleted_id).await,
        Err(ShaideDBError::NotFound(Resource::Organization))
    ));
    assert_eq!(count(&db, "organization_memberships").await, 1);
    assert_eq!(
        db.list_organization_member_ids(kept_id).await.unwrap(),
        vec![alice]
    );
    assert_eq!(db.get_user_by_id(alice).await.unwrap().username, "alice");
    assert_eq!(db.get_embedding_model(model).await.unwrap().id, model);
    assert!(matches!(
        db.delete_organization(deleted_id).await,
        Err(ShaideDBError::NotFound(Resource::Organization))
    ));
}

#[tokio::test]
async fn embedding_model_in_use_by_an_organization_cannot_be_deleted() {
    let temp_dir = TempDir::default();
    let db = test_db(&temp_dir).await;
    let model = create_embedding_model(&db, "embedder").await;
    let id = db
        .create_organization(organization("Research", model, vec![]))
        .await
        .unwrap();

    let error = db
        .delete_embedding_model(model)
        .await
        .expect_err("the foreign key should keep the model");
    assert!(
        matches!(&error, ShaideDBError::DBError(sqlx::Error::Database(database_error))
            if database_error.is_foreign_key_violation()),
        "unexpected error: {error:?}"
    );
    assert_eq!(
        db.get_organization(id).await.unwrap().embedding_model_id,
        model
    );

    db.delete_organization(id).await.unwrap();
    db.delete_embedding_model(model)
        .await
        .expect("an unused model should be deletable");
}

#[tokio::test]
async fn deleting_a_user_removes_only_their_memberships() {
    let temp_dir = TempDir::default();
    let db = test_db(&temp_dir).await;
    let alice = create_user(&db, "alice").await;
    let bob = create_user(&db, "bob").await;
    let model = create_embedding_model(&db, "embedder").await;
    let id = db
        .create_organization(organization("Research", model, vec![alice, bob]))
        .await
        .unwrap();

    // There is no user deletion API yet; the foreign key must still cascade.
    sqlx::query("DELETE FROM users WHERE id = ?")
        .bind(bob)
        .execute(&db.pool)
        .await
        .unwrap();

    assert_eq!(
        db.list_organization_member_ids(id).await.unwrap(),
        vec![alice]
    );
    assert_eq!(db.get_organization(id).await.unwrap().name, "Research");
}

#[tokio::test]
async fn adding_and_removing_members_is_idempotent() {
    let temp_dir = TempDir::default();
    let db = test_db(&temp_dir).await;
    let alice = create_user(&db, "alice").await;
    let bob = create_user(&db, "bob").await;
    let carol = create_user(&db, "carol").await;
    let model = create_embedding_model(&db, "embedder").await;
    let id = db
        .create_organization(organization("Research", model, vec![alice]))
        .await
        .unwrap();

    assert_eq!(
        db.add_organization_members(id, &[alice, bob, bob])
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        db.add_organization_members(id, &[alice, bob])
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        db.list_organization_member_ids(id).await.unwrap(),
        vec![alice, bob]
    );

    assert_eq!(
        db.remove_organization_members(id, &[bob, carol])
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        db.remove_organization_members(id, &[bob, carol])
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        db.list_organization_member_ids(id).await.unwrap(),
        vec![alice]
    );
}

#[tokio::test]
async fn membership_changes_report_missing_records_without_partial_writes() {
    let temp_dir = TempDir::default();
    let db = test_db(&temp_dir).await;
    let alice = create_user(&db, "alice").await;
    let bob = create_user(&db, "bob").await;
    let model = create_embedding_model(&db, "embedder").await;
    let id = db
        .create_organization(organization("Research", model, vec![alice]))
        .await
        .unwrap();

    let missing_user = db
        .add_organization_members(id, &[bob, bob + 100])
        .await
        .expect_err("a missing user should reject the whole change");
    assert!(
        matches!(
            missing_user,
            ShaideDBError::MissingReference(Resource::User)
        ),
        "unexpected error: {missing_user:?}"
    );
    assert_eq!(
        db.list_organization_member_ids(id).await.unwrap(),
        vec![alice]
    );

    for result in [
        db.add_organization_members(id + 100, &[alice]).await,
        db.remove_organization_members(id + 100, &[alice]).await,
    ] {
        assert!(
            matches!(result, Err(ShaideDBError::NotFound(Resource::Organization))),
            "unexpected result: {result:?}"
        );
    }
}
