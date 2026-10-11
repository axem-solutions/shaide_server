use super::*;
use axum::{Json, Router, routing::get};
use serde_json::json;
use shaide_common::api::{models::CreateModelRequest, platform_models::PlatformModel};

fn service(name: &str, category: &str, service_type: &str, port: Option<u16>) -> Service {
    serde_json::from_value(json!({
        "metadata": {"name": name, "namespace": "models", "labels": {
            "app.kubernetes.io/part-of": "app-serving",
            "axem.dev/model-slug": name,
            "axem.dev/model-category": category
        }},
        "spec": {"type": service_type, "ports": port.map(|port| vec![json!({"port": port})])}
    }))
    .unwrap()
}

fn info(name: &str) -> Value {
    json!({"schema_version": 1, "vllm_config": {"model_config": {
        "model": "/weights/local-path", "served_model_name": [name], "max_model_len": 8192,
        "override_generation_config": {"max_new_tokens": 2048},
        "multimodal_config": {"limit_per_prompt": {"image": 4}},
        "pooler_config": {"dimensions": 768}
    }}})
}

#[test]
fn service_topology_uses_labels_dns_and_service_port() {
    let gen_svc = service("any-name", "generative", "ExternalName", None);
    let found = target(&gen_svc).unwrap().unwrap();
    assert_eq!(found.base, "http://any-name.models.svc.cluster.local:80");
    let embed = target(&service("embed", "embedder", "ClusterIP", Some(8200)))
        .unwrap()
        .unwrap();
    assert_eq!(embed.base, "http://embed.models.svc.cluster.local:8200");
    assert!(
        target(&service("embed-gateway", "embedder", "ExternalName", None))
            .unwrap()
            .is_none()
    );
    let mut unrelated = gen_svc.clone();
    unrelated.metadata.labels = None;
    assert!(target(&unrelated).unwrap().is_none());
    unrelated = gen_svc;
    unrelated.metadata.namespace = Some("evil.example/path".into());
    assert!(target(&unrelated).is_err());
    assert!(target(&service("embed", "embedder", "ClusterIP", None)).is_err());
}

#[test]
fn runtime_names_and_limits_become_registration_requests() {
    let target = target(&service(
        "deployment-slug",
        "generative",
        "ExternalName",
        None,
    ))
    .unwrap()
    .unwrap();
    let result = metadata::normalize(&target, Some(&info("config-name")), Some(&json!({"data": [
        {"id":"public-alias","parent":null}, {"id":"second-alias"}, {"id":"adapter","parent":"base"}
    ]}))).unwrap();
    assert_eq!(result.models.len(), 2);
    let payload = serde_json::to_value(&result.models[0]).unwrap();
    let registration: CreateModelRequest = serde_json::from_value(payload.clone()).unwrap();
    assert_eq!(registration.name, "public-alias");
    assert_eq!(registration.variant, "public-alias");
    assert_eq!(registration.context_size, 8192);
    assert_eq!(registration.max_generated_tokens, 2048);
    assert_eq!(registration.platform.as_deref(), Some("axem"));
    assert_eq!(registration.max_images_per_request, Some(4));
    assert!(registration.supports_images);
    assert_eq!(payload["kind"], "generative");
    assert!(payload.get("vllm_config").is_none());
    assert!(registration.reasoning_effort_values.is_empty());
}

#[test]
fn missing_or_text_metadata_is_reported_without_guessing() {
    let target = target(&service("chat", "generative", "ExternalName", None))
        .unwrap()
        .unwrap();
    assert!(
        metadata::normalize(
            &target,
            None,
            Some(&json!({"data":[{"id":"chat","max_model_len":8192}]}))
        )
        .is_err()
    );
    assert!(
        metadata::normalize(
            &target,
            Some(&json!({"vllm_config":"VllmConfig(model_config=...)"})),
            None
        )
        .is_err()
    );
    for invalid in [json!(0), json!(-1), json!(1.5), json!("8192"), Value::Null] {
        let mut data = info("chat");
        data["vllm_config"]["model_config"]["max_model_len"] = invalid;
        assert!(metadata::normalize(&target, Some(&data), None).is_err());
    }
}

#[test]
fn embedding_dimensions_must_be_explicit_not_hidden_size() {
    let target = target(&service("embed", "embedder", "ClusterIP", Some(8200)))
        .unwrap()
        .unwrap();
    let mut data = info("embedding-model");
    let result = metadata::normalize(&target, Some(&data), None).unwrap();
    let PlatformModel::Embedding(model) = &result.models[0] else {
        panic!("wrong kind")
    };
    assert_eq!(model.vector_size, 768);
    assert_eq!(
        model.url,
        "http://embed.models.svc.cluster.local:8200/v1/embeddings"
    );
    data["vllm_config"]["model_config"]["pooler_config"]["dimensions"] = Value::Null;
    data["vllm_config"]["model_config"]["hf_config"] = json!({"hidden_size":4096});
    assert!(
        metadata::normalize(&target, Some(&data), None)
            .unwrap_err()
            .contains("No inference probe")
    );
}

#[test]
fn limits_and_image_disabled_are_preserved() {
    let target = target(&service("chat", "generative", "ExternalName", None))
        .unwrap()
        .unwrap();
    let mut data = info("chat");
    data["vllm_config"]["model_config"]["override_generation_config"]["max_new_tokens"] =
        json!(20000);
    data["vllm_config"]["model_config"]["multimodal_config"]["limit_per_prompt"]["image"] =
        json!(0);
    let result = metadata::normalize(&target, Some(&data), None).unwrap();
    let PlatformModel::Generative(model) = &result.models[0] else {
        panic!("wrong kind")
    };
    assert_eq!(model.max_generated_tokens, 8192);
    assert!(!model.supports_images);
    assert_eq!(model.max_images_per_request, None);
}

