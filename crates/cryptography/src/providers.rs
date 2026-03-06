use std::sync::Once;

use rustls::crypto::{CryptoProvider, aws_lc_rs};

// Static flag to ensure the provider is installed only once
static INSTALL_PROVIDER: Once = Once::new();

/// Installs the AWS-LC cryptographic provider as the default for rustls.
///
/// This function ensures that the cryptographic provider is installed only once
/// using a static guard. If no default provider is already set, it will install
/// the AWS-LC provider and log the result.
pub fn install_cryptographic_provider() {
    INSTALL_PROVIDER.call_once(|| {
        if CryptoProvider::get_default().is_none() {
            log::debug!("Installing aws_lc_rs cryptographic provider");

            match aws_lc_rs::default_provider().install_default() {
                Ok(()) => log::debug!("Cryptographic provider installed successfully"),
                Err(e) => log::debug!("Error installing cryptographic provider: {e:?}"),
            }
        }
    });
}
