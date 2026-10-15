use std::{
    net::{IpAddr, Ipv4Addr},
    str::FromStr,
    sync::OnceLock,
};

use dotenv::EnvLoader;
use serde_json::Value;
use url::Url;

fn validate_jwt_secret(jwt_secret: String) -> String {
    assert!(
        jwt_secret.len() >= 32,
        "JWT_SECRET must be at least 32 bytes long"
    );
    jwt_secret
}

/// An MCP server configured through `MCP_SERVERS` instead of being discovered
/// in Kubernetes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaticMcpServer {
    /// Server id used by `/v1/list-mcps` and `/v1/mcp/proxy/{server_id}/...`.
    pub name: String,
    /// Base URL without a trailing slash; proxied paths are appended to it.
    pub url: String,
}

/// Parses `MCP_SERVERS`, a JSON array of `{"name", "url"}` objects. An unset or
/// blank value returns `None`, which keeps Kubernetes discovery. Error messages
/// never quote the input, since a rejected URL may contain a secret.
pub fn parse_mcp_servers(raw: Option<&str>) -> Result<Option<Vec<StaticMcpServer>>, String> {
    let Some(raw) = raw.filter(|raw| !raw.trim().is_empty()) else {
        return Ok(None);
    };
    let document: Value = serde_json::from_str(raw).map_err(|error| {
        format!(
            "not valid JSON at line {} column {}",
            error.line(),
            error.column()
        )
    })?;
    let entries = document
        .as_array()
        .ok_or("must be a JSON array of {\"name\", \"url\"} objects")?;
    let mut servers: Vec<StaticMcpServer> = Vec::with_capacity(entries.len());
    for (index, entry) in entries.iter().enumerate() {
        let server = parse_mcp_server(index, entry)?;
        if servers.iter().any(|existing| existing.name == server.name) {
            return Err(format!("entry {index}: duplicate name `{}`", server.name));
        }
        servers.push(server);
    }
    Ok(Some(servers))
}

fn parse_mcp_server(index: usize, entry: &Value) -> Result<StaticMcpServer, String> {
    let entry = entry
        .as_object()
        .ok_or_else(|| format!("entry {index} must be an object"))?;
    if entry.keys().any(|key| key != "name" && key != "url") {
        return Err(format!(
            "entry {index} has an unsupported field; only `name` and `url` are allowed"
        ));
    }
    let field = |key: &str| {
        entry
            .get(key)
            .ok_or_else(|| format!("entry {index} is missing `{key}`"))?
            .as_str()
            .ok_or_else(|| format!("`{key}` of entry {index} must be a string"))
    };
    let name = field("name")?;
    if !is_valid_mcp_server_name(name) {
        return Err(format!(
            "entry {index}: `name` must be 1-63 lowercase letters, digits or '-', \
             starting and ending with a letter or digit"
        ));
    }
    let url = normalize_mcp_server_url(field("url")?)
        .map_err(|reason| format!("entry {index}: `url` {reason}"))?;
    Ok(StaticMcpServer {
        name: name.to_owned(),
        url,
    })
}

/// Names are DNS labels, so they are safe as a single proxy path segment.
fn is_valid_mcp_server_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    (1..=63).contains(&bytes.len())
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'-')
        && bytes.first() != Some(&b'-')
        && bytes.last() != Some(&b'-')
}

fn normalize_mcp_server_url(raw: &str) -> Result<String, &'static str> {
    let url = Url::parse(raw).map_err(|_| "must be an absolute http(s) URL")?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err("must use http or https");
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(
            "must not contain credentials; callers send downstream credentials in x-mcp-authorization",
        );
    }
    if url.query().is_some() {
        return Err("must not contain a query");
    }
    if url.fragment().is_some() {
        return Err("must not contain a fragment");
    }
    Ok(url.as_str().trim_end_matches('/').to_owned())
}

fn mcp_servers_from_env(raw: Option<String>) -> Option<Vec<StaticMcpServer>> {
    parse_mcp_servers(raw.as_deref())
        .unwrap_or_else(|error| panic!("MCP_SERVERS is invalid: {error}"))
}

