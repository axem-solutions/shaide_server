use super::*;
use axum::body::to_bytes;
use jsonwebtoken::Algorithm;
use serde_json::json;
use shaide_db::UserRole;

#[test]
fn disabled_requires_no_secrets_and_config_errors_are_redacted() {
    assert!(
        ContextTargets::parse(None, None, "synthetic-user-secret")
            .unwrap()
            .0
            .is_empty()
    );
    assert!(
        ContextTargets::parse(Some("[]"), None, "synthetic-user-secret")
            .unwrap()
            .0
            .is_empty()
    );
    for raw in ["{", r#"[{"token":"PRIVATE-MARKER"}]"#, "[null]", "[{},{}]"] {
        let err = ContextTargets::parse(Some(raw), None, "synthetic-user-secret").unwrap_err();
        assert!(!err.contains("PRIVATE-MARKER"));
    }
    assert!(ContextTargets::parse(None, Some("1"), "synthetic-user-secret").is_err());
}

#[test]
fn origins_and_routes_are_exact() {
    assert_eq!(
        origin("https://KC-MCP:443", false).unwrap(),
        "https://kc-mcp"
    );
    assert!(origin("http://127.0.0.1:12345", false).is_err());
    assert!(origin("http://127.0.0.1:12345", true).is_ok());
    for bad in [
        "https://u:p@kc",
        "https://kc/base",
        "https://kc/../",
        "https://kc?x=1",
        "https://kc#f",
        "https://kc/%2e",
        "https://kc\\x",
    ] {
        assert!(origin(bad, true).is_err(), "{bad}");
    }
    for path in [
        "/v1/mcp/proxy/kc/%6dcp",
        "/v1/mcp/proxy/kc/mcp/",
        "/v1/mcp/proxy/kc/internal",
        "/v1/mcp/proxy/kc/mcp?q=x",
        "/v1/mcp/proxy/kc/a/../mcp",
    ] {
        assert!(exact_route(&path.parse().unwrap(), &Method::POST, "kc").is_err());
    }
    for method in [Method::POST, Method::GET, Method::DELETE] {
        assert!(exact_route(&"/v1/mcp/proxy/kc/mcp".parse().unwrap(), &method, "kc").is_ok());
    }
    assert!(exact_route(&"/v1/mcp/proxy/kc/mcp".parse().unwrap(), &Method::PUT, "kc").is_err());
}
#[test]
fn inbound_private_headers_and_hop_nominations_are_removed() {
    let mut headers = HeaderMap::new();
    for name in [
        "x-shaide-collection-context",
        "x-kc-token",
        "authorization",
        "x-mcp-authorization",
        "x-user-id",
        "x-role",
        "x-collection-ids",
        "x-client",
        "keep-alive",
    ] {
        headers.insert(name, http::HeaderValue::from_static("synthetic"));
    }
    headers.insert(
        "connection",
        http::HeaderValue::from_static("Authorization, X-Shaide-Collection-Context, x-client"),
    );
    clean_headers(&mut headers, true);
    assert!(headers.is_empty());
    let mut ordinary = HeaderMap::new();
    ordinary.insert(
        "x-shaide-collection-context",
        http::HeaderValue::from_static("forged"),
    );
    ordinary.insert("x-kc-token", http::HeaderValue::from_static("forged"));
    ordinary.insert(
        "x-mcp-authorization",
        http::HeaderValue::from_static("ordinary-key"),
    );
    clean_headers(&mut ordinary, false);
    assert_eq!(ordinary.len(), 1);
    assert_eq!(ordinary["x-mcp-authorization"], "ordinary-key");
}
#[tokio::test]
async fn body_strips_only_authority_metadata_and_bounds_requests() {
    let body = json!({"jsonrpc":"2.0","id":1,"method":"tools/list","_meta":{"role":"admin","shaide/ctx":"forged","client":"kept"},"params":{"_meta":{"kc/context":"forged","collection_ids":[1],"progressToken":42},"arguments":{"query":"synthetic"}}});
    let (body, catalog) = clean_body(Body::from(body.to_string()), &Method::POST)
        .await
        .unwrap();
    assert!(catalog);
    let value: serde_json::Value =
        serde_json::from_slice(&to_bytes(body, BODY_LIMIT).await.unwrap()).unwrap();
    assert_eq!(value["_meta"], json!({"client":"kept"}));
    assert_eq!(value["params"]["_meta"], json!({"progressToken":42}));
    assert_eq!(value["params"]["arguments"]["query"], "synthetic");
    for body in ["[]", "{}", "null", "[{\"jsonrpc\":\"2.0\"}]"] {
        assert!(clean_body(Body::from(body), &Method::POST).await.is_err());
    }
    assert!(
        clean_body(Body::from(vec![b'x'; BODY_LIMIT + 1]), &Method::POST)
            .await
            .is_err()
    );
    assert!(clean_body(Body::from("{}"), &Method::GET).await.is_err());
}

