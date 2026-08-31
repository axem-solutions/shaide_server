use std::sync::Arc;

use axum_reverse_proxy::TargetResolver;
use dashmap::DashMap;
use futures::{StreamExt, TryStreamExt};
use k8s_openapi::api::core::v1::Pod;
use kube::{
    Api, Client, Config, ResourceExt,
    api::{ListParams, WatchEvent, WatchParams},
};
use serde::{Deserialize, Serialize};
use shaide_common::api::mcp::{McpServerResponse, McpServerStatusResponse};
use tokio::sync::OnceCell;
use tracing::{debug, warn};

use crate::{config::get_environment_config, error::ShaideError};

#[derive(Clone, Debug)]
pub struct ResolvedMcpTarget(pub String);

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub enum McpStatus {
    Starting,
    Running,
    Restarting,
}

impl McpStatus {
    fn into_response(self) -> McpServerStatusResponse {
        match self {
            Self::Starting => McpServerStatusResponse::Starting,
            Self::Running => McpServerStatusResponse::Running,
            Self::Restarting => McpServerStatusResponse::Restarting,
        }
    }
}

impl McpStatus {
    fn priority(&self) -> i8 {
        match self {
            Self::Restarting => 3,
            Self::Starting => 2,
            Self::Running => 1,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct McpServer {
    name: String,
    status: McpStatus,
    url: String,
}

impl McpServer {
    pub fn into_response(self) -> McpServerResponse {
        McpServerResponse {
            name: self.name,
            status: self.status.into_response(),
            url: self.url,
        }
    }

    fn try_from_pod(pod: Pod) -> Option<Self> {
        let name = pod.annotations().get("mcp.shaide/datasource")?.to_owned();
        let url = pod.annotations().get("mcp.shaide/url")?.to_owned();
        let status = pod.status.as_ref()?;
        let status = if let Some(conditions) = &status.conditions
            && conditions
                .iter()
                .any(|c| c.type_ == "Ready" && c.status == "True")
        {
            McpStatus::Running
        } else if let Some(container_status) = &status.container_statuses
            && container_status.iter().any(|s| s.restart_count > 0)
        {
            McpStatus::Restarting
        } else {
            McpStatus::Starting
        };
        Some(Self { name, status, url })
    }
}

#[derive(Debug, Clone)]
pub struct McpService {
    servers: Arc<DashMap<String, McpServer>>,
}

impl Default for McpService {
    fn default() -> Self {
        Self {
            servers: Arc::new(DashMap::new()),
        }
    }
}

impl McpService {
    async fn update_pod_list(&mut self, pods: Api<Pod>, lp: ListParams) -> anyhow::Result<String> {
        let list = pods.list(&lp).await?;
        let resource_version = list
            .metadata
            .resource_version
            .ok_or_else(|| anyhow::anyhow!("pod list response did not include resourceVersion"))?;
        self.servers.clear();
        for (name, server) in list.items.into_iter().filter_map(|pod| {
            let name = pod.metadata.name.clone()?;
            let mcp_server = McpServer::try_from_pod(pod)?;
            Some((name, mcp_server))
        }) {
            self.servers.insert(name, server);
        }
        Ok(resource_version)
    }

    fn insert_pod(&mut self, pod: Pod) -> Option<String> {
        let resource_version = pod.metadata.resource_version.clone();
        let Some(pod_name) = pod.metadata.name.clone() else {
            return resource_version;
        };
        let Some(mcp_server) = McpServer::try_from_pod(pod) else {
            return resource_version;
        };
        self.servers.insert(pod_name, mcp_server);
        resource_version
    }

    fn delete_pod(&mut self, pod: Pod) -> Option<String> {
        let resource_version = pod.metadata.resource_version.clone();
        let Some(pod_name) = pod.metadata.name.clone() else {
            debug!("Delete event received, but pod name was not supplied");
            return resource_version;
        };
        let removed = self.servers.remove(&pod_name);
        if removed.is_none() {
            debug!("Delete event received, but pod was not found: {pod_name}");
        }
        resource_version
    }
}

async fn watch_pods(
    pods: Api<Pod>,
    mut service: McpService,
    wp: WatchParams,
    mut resource_version: String,
    lp: ListParams,
) {
    loop {
        let result = async {
            let mut watchers = pods.watch(&wp, &resource_version).await?.boxed();
            while let Some(event) = watchers.try_next().await? {
                debug!("Event received: {event:?}");
                match event {
                    WatchEvent::Added(pod) | WatchEvent::Modified(pod) => {
                        if let Some(rv) = service.insert_pod(pod) {
                            resource_version = rv;
                        }
                    }
                    WatchEvent::Deleted(pod) => {
                        if let Some(rv) = service.delete_pod(pod) {
                            resource_version = rv;
                        }
                    }
                    WatchEvent::Bookmark(bookmark) => {
                        resource_version = bookmark.metadata.resource_version.clone();
                    }
                    WatchEvent::Error(status) => {
                        resource_version =
                            service.update_pod_list(pods.clone(), lp.clone()).await?;
                        anyhow::bail!("Kubernetes watch error: {status:?}");
                    }
                }
            }
            anyhow::Ok(())
        }
        .await;

        match result {
            Ok(()) => warn!(resource_version, "MCP pod watch ended; restarting"),
            Err(err) => warn!(?err, resource_version, "MCP pod watch failed; restarting"),
        }
    }
}

impl McpService {
    async fn new(mcp_namespace: &str, mcp_label_selector: &str) -> anyhow::Result<Self> {
        let config = Config::infer().await?;
        let client = Client::try_from(config)?;
        let pods: Api<Pod> = Api::namespaced(client.clone(), mcp_namespace);
        let lp = ListParams::default().labels(mcp_label_selector);
        let mut mcp_service = McpService::default();
        let resource_version = mcp_service
            .update_pod_list(pods.clone(), lp.clone())
            .await?;
        let wp = WatchParams::default().labels(mcp_label_selector);
        tokio::spawn(watch_pods(
            pods,
            mcp_service.clone(),
            wp,
            resource_version,
            lp,
        ));
        Ok(mcp_service)
    }

    pub fn get_service_url(&self, server_name: &str) -> Option<String> {
        let server = self.servers.iter().fold(None, |acc, val| {
            if val.name != server_name {
                return acc;
            }
            let Some(acc) = acc else {
                return Some(val);
            };
            if acc.status.priority() >= val.status.priority() {
                Some(acc)
            } else {
                Some(val)
            }
        });
        server.map(|s| s.url.clone())
    }

    pub async fn get_services(&self) -> Vec<McpServer> {
        self.servers.iter().map(|srvs| srvs.clone()).collect()
    }
}

impl TargetResolver for &'static McpService {
    fn resolve(
        &self,
        req: &http::Request<axum::body::Body>,
        params: &[(String, String)],
    ) -> String {
        let target_base = req
            .extensions()
            .get::<ResolvedMcpTarget>()
            .unwrap()
            .0
            .trim_end_matches('/');
        let path = params
            .iter()
            .find(|(k, _)| k == "path")
            .map(|(_, v)| v.to_string())
            .unwrap_or_default();
        let query = req
            .uri()
            .query()
            .map(|q| format!("?{q}"))
            .unwrap_or_default();
        format!("{target_base}/{path}{query}")
    }
}

static MCP_SERVICE: OnceCell<McpService> = OnceCell::const_new();

pub async fn get_mcp_service() -> Result<&'static McpService, ShaideError> {
    let config = get_environment_config();
    let service = MCP_SERVICE
        .get_or_try_init(|| McpService::new(&config.mcp_namespace, &config.mcp_label_selector))
        .await
        .unwrap();
    Ok(service)
}
