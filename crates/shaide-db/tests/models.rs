// Turso pool futures require deeper Send trait evaluation.
#![recursion_limit = "256"]

use std::path::Path;

use shaide_db::{
    DbConn, InsertModelDAO,
    models::{NativeFimModeDao, ReasoningEffortValuesDao},
};
use sqlx::{Connection, migrate::Migrate};
use sqlx_turso::TursoConnection;
use temp_testdir::TempDir;

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

fn insert_model(name: &str, reasoning_effort_values: &[&str]) -> InsertModelDAO {
    InsertModelDAO {
        name: name.to_owned(),
        variant: name.to_owned(),
        chat_completions_endpoint: "https://example.com/v1/chat/completions".to_owned(),
        completions_endpoint: None,
        responses_endpoint: Some("https://example.com/v1/responses".to_owned()),
        api_schema: "open_ai".to_owned(),
        daily_input_token_limit: None,
        daily_output_token_limit: None,
        supports_images: false,
        reasoning_effort_values: ReasoningEffortValuesDao(
            reasoning_effort_values
                .iter()
                .map(|value| (*value).to_owned())
                .collect(),
        ),
        max_images_per_request: None,
        max_image_bytes: None,
        max_image_width_px: None,
        max_image_height_px: None,
        max_generated_tokens: 512,
        context_size: 32768,
        platform: None,
        native_fim_mode: None::<NativeFimModeDao>,
        fim_prompt_template: None,
    }
}

async fn test_db(db_file: &Path) -> DbConn {
    DbConn::new(db_file)
        .await
        .expect("test database should be created")
}

async fn round_trip(values: &[&str]) -> Vec<String> {
    let temp_dir = TempDir::default();
    let db = test_db(&temp_dir.join("shaide-test.sqlite")).await;
    db.create_model(insert_model("some-model", values))
        .await
        .expect("model should be created");

    let model = db
        .get_model_by_name("some-model")
        .await
        .expect("model should be found");
    model.to_api_response().reasoning_effort_values
}

#[tokio::test]
async fn values_survive_the_json_column_unchanged() {
    for case in [
        vec![],
        vec!["low", "medium", "high"],
        vec!["none", "minimal", "low", "medium", "high", "xhigh"],
        vec!["high", "minimal", "low"],
    ] {
        assert_eq!(round_trip(&case).await, case, "{case:?} should round-trip");
    }
}

#[tokio::test]
async fn responses_endpoint_survives_round_trip() {
    let temp_dir = TempDir::default();
    let db = test_db(&temp_dir.join("shaide-test.sqlite")).await;
    let model = insert_model("responses-model", &[]);
    let expected_endpoint = model.responses_endpoint.clone();
    db.create_model(model)
        .await
        .expect("model should be created");

    let stored_model = db
        .get_model_by_name("responses-model")
        .await
        .expect("model should be found");

    assert_eq!(stored_model.responses_endpoint, expected_endpoint);
}

#[tokio::test]
async fn baseline_migration_is_reversible() {
    let temp_dir = TempDir::default();
    let db_file = temp_dir.join("revert.sqlite");
    let db = test_db(&db_file).await;
    db.pool.close().await;

    let mut conn = TursoConnection::connect(&format!("turso://{}", db_file.display()))
        .await
        .expect("test database should be reachable");
    let down_migration = MIGRATOR
        .iter()
        .find(|migration| migration.migration_type.is_down_migration())
        .expect("baseline should have a down migration");
    conn.revert(&MIGRATOR.table_name, down_migration)
        .await
        .expect("baseline migration should revert");

    let remaining_tables: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_schema
         WHERE type = 'table'
           AND name NOT LIKE 'sqlite_%'
           AND name != '_sqlx_migrations'",
    )
    .fetch_one(&mut conn)
    .await
    .expect("remaining tables should be countable");
    assert_eq!(remaining_tables, 0);
}

#[tokio::test]
async fn model_constraints_and_values_survive_reopening() {
    let temp_dir = TempDir::default();
    let db_file = temp_dir.join("models.turso");
    let db = test_db(&db_file).await;
    let mut model = insert_model("persistent-model", &["high"]);
    model.supports_images = true;
    model.daily_input_token_limit = Some(i64::from(i32::MAX) + 1);
    let id = db.create_model(model).await.unwrap();
    assert!(matches!(
        db.create_model(insert_model("persistent-model", &[])).await,
        Err(shaide_db::error::ShaideDBError::InsertFailedOnConflict(_))
    ));
    db.pool.close().await;

    let db = test_db(&db_file).await;
    let model = db.get_model_by_name("persistent-model").await.unwrap();
    assert_eq!(model.id, id);
    assert!(model.supports_images);
    assert_eq!(model.daily_input_token_limit, Some(i64::from(i32::MAX) + 1));
    assert!(model.platform.is_none());
    assert_eq!(model.reasoning_effort_values.0, ["high"]);
}

#[tokio::test]
async fn usage_transactions_roll_back_and_foreign_keys_cascade() {
    use shaide_db::daily_usage::UpsertDailyUsageDao;

    let temp_dir = TempDir::default();
    let db = test_db(&temp_dir.join("usage.turso")).await;
    let user = db
        .create_user("usage-user".into(), "hash".into(), chrono::Utc::now())
        .await
        .unwrap();
    let model = db
        .create_model(insert_model("usage-model", &[]))
        .await
        .unwrap();
    for _ in 0..2 {
        db.upsert_daily_usage_token_count(UpsertDailyUsageDao {
            user,
            model,
            date: "2026-09-09".into(),
            input_token_count: i64::from(i32::MAX),
            output_token_count: 7,
        })
        .await
        .unwrap();
    }
    let usage = db
        .get_daily_usage("2026-09-09", user, model)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(usage.total_input_token_count, 2 * i64::from(i32::MAX));
    assert_eq!(usage.total_output_token_count, 14);

    let mut transaction = db.pool.begin().await.unwrap();
    sqlx::query("DELETE FROM models WHERE id = ?")
        .bind(model)
        .execute(&mut *transaction)
        .await
        .unwrap();
    transaction.rollback().await.unwrap();
    assert!(
        db.get_daily_usage("2026-09-09", user, model)
            .await
            .unwrap()
            .is_some()
    );

    db.delete_model_by_id(model).await.unwrap();
    assert!(
        db.get_daily_usage("2026-09-09", user, model)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        db.upsert_daily_usage_token_count(UpsertDailyUsageDao {
            user,
            model,
            date: "2026-09-09".into(),
            input_token_count: 1,
            output_token_count: 1,
        })
        .await
        .is_err()
    );
}