#[derive(Debug)]
pub struct RunTimeConfig {
    pub admin_password: String,
    pub jwt_secret: String,
    pub control_panel_fqdn: String,
    pub control_panel_port: String,
    pub host: IpAddr,
    pub port: u16,
    pub vector_db_url: String,
    pub mcp_namespace: String,
    pub mcp_label_selector: String,
    pub mcp_servers: Option<Vec<StaticMcpServer>>,
    pub mcp_collection_context_targets: crate::services::collection_context::ContextTargets,
    pub knowledge_center: Option<crate::services::knowledge_center::KcConfig>,
    pub is_trial: bool,
    pub webapp_url: Option<String>,
}

impl RunTimeConfig {
    fn from_env() -> Self {
        const DEFAULT_HOST: IpAddr = IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0));
        const DEFAULT_PORT: u16 = 8080;
        const DEFAULT_VECTOR_DB_URL: &str = "http://localhost:6334";
        if let Ok(env_map) = EnvLoader::new().load() {
            let admin_password = env_map
                .var("ADMIN_PASSWORD")
                .expect("ADMIN_PASSWORD not set");
            let jwt_secret =
                validate_jwt_secret(env_map.var("JWT_SECRET").expect("JWT_SECRET not set"));
            let control_panel_fqdn = env_map
                .var("SHAIDE_SERVER_UI_FQDN")
                .expect("SHAIDE_SERVER_UI_FQDN not set");
            let control_panel_port = env_map
                .var("SHAIDE_SERVER_UI_PORT")
                .expect("SHAIDE_SERVER_UI_PORT not set");
            let host = env_map
                .var("HOST")
                .map(|host| IpAddr::from_str(&host).unwrap())
                .unwrap_or(DEFAULT_HOST);
            let port = env_map
                .var("PORT")
                .map(|s| s.parse().unwrap())
                .unwrap_or(DEFAULT_PORT);
            let vector_db_url = env_map
                .var("VECTOR_DB_URL")
                .unwrap_or(DEFAULT_VECTOR_DB_URL.into());
            let mcp_namespace = env_map.var("MCP_NAMESPACE").unwrap_or_default();
            let mcp_label_selector = env_map.var("MCP_LABEL_SELECTOR").unwrap_or_default();
            let mcp_servers = mcp_servers_from_env(env_map.var("MCP_SERVERS").ok());
            let mcp_collection_context_targets =
                crate::services::collection_context::ContextTargets::parse(
                    env_map
                        .var("MCP_COLLECTION_CONTEXT_TARGETS")
                        .ok()
                        .as_deref(),
                    env_map
                        .var("MCP_COLLECTION_CONTEXT_ALLOW_LOCAL_HTTP")
                        .ok()
                        .as_deref(),
                    &jwt_secret,
                )
                .unwrap_or_else(|error| panic!("MCP collection context configuration: {error}"));
            let knowledge_center = crate::services::knowledge_center::KcConfig::parse(
                env_map.var("KNOWLEDGE_CENTER_URL").ok().as_deref(),
                env_map
                    .var("KNOWLEDGE_CENTER_INTERNAL_TOKEN")
                    .ok()
                    .as_deref(),
                env_map.var("S3_WEBHOOK_TOKEN").ok().as_deref(),
                env_map
                    .var("KNOWLEDGE_CENTER_ALLOW_LOCAL_HTTP")
                    .ok()
                    .as_deref()
                    == Some("true"),
            )
            .unwrap_or_else(|e| panic!("KC configuration: {e}"));
            if knowledge_center
                .as_ref()
                .is_some_and(|config| config.uses_token(&jwt_secret))
            {
                panic!("KC tokens must differ from JWT_SECRET");
            }
            let is_trial = env_map
                .var("TRIAL")
                .map(|s| s.eq_ignore_ascii_case("true"))
                .unwrap_or_default();
            let webapp_url = env_map.var("WEBAPP_URL").ok();
            Self {
                admin_password,
                jwt_secret,
                control_panel_fqdn,
                control_panel_port,
                host,
                port,
                vector_db_url,
                mcp_namespace,
                mcp_label_selector,
                mcp_servers,
                mcp_collection_context_targets,
                knowledge_center,
                is_trial,
                webapp_url,
            }
        } else {
            let admin_password =
                std::env::var("ADMIN_PASSWORD").expect("ADMIN_PASSWORD is not set");
            let jwt_secret =
                validate_jwt_secret(std::env::var("JWT_SECRET").expect("JWT_SECRET is not set"));
            let control_panel_fqdn =
                std::env::var("SHAIDE_SERVER_UI_FQDN").expect("SHAIDE_SERVER_UI_FQDN is not set");
            let control_panel_port =
                std::env::var("SHAIDE_SERVER_UI_PORT").expect("SHAIDE_SERVER_UI_PORT is not set");
            let host = std::env::var("HOST")
                .map(|host| IpAddr::from_str(&host).unwrap())
                .unwrap_or(DEFAULT_HOST);
            let port = std::env::var("PORT")
                .map(|s| s.parse().unwrap())
                .unwrap_or(DEFAULT_PORT);
            let vector_db_url =
                std::env::var("VECTOR_DB_URL").unwrap_or(DEFAULT_VECTOR_DB_URL.into());
            let mcp_namespace = std::env::var("MCP_NAMESPACE").unwrap_or_default();
            let mcp_label_selector = std::env::var("MCP_LABEL_SELECTOR").unwrap_or_default();
            let mcp_servers = mcp_servers_from_env(std::env::var("MCP_SERVERS").ok());
            let mcp_collection_context_targets =
                crate::services::collection_context::ContextTargets::parse(
                    std::env::var("MCP_COLLECTION_CONTEXT_TARGETS")
                        .ok()
                        .as_deref(),
                    std::env::var("MCP_COLLECTION_CONTEXT_ALLOW_LOCAL_HTTP")
                        .ok()
                        .as_deref(),
                    &jwt_secret,
                )
                .unwrap_or_else(|error| panic!("MCP collection context configuration: {error}"));
            let knowledge_center = crate::services::knowledge_center::KcConfig::parse(
                std::env::var("KNOWLEDGE_CENTER_URL").ok().as_deref(),
                std::env::var("KNOWLEDGE_CENTER_INTERNAL_TOKEN")
                    .ok()
                    .as_deref(),
                std::env::var("S3_WEBHOOK_TOKEN").ok().as_deref(),
                std::env::var("KNOWLEDGE_CENTER_ALLOW_LOCAL_HTTP")
                    .ok()
                    .as_deref()
                    == Some("true"),
            )
            .unwrap_or_else(|e| panic!("KC configuration: {e}"));
            if knowledge_center
                .as_ref()
                .is_some_and(|config| config.uses_token(&jwt_secret))
            {
                panic!("KC tokens must differ from JWT_SECRET");
            }
            let is_trial = std::env::var("TRIAL")
                .map(|s| s.eq_ignore_ascii_case("true"))
                .unwrap_or_default();
            let webapp_url = std::env::var("WEBAPP_URL").ok();
            Self {
                admin_password,
                jwt_secret,
                control_panel_fqdn,
                control_panel_port,
                host,
                port,
                vector_db_url,
                mcp_namespace,
                mcp_label_selector,
                mcp_servers,
                mcp_collection_context_targets,
                knowledge_center,
                is_trial,
                webapp_url,
            }
        }
    }
}

