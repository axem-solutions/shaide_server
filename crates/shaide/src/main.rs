#[cfg(target_family = "unix")]
use std::os::unix::fs::PermissionsExt;

use shaide::{error_formatting::install_color_eyre, install_rustls_crypto_provider, logger};

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
    shaide::start_server().await
}
