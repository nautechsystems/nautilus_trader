// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  You may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
//
//  Unless required by applicable law or agreed to in writing, software
//  distributed under the License is distributed on an "AS IS" BASIS,
//  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//  See the License for the specific language governing permissions and
//  limitations under the License.
// -------------------------------------------------------------------------------------------------

use std::sync::Arc;

use rustls::{ClientConfig, RootCertStore};
use webpki_roots;

use crate::providers::install_cryptographic_provider;

/// Loads a TLS client configuration with certificates.
#[must_use]
pub fn create_tls_config() -> Arc<ClientConfig> {
    install_cryptographic_provider();

    log::debug!("Loading certificates");

    let mut root_store = RootCertStore::empty();
    root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());

    let config = ClientConfig::builder()
        .with_root_certificates(root_store)
        .with_no_client_auth();

    Arc::new(config)
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use rustls::crypto::CryptoProvider;

    use super::*;

    #[rstest]
    fn test_create_tls_config_installs_default_provider() {
        // Must build without panicking and leave a usable process-default
        // provider, even when called as the first rustls use in the process.
        let _config = create_tls_config();
        assert!(CryptoProvider::get_default().is_some());

        // Second call exercises the idempotent install path
        let _config = create_tls_config();
        assert!(CryptoProvider::get_default().is_some());
    }
}
