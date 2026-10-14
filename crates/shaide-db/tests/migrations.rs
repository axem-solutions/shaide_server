use chrono::{Duration, Utc};
use shaide_db::{
    DbConn, embedding_models::InsertEmbeddingModelDao, files::InsertFileDao,
    organizations::InsertOrganizationDao,
};
use sqlx::{Connection, SqliteConnection, migrate::Migrate};
use temp_testdir::TempDir;

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

const ORGANIZATIONS_VERSION: i64 = 3;

async fn table_names(conn: &mut SqliteConnection) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT name FROM sqlite_schema
         WHERE type = 'table' AND name NOT LIKE 'sqlite_%' AND name != '_sqlx_migrations'
         ORDER BY name",
    )
    .fetch_all(conn)
    .await
    .expect("tables should be listable")
}

// Reverting the organizations migration on a database in use removes only the
// organization metadata; users and models created before it stay intact, and
// the migration can be applied again.
#[tokio::test]
async fn organizations_migration_reverts_without_touching_existing_data() {
    let temp_dir = TempDir::default();
    let db_file = temp_dir.join("revert.sqlite");
    let db = DbConn::new(&db_file).await.unwrap();
    let user = db
        .create_user(
            "alice".to_owned(),
            "test-password-hash".to_owned(),
            Utc::now() + Duration::days(1),
        )
        .await
        .unwrap();
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
        .unwrap();
    let organization = db
        .create_organization(InsertOrganizationDao {
            name: "Research".to_owned(),
            description: String::new(),
            can_users_upload: false,
            embedding_model_id: model,
            user_ids: vec![user],
        })
        .await
        .unwrap();
    db.insert_file(InsertFileDao {
        hash: "hash-a".to_owned(),
        organization_id: organization,
        file_name: "a.txt".to_owned(),
        mime_type: "text/plain".to_owned(),
    })
    .await
    .unwrap();
    db.pool.close().await;

    let mut conn = SqliteConnection::connect(&format!("sqlite://{}", db_file.display()))
        .await
        .unwrap();
    let tables_with_organizations = table_names(&mut conn).await;
    let down = MIGRATOR
        .iter()
        .find(|migration| {
            migration.version == ORGANIZATIONS_VERSION
                && migration.migration_type.is_down_migration()
        })
        .expect("the organizations migration should have a down migration");
    conn.revert(&MIGRATOR.table_name, down).await.unwrap();

    let tables = table_names(&mut conn).await;
    for table in ["organizations", "organization_memberships", "files"] {
        assert!(
            tables_with_organizations.contains(&table.to_owned()),
            "{table} should exist before reverting"
        );
        assert!(
            !tables.contains(&table.to_owned()),
            "{table} should be dropped"
        );
    }
    assert_eq!(
        tables.len(),
        tables_with_organizations.len() - 3,
        "only the organization tables should be dropped: {tables:?}"
    );
    conn.close().await.unwrap();

    // Opening the database applies the migration again.
    let db = DbConn::new(&db_file).await.unwrap();
    assert_eq!(db.get_user_by_id(user).await.unwrap().username, "alice");
    assert_eq!(
        db.get_embedding_model(model).await.unwrap().name,
        "embedder"
    );
    assert!(db.list_organizations().await.unwrap().is_empty());
}
