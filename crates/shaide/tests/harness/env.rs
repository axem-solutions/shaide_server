//! Process-global setup every [`TestServer`](super::TestServer) goes through before it touches
//! any server code.
//!
//! Two pieces of the server are process-global and initialize themselves on first use:
//! [`get_environment_config`] (a `OnceLock` over the environment) and the shaide root path (a
//! `LazyLock` over `shaide_ROOT`). The auth service is derived from the configuration and is
//! global too. All of them are therefore set up exactly once per test binary, here, before the
//! first test constructs anything.

use std::{
    path::{Path, PathBuf},
    sync::LazyLock,
};

use shaide::config::{RunTimeConfig, get_environment_config};

/// Password the harness gives the admin user. Tests read it from [`TestEnv::admin_password`]
/// rather than hard-coding it, so an environment that already pins `ADMIN_PASSWORD` keeps working.
const ADMIN_PASSWORD: &str = "integration-test-admin-password";

/// `JWT_SECRET` is asserted to be at least 32 bytes by the configuration itself.
const JWT_SECRET: &str = "integration-test-jwt-secret-that-is-long-enough";

/// Deliberately unroutable: a test that reaches Qdrant instead of staying hermetic fails fast
/// rather than hanging or, worse, hitting a developer's local instance.
const UNREACHABLE_VECTOR_DB_URL: &str = "http://127.0.0.1:1";

pub struct TestEnv {
    /// `shaide_ROOT` for this test binary — an empty directory under `target/tmp`, so the logs and
    /// database of the machine running the tests are never read or written.
    pub root: PathBuf,
    pub admin_password: String,
}

static TEST_ENV: LazyLock<TestEnv> = LazyLock::new(init);

/// Initializes the process-global server state and returns the values tests need from it.
pub fn test_env() -> &'static TestEnv {
    &TEST_ENV
}

fn init() -> TestEnv {
    let root = isolated_root();

    // SAFETY: `std::env::set_var` is only sound while no other thread reads the environment. This
    // runs inside the `LazyLock` that every harness entry point goes through, before any test has
    // constructed a server, so nothing else in the process is looking at the environment yet.
    unsafe {
        std::env::set_var("shaide_ROOT", &root);

        std::env::set_var("ADMIN_PASSWORD", ADMIN_PASSWORD);
        std::env::set_var("JWT_SECRET", JWT_SECRET);
        std::env::set_var("HOST", "127.0.0.1");
        std::env::set_var("PORT", "0");
        std::env::set_var("SHAIDE_SERVER_UI_FQDN", "127.0.0.1");
        std::env::set_var("SHAIDE_SERVER_UI_PORT", "3000");
        std::env::set_var("VECTOR_DB_URL", UNREACHABLE_VECTOR_DB_URL);
        std::env::set_var("MCP_NAMESPACE", "");
        std::env::set_var("MCP_LABEL_SELECTOR", "");
        std::env::set_var("TRIAL", "false");
    }

    shaide::install_rustls_crypto_provider();

    // Forces the `OnceLock` to initialize from the environment we just wrote, and reports back
    // what it actually resolved to.
    let RunTimeConfig { admin_password, .. } = get_environment_config();

    TestEnv {
        root,
        admin_password: admin_password.clone(),
    }
}

/// A fresh `shaide_ROOT` per test binary, under `target/tmp` so `cargo clean` collects it.
fn isolated_root() -> PathBuf {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "shaide-root-{}-{}",
        option_env!("CARGO_CRATE_NAME").unwrap_or("tests"),
        std::process::id()
    ));
    // A previous run that crashed before cleanup must not leak anything into this one.
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("test root should be creatable");
    root
}
