//! Runtime discovery uses Kubernetes for routing and vLLM for model facts.
//! Never reads deployment files or registers models as a side effect.
mod metadata;

use std::{collections::BTreeMap, time::Duration};

use futures::{StreamExt, stream};
use k8s_openapi::api::core::v1::Service;
use kube::{Api, Client, ResourceExt, api::ListParams};
use reqwest::redirect::Policy;
use serde_json::Value;
use shaide_common::api::platform_models::{PlatformModelWarning, PlatformModelsResponse};
use tokio::{sync::Mutex, time::Instant};

const SELECTOR: &str =
    "app.kubernetes.io/part-of=app-serving,axem.dev/model-slug,axem.dev/model-category";
const CACHE_TTL: Duration = Duration::from_secs(30);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(3);
const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(20);
const MAX_RESPONSE_BYTES: usize = 2 * 1024 * 1024;

type DiscoveryResult = Result<PlatformModelsResponse, &'static str>;

#[derive(Default)]
pub struct PlatformDiscovery {
    // The lock also coalesces concurrent refreshes. Never retain stale results after a failed refresh.
    cache: Mutex<Option<(Instant, DiscoveryResult)>>,
}

impl PlatformDiscovery {
    pub async fn list(&self) -> DiscoveryResult {
        let mut cache = self.cache.lock().await;
        if let Some((created, result)) = &*cache
            && created.elapsed() < CACHE_TTL
        {
            return result.clone();
        }
        let result = self.discover().await;
        *cache = Some((Instant::now(), result.clone()));
        result
    }

    async fn discover(&self) -> DiscoveryResult {
        // Do not silently use a developer's ambient kubeconfig in a non-Kubernetes deployment.
        let config = kube::Config::incluster().map_err(
            |_| "AI Platform discovery requires an in-cluster Kubernetes ServiceAccount.",
        )?;
        let client = Client::try_from(config)
            .map_err(|_| "Unable to initialize AI Platform Kubernetes discovery.")?;
        let http = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .redirect(Policy::none())
            .no_proxy()
            .build()
            .map_err(|_| "Unable to initialize the model metadata client.")?;
        discover_kubernetes(client, &http).await
    }
}

async fn discover_kubernetes(client: Client, http: &reqwest::Client) -> DiscoveryResult {
    let services: Api<Service> = Api::all(client);
    let services = tokio::time::timeout(
        Duration::from_secs(5),
        services.list(&ListParams::default().labels(SELECTOR)),
    )
    .await
    .map_err(|_| "Kubernetes model discovery timed out.")?
    .map_err(
        |_| "Unable to list model Services. Check the shaide-server topology-reader permissions.",
    )?;
    Ok(discover_services(http, services.items).await)
}

#[derive(Clone, Debug)]
struct Target {
    source: String,
    base: String,
    category: String,
}

fn dns_label(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 63
        && value.as_bytes()[0].is_ascii_alphanumeric()
        && value.as_bytes()[value.len() - 1].is_ascii_alphanumeric()
        && value
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
}

fn target(service: &Service) -> Result<Option<Target>, &'static str> {
    let labels = service.labels();
    if labels.get("app.kubernetes.io/part-of").map(String::as_str) != Some("app-serving")
        || !labels.contains_key("axem.dev/model-slug")
    {
        return Ok(None);
    }
    let Some(category) = labels.get("axem.dev/model-category") else {
        return Ok(None);
    };
    let spec = service
        .spec
        .as_ref()
        .ok_or("Service has no routing specification.")?;
    let service_type = spec.type_.as_deref().unwrap_or("ClusterIP");
    // Older platform versions also create a gateway for embedders. Use only their direct Service.
    if !matches!(
        (category.as_str(), service_type),
        ("generative", "ExternalName") | ("embedder", "ClusterIP")
    ) {
        return Ok(None);
    }
    let name = service.name_any();
    let namespace = service.namespace().ok_or("Service has no namespace.")?;
    if !dns_label(&name) || !dns_label(&namespace) {
        return Err("Service has an invalid DNS name or namespace.");
    }
    let ports: Vec<_> = spec
        .ports
        .iter()
        .flatten()
        .filter(|port| port.protocol.as_deref().unwrap_or("TCP") == "TCP")
        .collect();
    let port = match ports.as_slice() {
        [] if service_type == "ExternalName" => 80,
        [port] => port.port,
        ports => {
            ports
                .iter()
                .find(|port| port.name.as_deref() == Some("http"))
                .ok_or("Service does not have an unambiguous HTTP port.")?
                .port
        }
    };
    if !(1..=65535).contains(&port) {
        return Err("Service has an invalid HTTP port.");
    }
    Ok(Some(Target {
        source: format!("{namespace}/{name}"),
        base: format!("http://{name}.{namespace}.svc.cluster.local:{port}"),
        category: category.clone(),
    }))
}