fn synthetic_target(root: &temp_testdir::TempDir) -> (ContextTargets, std::path::PathBuf) {
    let key_path = root.join("private.pem");
    let public_path = root.join("public.pem");
    assert!(
        std::process::Command::new("openssl")
            .args([
                "genpkey",
                "-algorithm",
                "RSA",
                "-pkeyopt",
                "rsa_keygen_bits:2048",
                "-out"
            ])
            .arg(&key_path)
            .output()
            .unwrap()
            .status
            .success()
    );
    assert!(
        std::process::Command::new("openssl")
            .args(["rsa", "-in"])
            .arg(&key_path)
            .args(["-RSAPublicKey_out", "-outform", "DER", "-out"])
            .arg(&public_path)
            .output()
            .unwrap()
            .status
            .success()
    );
    let token_path = root.join("token");
    let token = format!("{}{}", fresh_jti(), fresh_jti());
    std::fs::write(&token_path, token).unwrap();
    let config = json!([{"server_id":"kc", "url":"http://127.0.0.1:23456", "issuer":"shaide-gateway:synthetic", "kc_audience":"kc:kc", "kid":"kc-context-v1", "signing_key_file":key_path,"gateway_token_file":token_path}]);
    (
        ContextTargets::parse(
            Some(&config.to_string()),
            Some("true"),
            "synthetic-user-secret",
        )
        .unwrap(),
        public_path,
    )
}
fn snapshot(count: usize, model_name: &str) -> CollectionContextSnapshot {
    CollectionContextSnapshot {
        user: shaide_db::UserDAO {
            id: 42,
            username: "never-forward-username".into(),
            password_hash: "never-forward-hash".into(),
            role: UserRole::User,
            expiry: chrono::Utc::now() + chrono::Duration::days(1),
        },
        collections: (1..=count)
            .map(|i| shaide_db::collection_context::CollectionContextRow {
                collection_id: i as i64,
                model_name: Some(model_name.into()),
                vector_size: Some(1024),
                name: "🦀".repeat(100),
                description: "synthetic display".repeat(40),
                uploaded_documents: 5,
                searchable_documents: 4,
                document_names: vec![
                    "processed-a.txt".into(),
                    "processed-b.txt".into(),
                    "processed-c.txt".into(),
                ],
            })
            .collect(),
    }
}
#[test]
fn signer_is_interoperable_and_catalog_omissions_do_not_reduce_scope() {
    let root = temp_testdir::TempDir::default();
    let (targets, public) = synthetic_target(&root);
    let target = &targets.0["kc"];
    let now = chrono::Utc::now().timestamp();
    let claims = target
        .claims(snapshot(64, "synthetic-model"), 77, true, now)
        .unwrap();
    assert_eq!(claims.collections.len(), 64);
    let catalog = claims.catalog.as_ref().unwrap();
    assert!(!catalog.entries.is_empty());
    assert_eq!(catalog.entries.len() + catalog.omitted_collections, 64);
    assert!(catalog.omitted_collections > 0);
    assert_eq!(catalog.entries[0].omitted_documents, 1);
    assert!(catalog.entries[0].text_truncated);
    assert_eq!(catalog.entries[0].name.chars().count(), 96);
    assert!(serde_json::to_vec(catalog).unwrap().len() <= 4096);
    assert!(serde_json::to_vec(&claims).unwrap().len() <= CLAIMS_LIMIT);
    let compact = target.sign(&claims).unwrap();
    assert!(compact.len() <= ENVELOPE_LIMIT);
    let mut validation = jsonwebtoken::Validation::new(Algorithm::RS256);
    validation.leeway = 0;
    validation.set_issuer(&["shaide-gateway:synthetic"]);
    validation.set_audience(&["kc-mcp:kc", "kc:kc"]);
    let decoded = jsonwebtoken::decode::<Claims>(
        &compact,
        &jsonwebtoken::DecodingKey::from_rsa_der(&std::fs::read(public).unwrap()),
        &validation,
    )
    .unwrap();
    assert_eq!(
        decoded.header.typ.as_deref(),
        Some("shaide-collection-context+jwt")
    );
    assert_eq!(decoded.header.kid.as_deref(), Some("kc-context-v1"));
    assert_eq!(decoded.claims.sub, "42");
    assert_eq!(decoded.claims.exp - decoded.claims.iat, 30);
    assert_eq!(decoded.claims.request_id, "77");
    assert_eq!(decoded.claims.aud, ["kc-mcp:kc", "kc:kc"]);
    let serialized = serde_json::to_string(&decoded.claims).unwrap();
    assert!(!serialized.contains("never-forward"));
    let second = target
        .claims(snapshot(0, "unused"), 78, false, now)
        .unwrap();
    assert_ne!(second.jti, claims.jti);
    assert!(second.catalog.is_none());
    assert!(second.collections.is_empty());
    assert!(
        target
            .claims(snapshot(65, "synthetic-model"), 79, false, now)
            .is_err()
    );
    assert!(
        target
            .claims(snapshot(64, &"🦀".repeat(256)), 80, false, now)
            .is_err()
    );
    let mut invalid = snapshot(1, "synthetic-model");
    invalid.collections[0].model_name = None;
    assert!(target.claims(invalid, 81, false, now).is_err());
    let mut headers = HeaderMap::new();
    headers.insert(
        "connection",
        http::HeaderValue::from_static("authorization,x-shaide-collection-context"),
    );
    clean_headers(&mut headers, true);
    target.inject(&mut headers, &compact).unwrap();
    assert!(headers.contains_key("authorization"));
    assert_eq!(headers[CONTEXT_HEADER], compact);
    assert!(!headers.contains_key("connection"));
}