static ENVIRONMENT_CONFIG: OnceLock<RunTimeConfig> = OnceLock::new();

pub fn get_environment_config() -> &'static RunTimeConfig {
    ENVIRONMENT_CONFIG.get_or_init(RunTimeConfig::from_env)
}

#[cfg(test)]
mod tests {
    use super::{StaticMcpServer, parse_mcp_servers};

    fn parse_error(raw: &str) -> String {
        parse_mcp_servers(Some(raw)).expect_err("configuration should be rejected")
    }

    fn server(name: &str, url: &str) -> StaticMcpServer {
        StaticMcpServer {
            name: name.to_owned(),
            url: url.to_owned(),
        }
    }

    #[test]
    fn unset_or_blank_keeps_kubernetes_discovery() {
        assert_eq!(parse_mcp_servers(None), Ok(None));
        assert_eq!(parse_mcp_servers(Some("")), Ok(None));
        assert_eq!(parse_mcp_servers(Some("  \n")), Ok(None));
        assert_eq!(parse_mcp_servers(Some("[]")), Ok(Some(vec![])));
    }

    #[test]
    fn urls_are_normalized_and_keep_base_paths() {
        let raw = r#"[
            {"name": "search", "url": "http://mcp-search:8000/"},
            {"name": "tools-2", "url": "https://LOCALHOST:8443/mcp/base//"},
            {"name": "x", "url": "http://127.0.0.1:80"}
        ]"#;
        assert_eq!(
            parse_mcp_servers(Some(raw)),
            Ok(Some(vec![
                server("search", "http://mcp-search:8000"),
                server("tools-2", "https://localhost:8443/mcp/base"),
                server("x", "http://127.0.0.1"),
            ]))
        );
    }

    #[test]
    fn malformed_documents_are_rejected() {
        let cases = [
            ("[{", "not valid JSON"),
            (
                r#"{"name": "a", "url": "http://a"}"#,
                "must be a JSON array",
            ),
            (r#"["http://a"]"#, "entry 0 must be an object"),
            (r#"[{"url": "http://a"}]"#, "entry 0 is missing `name`"),
            (r#"[{"name": "a"}]"#, "entry 0 is missing `url`"),
            (
                r#"[{"name": 1, "url": "http://a"}]"#,
                "`name` of entry 0 must be a string",
            ),
            (
                r#"[{"name": "a", "url": 1}]"#,
                "`url` of entry 0 must be a string",
            ),
            (
                r#"[{"name": "a", "url": "http://a", "status": "Running"}]"#,
                "entry 0 has an unsupported field",
            ),
        ];
        for (raw, want) in cases {
            let error = parse_error(raw);
            assert!(error.contains(want), "{raw}: {error:?} lacks {want:?}");
        }
    }

    #[test]
    fn invalid_and_duplicate_names_are_rejected() {
        let long = "a".repeat(64);
        for name in [
            "",
            "Search",
            "-a",
            "a-",
            "a_b",
            "a/b",
            "a b",
            "..",
            long.as_str(),
        ] {
            let raw = serde_json::json!([{"name": name, "url": "http://a"}]).to_string();
            let error = parse_error(&raw);
            assert!(
                error.contains("entry 0: `name` must be"),
                "{name}: {error:?}"
            );
        }
        let error =
            parse_error(r#"[{"name": "a", "url": "http://a"}, {"name": "a", "url": "http://b"}]"#);
        assert!(error.contains("entry 1: duplicate name `a`"), "{error:?}");
    }

    #[test]
    fn unsafe_urls_are_rejected_without_echoing_them() {
        let cases = [
            ("relative/path", "absolute http(s) URL"),
            ("ftp://mcp-search", "http or https"),
            ("file:///etc/hosts", "http or https"),
            ("http://operator:leak-marker@mcp-search", "credentials"),
            ("http://leak-marker@mcp-search", "credentials"),
            ("http://mcp-search/#leak-marker", "fragment"),
            ("http://mcp-search/?token=leak-marker", "query"),
            ("http://mcp-search/?", "query"),
        ];
        for (url, want) in cases {
            let raw = serde_json::json!([{"name": "a", "url": url}]).to_string();
            let error = parse_error(&raw);
            assert!(error.contains(want), "{url}: {error:?} lacks {want:?}");
            assert!(error.starts_with("entry 0: `url`"), "{url}: {error:?}");
            assert!(!error.contains(url), "{error:?} echoes the URL");
            assert!(!error.contains("leak-marker"), "{error:?} echoes the URL");
        }
    }

    #[test]
    fn errors_stay_bounded() {
        let huge = "x".repeat(100_000);
        let cases = [
            serde_json::json!([{"name": huge, "url": "http://a"}]).to_string(),
            serde_json::json!([{"name": "a", "url": format!("http://{huge}@a")}]).to_string(),
            serde_json::json!([{"name": "a", "url": "http://a", huge.as_str(): 1}]).to_string(),
            format!("[{huge}"),
        ];
        for raw in cases {
            let error = parse_error(&raw);
            assert!(error.len() < 200, "{} byte error", error.len());
        }
    }
}
