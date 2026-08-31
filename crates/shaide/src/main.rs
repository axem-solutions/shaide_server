mod config;
mod error;
mod error_formatting;
mod logger;
mod middlewares;
mod openapi;
mod providers;
mod routes;
mod services;
mod utils;

#[cfg(target_family = "unix")]
use std::os::unix::fs::PermissionsExt;
use std::{net::SocketAddr, sync::Arc};

use axum::{
    Router,
    middleware::from_fn,
    routing::{self},
};
use axum_prometheus::PrometheusMetricLayer;
use axum_reverse_proxy::ReverseProxy;
use shaide_common::path::get_db_file;
use shaide_db::{
    DbConn, Role,
    error::{Resource, ShaideDBError},
};
use tower_http::cors::CorsLayer;
use tracing::debug;

use crate::{
    config::get_environment_config,
    error_formatting::install_color_eyre,
    middlewares::{
        error_response::map_error_response, forward_headers_middleware, logging_middleware,
    },
    routes::{fallback_404, metrics::metrics},
    services::{
        auth::{AuthService, get_auth_service},
        health::HealthState,
    },
};

const ADMIN_USERNAME: &str = "admin";

async fn ensure_admin(
    db: &DbConn,
    password: &str,
    auth_service: &AuthService,
) -> anyhow::Result<()> {
    match db.get_user_by_username(ADMIN_USERNAME).await {
        Ok(user) if user.role == Role::Admin => Ok(()),
        Ok(_) => anyhow::bail!("user named '{ADMIN_USERNAME}' exists but is not an admin"),
        Err(ShaideDBError::NotFound(Resource::User)) => {
            let password_hash = auth_service.hash_password(password.to_owned()).await?;
            db.create_admin(ADMIN_USERNAME.to_owned(), password_hash)
                .await?;
            Ok(())
        }
        Err(error) => Err(error.into()),
    }
}

fn install_rustls_crypto_provider() {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
}

pub async fn api_router(health_state: Arc<HealthState>, db: DbConn) -> Router {
    Router::new()
        .merge(crate::routes::health::health_router(health_state))
        .merge(crate::routes::trial::trial_router())
        .merge(crate::routes::completions::completion_router(db.clone()))
        .merge(crate::routes::chat::chat_router(db.clone()))
        .merge(crate::routes::responses::responses_router(db.clone()))
        .merge(crate::routes::users::users_router(db.clone()))
        .merge(crate::routes::models::model_router(db.clone()))
        .merge(crate::routes::embedding::embedding_router(db.clone()))
        .merge(crate::routes::vector_db::vector_db_router(db.clone()))
        .merge(crate::routes::statistics::statistics_router(db.clone()))
        .merge(crate::routes::mcp::mcp_user_routes(db.clone()).await)
        .merge(crate::routes::logs::logs_router())
        .merge(crate::openapi::openapi_router())
}

// TODO: this will need to be structued better
pub async fn start_server() {
    let environment_config = get_environment_config();
    let host = environment_config.host;
    let port = environment_config.port;
    let address = SocketAddr::from((host, port));

    debug!(
        bind_addr = %address.ip(),
        port = address.port(),
        "Starting shaide server"
    );

    let db = DbConn::new(get_db_file().as_path())
        .await
        .expect("Must be able to initialize db");
    ensure_admin(&db, &environment_config.admin_password, get_auth_service())
        .await
        .expect("Must be able to ensure admin user");
    let health_state = Arc::new(HealthState::new());
    let router = api_router(health_state, db).await;

    let (prometheus_layer, prometheus_handle) = PrometheusMetricLayer::pair();

    let control_panel_uri = format!(
        "http://{}:{}/control-panel",
        environment_config.control_panel_fqdn.clone(),
        environment_config.control_panel_port.clone()
    );

    let app = router
        .merge(ReverseProxy::new("/control-panel", &control_panel_uri))
        .layer(CorsLayer::permissive())
        .layer(prometheus_layer)
        .route(
            "/metrics",
            routing::get(metrics).with_state(Arc::new(prometheus_handle)),
        )
        .fallback(fallback_404);

    let app = app
        .layer(from_fn(logging_middleware))
        .layer(from_fn(forward_headers_middleware))
        .layer(from_fn(map_error_response));
    let version = env!("CARGO_PKG_VERSION");
    println!(
        r#"
📄 Version {version}
🚀 Listening at http://{address}
"#
    );
    let listener = tokio::net::TcpListener::bind(address).await.unwrap();

    ::axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    .unwrap_or_else(|err| {
        tracing::error!(message = %format!("Error occurred during serving: {}", err), "fatal error");
        std::process::exit(1);
    })
}

#[tokio::main]
async fn main() {
    install_rustls_crypto_provider();
    let root = shaide_common::path::shaide_root();
    std::fs::create_dir_all(&root).expect("Must be able to create shaide root");
    #[cfg(target_family = "unix")]
    {
        let mut permissions = std::fs::metadata(&root).unwrap().permissions();
        permissions.set_mode(0o700);
        std::fs::set_permissions(&root, permissions).unwrap();
    }
    logger::init_tracing();
    install_color_eyre().expect("Must be able to install color_eyre");
    start_server().await
}