#[test]
fn synthetic_key_material_is_validated_and_never_debugged() {
    let root = temp_testdir::TempDir::default();
    let (targets, _) = synthetic_target(&root);
    assert_eq!(format!("{targets:?}"), "ContextTargets([redacted])");
    assert_eq!(
        format!("{:?}", targets.0["kc"]),
        "ContextTarget([redacted])"
    );
    let config = json!([{"server_id":"kc", "url":"https://kc", "issuer":"synthetic", "kc_audience":"kc:kc", "kid":"kc-context-v1", "signing_key_file":root.join("private.pem"), "gateway_token_file":root.join("token")}]);
    let raw = config.to_string();
    std::fs::write(root.join("token"), "short").unwrap();
    assert!(ContextTargets::parse(Some(&raw), None, "synthetic").is_err());
    std::fs::write(root.join("token"), "x".repeat(32)).unwrap();
    std::fs::write(root.join("private.pem"), "PRIVATE-KEY-MARKER").unwrap();
    let err = ContextTargets::parse(Some(&raw), None, "synthetic").unwrap_err();
    assert!(!err.contains("PRIVATE-KEY-MARKER"));
    assert!(!err.contains(&root.display().to_string()));
}

#[tokio::test]
async fn duplicate_body_members_fail_closed() {
    for raw in [
        r#"{"jsonrpc":"2.0","method":"tools/list","method":"tools/call"}"#,
        r#"{"jsonrpc":"2.0","method":"tools/list","params":{"_meta":{"role":"user","role":"admin"}}}"#,
    ] {
        assert!(clean_body(Body::from(raw), &Method::POST).await.is_err());
    }
}

#[test]
fn duplicate_configuration_members_are_rejected_without_loading_secrets() {
    let raw = r#"[{"server_id":"kc","server_id":"ordinary","url":"https://kc","issuer":"synthetic","kc_audience":"kc:kc","kid":"kc-context-v1","signing_key_file":"/never/key","gateway_token_file":"/never/token"}]"#;
    assert_eq!(
        ContextTargets::parse(Some(raw), None, "synthetic").unwrap_err(),
        "invalid context configuration"
    );
}