#[test]
fn duplicate_names_exclude_all_conflicting_targets() {
    let target = target(&service("a", "generative", "ExternalName", None))
        .unwrap()
        .unwrap();
    let mut result = metadata::normalize(&target, Some(&info("same")), None).unwrap();
    result.models.extend(
        metadata::normalize(&target, Some(&info("same")), None)
            .unwrap()
            .models,
    );
    result.models.extend(
        metadata::normalize(&target, Some(&info("healthy")), None)
            .unwrap()
            .models,
    );
    remove_duplicates(&mut result);
    assert_eq!(result.models.len(), 1);
    assert_eq!(result.models[0].key().1, "healthy");
    assert_eq!(result.warnings.len(), 1);
    assert_eq!(result.warnings[0].source, "same");
}

#[tokio::test]
async fn discovery_keeps_healthy_services_and_never_posts_or_forwards_auth() {
    async fn metadata_handler(
        headers: axum::http::HeaderMap,
        query: axum::extract::Query<BTreeMap<String, String>>,
    ) -> Json<Value> {
        assert!(!headers.contains_key("authorization"));
        assert!(query.is_empty());
        Json(info("served"))
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let app = Router::new()
        .route("/v1/shaide/metadata", get(metadata_handler))
        .route(
            "/v1/models",
            get(|| async { Json(json!({"data":[{"id":"served"}]})) }),
        );
    let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_millis(500))
        .resolve("chat.models.svc.cluster.local", address)
        .resolve(
            "broken.models.svc.cluster.local",
            "127.0.0.1:1".parse().unwrap(),
        )
        .build()
        .unwrap();
    let result = discover_services(
        &client,
        vec![
            service("chat", "generative", "ExternalName", Some(address.port())),
            service("broken", "generative", "ExternalName", Some(1)),
        ],
    )
    .await;
    task.abort();
    assert_eq!(result.models.len(), 1);
    assert_eq!(result.models[0].key().1, "served");
    assert_eq!(result.warnings.len(), 1);
    assert_eq!(result.warnings[0].source, "models/broken");
}

#[tokio::test]
async fn malformed_http_response_does_not_become_a_model() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new().route("/v1/shaide/metadata", get(|| async { "not-json" })),
        )
        .await
        .unwrap()
    });
    let target = Target {
        base: format!("http://{address}"),
        source: "test".into(),
        category: "generative".into(),
    };
    assert!(
        discover_target(&reqwest::Client::new(), &target)
            .await
            .is_err()
    );
    task.abort();
}

#[tokio::test]
async fn cached_catalog_does_not_connect_to_kubernetes() {
    let discovery = PlatformDiscovery {
        cache: Mutex::new(Some((
            Instant::now(),
            Ok(PlatformModelsResponse::default()),
        ))),
    };
    assert!(discovery.list().await.unwrap().models.is_empty());
}

#[tokio::test]
async fn kubernetes_catalog_uses_the_merged_label_contract_and_never_probes_embeddings() {
    crate::install_rustls_crypto_provider();
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let unexpected = Arc::new(AtomicUsize::new(0));
    let count = unexpected.clone();
    let app = Router::new()
        .route(
            "/api/v1/services",
            get(
                move |query: axum::extract::Query<BTreeMap<String, String>>| async move {
                    assert_eq!(
                        query.get("labelSelector").map(String::as_str),
                        Some(SELECTOR)
                    );
                    Json(
                        json!({"apiVersion":"v1","kind":"ServiceList","metadata":{},"items":[
                            service("embed", "embedder", "ClusterIP", Some(address.port()))
                        ]}),
                    )
                },
            ),
        )
        .route(
            "/v1/shaide/metadata",
            get(|| async {
                let mut data = info("embedding-name");
                data["vllm_config"]["model_config"]["pooler_config"] = Value::Null;
                Json(data)
            }),
        )
        .route(
            "/v1/models",
            get(|| async { Json(json!({"data":[{"id":"embedding-name"}]})) }),
        )
        .fallback(move || {
            count.fetch_add(1, Ordering::SeqCst);
            async { "unexpected request" }
        });
    let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let kube = Client::try_from(kube::Config::new(
        format!("http://{address}").parse().unwrap(),
    ))
    .unwrap();
    let http = reqwest::Client::builder()
        .no_proxy()
        .resolve("embed.models.svc.cluster.local", address)
        .build()
        .unwrap();
    let result = discover_kubernetes(kube, &http).await.unwrap();
    task.abort();
    assert!(result.models.is_empty());
    assert_eq!(result.warnings.len(), 1);
    assert!(result.warnings[0].message.contains("No inference probe"));
    assert_eq!(unexpected.load(Ordering::SeqCst), 0);
}

#[test]
fn metadata_schema_version_is_required_and_checked() {
    let target = target(&service("model", "generative", "ExternalName", None))
        .unwrap()
        .unwrap();
    for version in [Value::Null, json!(2), json!("1")] {
        let mut data = info("model");
        data["schema_version"] = version;
        let error = metadata::normalize(&target, Some(&data), None).unwrap_err();
        assert!(error.contains("schema_version"));
    }
}
