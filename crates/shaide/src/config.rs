use std::{
    net::{IpAddr, Ipv4Addr},
    str::FromStr,
    sync::OnceLock,
};

use dotenv::EnvLoader;

fn validate_jwt_secret(jwt_secret: String) -> String {
    assert!(
        jwt_secret.len() >= 32,
        "JWT_SECRET must be at least 32 bytes long"
    );
    jwt_secret
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
    pub is_trial: bool,
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
            let is_trial = env_map
                .var("TRIAL")
                .map(|s| s.eq_ignore_ascii_case("true"))
                .unwrap_or_default();
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
                is_trial,
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
            let is_trial = std::env::var("TRIAL")
                .map(|s| s.eq_ignore_ascii_case("true"))
                .unwrap_or_default();
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
                is_trial,
            }
        }
    }
}

static ENVIRONMENT_CONFIG: OnceLock<RunTimeConfig> = OnceLock::new();

pub fn get_environment_config() -> &'static RunTimeConfig {
    ENVIRONMENT_CONFIG.get_or_init(RunTimeConfig::from_env)
}