#[test]
fn expired_current_snapshot_never_gets_signed() {
    let root = temp_testdir::TempDir::default();
    let (targets, _) = synthetic_target(&root);
    let mut expired = snapshot(1, "synthetic");
    expired.user.expiry = chrono::Utc::now() - chrono::Duration::seconds(1);
    assert!(
        targets.0["kc"]
            .claims(expired, 1, false, chrono::Utc::now().timestamp())
            .is_err()
    );
    let mut admin = snapshot(1, "synthetic");
    admin.user.role = UserRole::Admin;
    admin.user.expiry = chrono::Utc::now() - chrono::Duration::days(1);
    assert_eq!(
        targets.0["kc"]
            .claims(admin, 2, false, chrono::Utc::now().timestamp())
            .unwrap()
            .role,
        "admin"
    );
}

#[test]
fn signer_rejects_core_model_and_header_bounds() {
    let root = temp_testdir::TempDir::default();
    let (targets, _) = synthetic_target(&root);
    let target = &targets.0["kc"];
    for size in [0, 65537] {
        let mut invalid = snapshot(1, "synthetic");
        invalid.collections[0].vector_size = Some(size);
        assert!(
            target
                .claims(invalid, 1, false, chrono::Utc::now().timestamp())
                .is_err()
        );
    }
    for name in ["", "  ", "model\ncontrol"] {
        assert!(
            target
                .claims(snapshot(1, name), 1, false, chrono::Utc::now().timestamp())
                .is_err()
        );
    }
    let mut duplicate = snapshot(2, "synthetic");
    duplicate.collections[1].collection_id = 1;
    assert!(
        target
            .claims(duplicate, 1, false, chrono::Utc::now().timestamp())
            .is_err()
    );
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-padding",
        http::HeaderValue::from_str(&"x".repeat(HEADER_LIMIT)).unwrap(),
    );
    assert!(target.inject(&mut headers, "synthetic-envelope").is_err());
}

#[test]
fn fix1_token_requires_base64url_and_32_decoded_bytes() {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    let root = temp_testdir::TempDir::default();
    let _ = synthetic_target(&root);
    let raw = json!([{"server_id":"kc", "url":"https://kc", "issuer":"synthetic", "kc_audience":"kc:kc", "kid":"kc-context-v1", "signing_key_file":root.join("private.pem"), "gateway_token_file":root.join("token")}]).to_string();
    let valid = URL_SAFE_NO_PAD.encode([251u8; 32]);
    for token in [
        valid.clone(),
        format!("{valid}\n"),
        URL_SAFE_NO_PAD.encode([0u8; 33]),
    ] {
        std::fs::write(root.join("token"), token).unwrap();
        assert!(ContextTargets::parse(Some(&raw), None, "synthetic").is_ok());
    }
    for token in [
        URL_SAFE_NO_PAD.encode([0u8; 24]),
        URL_SAFE_NO_PAD.encode([0u8; 31]),
        "!".repeat(43),
        format!("{valid}\n\n"),
        format!("{valid}\r\n"),
        format!("{valid} "),
        format!("{valid}="),
        format!("{}B", "A".repeat(42)),
    ] {
        std::fs::write(root.join("token"), token).unwrap();
        assert_eq!(
            ContextTargets::parse(Some(&raw), None, "synthetic").unwrap_err(),
            "invalid context gateway token"
        );
    }
    std::fs::write(root.join("token"), &valid).unwrap();
    assert_eq!(
        ContextTargets::parse(Some(&raw), None, &valid).unwrap_err(),
        "invalid context gateway token"
    );
}

#[test]
fn fix1_kid_uses_exact_dns_label_boundaries() {
    let root = temp_testdir::TempDir::default();
    let _ = synthetic_target(&root);
    let mut config = json!([{"server_id":"kc", "url":"https://kc", "issuer":"synthetic", "kc_audience":"kc:kc", "kid":"kc-context-v1", "signing_key_file":root.join("private.pem"), "gateway_token_file":root.join("token")}]);
    for kid in ["a".into(), "0".into(), "a-b".into(), "a".repeat(63)] {
        config[0]["kid"] = json!(kid);
        assert!(ContextTargets::parse(Some(&config.to_string()), None, "synthetic").is_ok());
    }
    for kid in [
        "".into(),
        "a".repeat(64),
        "-a".into(),
        "a-".into(),
        "A".into(),
        "a:b".into(),
        "a.b".into(),
        "a_b".into(),
        "a b".into(),
        "é".into(),
    ] {
        config[0]["kid"] = json!(kid);
        assert_eq!(
            ContextTargets::parse(Some(&config.to_string()), None, "synthetic").unwrap_err(),
            "invalid context configuration"
        );
    }
}
