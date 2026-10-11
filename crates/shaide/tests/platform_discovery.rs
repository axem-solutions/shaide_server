//! Exercises the real router/authentication in an isolated server process and temporary database.
use chrono::{TimeDelta, Utc};
use reqwest::{Client, StatusCode};
use serde_json::{Value, json};
use std::{
    process::{Child, Command, Stdio},
    time::Duration,
};

struct Server(Child);
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

async fn login(client: &Client, base: &str, username: &str) -> String {
    let response = client
        .post(format!("{base}/v1/login"))
        .json(&json!({"username":username,"password":"discovery-test-password"}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json::<Value>()
        .await
        .unwrap();
    response["access_token"].as_str().unwrap().to_owned()
}

#[tokio::test]
async fn discovery_is_admin_only_and_does_not_register_models() {
    let directory = temp_testdir::TempDir::default();
    let socket = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = socket.local_addr().unwrap();
    drop(socket);
    let mut server = Server(
        Command::new(env!("CARGO_BIN_EXE_shaide"))
            .env_clear()
            .env("shaide_ROOT", directory.join("data"))
            .env("HOST", "127.0.0.1")
            .env("PORT", address.port().to_string())
            .env("ADMIN_PASSWORD", "discovery-test-password")
            .env(
                "JWT_SECRET",
                "isolated-discovery-test-secret-at-least-32-bytes",
            )
            .env("SHAIDE_SERVER_UI_FQDN", "127.0.0.1")
            .env("SHAIDE_SERVER_UI_PORT", "1")
            .current_dir(directory.as_ref())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let base = format!("http://{address}");
    let client = Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    loop {
        assert!(
            server.0.try_wait().unwrap().is_none(),
            "isolated server exited during startup"
        );
        if client.get(format!("{base}/v1/models")).send().await.is_ok() {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "isolated server did not start"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(
        client
            .get(format!("{base}/v1/platform-models"))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    let admin = login(&client, &base, "admin").await;
    client.post(format!("{base}/v1/user")).bearer_auth(&admin).json(&json!({
        "username":"ordinary-user", "password":"discovery-test-password", "expiry":Utc::now()+TimeDelta::days(1)
    })).send().await.unwrap().error_for_status().unwrap();
    let user = login(&client, &base, "ordinary-user").await;
    assert_eq!(
        client
            .get(format!("{base}/v1/platform-models"))
            .bearer_auth(&user)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    let response = client
        .get(format!("{base}/v1/platform-models"))
        .bearer_auth(&admin)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["error"]["code"], "platform_discovery_unavailable");
    // Discovery is not registration. Existing APIs stay usable for ordinary callers.
    for path in ["/v1/models", "/v1/embedding_models"] {
        let models: Value = client
            .get(format!("{base}{path}"))
            .bearer_auth(&user)
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(models["models"], json!([]));
    }
}
