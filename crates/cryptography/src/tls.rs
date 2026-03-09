use std::sync::Arc;

use rustls::{ClientConfig, RootCertStore};
use webpki_roots;

/// Loads a TLS client configuration with certificates.
pub fn create_tls_config() -> Arc<ClientConfig> {
    log::debug!("Loading certificates");

    let mut root_store = RootCertStore::empty();
    root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());

    let config = ClientConfig::builder()
        .with_root_certificates(root_store)
        .with_no_client_auth();

    Arc::new(config)
}
