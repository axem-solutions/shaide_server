use chrono::Utc;
use shaide_db::DbConn;
use temp_testdir::TempDir;

#[tokio::test]
async fn created_user_keeps_the_supplied_password_hash() {
    let temp_dir = TempDir::default();
    let db = DbConn::new(&temp_dir.join("shaide-test.sqlite"))
        .await
        .expect("test database should be created");

    let user_id = db
        .create_user(
            "test-user".to_owned(),
            "test-password-hash".to_owned(),
            Utc::now(),
        )
        .await
        .expect("user should be created");
    let user = db
        .get_user_by_username("test-user")
        .await
        .expect("created user should be readable");
    let user_by_id = db
        .get_user_by_id(user_id)
        .await
        .expect("created user should be readable by ID");

    assert_eq!(user.id, user_id);
    assert_eq!(user.username, "test-user");
    assert_eq!(user.password_hash, "test-password-hash");
    assert_eq!(user_by_id.username, user.username);
}