async fn json_response(mut response: reqwest::Response) -> Result<Value, &'static str> {
    if !response.status().is_success() {
        return Err("Model metadata endpoint returned an error.");
    }
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "Unable to read model metadata.")?
    {
        if body.len() + chunk.len() > MAX_RESPONSE_BYTES {
            return Err("Model metadata response is too large.");
        }
        body.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&body).map_err(|_| "Model metadata endpoint returned invalid JSON.")
}

async fn get_json(http: &reqwest::Client, url: String) -> Result<Value, &'static str> {
    let response = http
        .get(url)
        .send()
        .await
        .map_err(|_| "Model metadata endpoint is unreachable or timed out.")?;
    json_response(response).await
}

async fn discover_target(http: &reqwest::Client, target: &Target) -> DiscoveryResult {
    let (info, models) = tokio::join!(
        get_json(http, format!("{}/v1/shaide/metadata", target.base)),
        get_json(http, format!("{}/v1/models", target.base)),
    );
    let info = info.ok();
    let models = models.ok();
    metadata::normalize(target, info.as_ref(), models.as_ref())
}

async fn discover_services(
    http: &reqwest::Client,
    services: Vec<Service>,
) -> PlatformModelsResponse {
    let mut result = PlatformModelsResponse::default();
    let mut targets = Vec::new();
    for service in services {
        match target(&service) {
            Ok(Some(target)) => targets.push(target),
            Ok(None) => {}
            Err(message) => result.warnings.push(PlatformModelWarning {
                source: format!(
                    "{}/{}",
                    service.namespace().unwrap_or_default(),
                    service.name_any()
                ),
                message: message.into(),
            }),
        }
    }
    let mut remaining = targets.len();
    let deadline = Instant::now() + DISCOVERY_TIMEOUT;
    let mut pending = stream::iter(targets)
        .map(|target| async move {
            let result = discover_target(http, &target).await;
            (target, result)
        })
        .buffer_unordered(8);
    while remaining > 0 {
        match tokio::time::timeout_at(deadline, pending.next()).await {
            Ok(Some((target, discovered))) => {
                remaining -= 1;
                match discovered {
                    Ok(mut found) => {
                        result.models.append(&mut found.models);
                        result.warnings.append(&mut found.warnings);
                    }
                    Err(message) => result.warnings.push(PlatformModelWarning {
                        source: target.source,
                        message: message.into(),
                    }),
                }
            }
            _ => {
                result.warnings.push(PlatformModelWarning {
                    source: "discovery".into(),
                    message: format!(
                        "{remaining} model Services did not complete discovery before the deadline."
                    ),
                });
                break;
            }
        }
    }
    remove_duplicates(&mut result);
    result.models.sort_by(|a, b| a.key().cmp(&b.key()));
    result
        .warnings
        .sort_by(|a, b| (&a.source, &a.message).cmp(&(&b.source, &b.message)));
    result
}

fn remove_duplicates(result: &mut PlatformModelsResponse) {
    let mut counts = BTreeMap::new();
    for model in &result.models {
        let (kind, name) = model.key();
        *counts.entry((kind, name.to_owned())).or_insert(0) += 1;
    }
    result.models.retain(|model| {
        let (kind, name) = model.key();
        counts[&(kind, name.to_owned())] == 1
    });
    for ((kind, name), count) in counts {
        if count > 1 {
            result.warnings.push(PlatformModelWarning {
                source: name,
                message: format!("Multiple {kind} Services advertise this served name; none were offered for registration."),
            });
        }
    }
}

#[cfg(test)]
mod tests;
