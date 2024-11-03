// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

use rustls::{self, ClientConfig, RootCertStore};
use rustls_native_certs::load_native_certs;

// TODO: We could disentangle and extract network.tls and add the functionality here
pub fn create_tls_config() -> Arc<ClientConfig> {
    tracing::info!("Loading native certificates");
    let mut root_store = RootCertStore::empty();
    let cert_result = load_native_certs();
    for e in cert_result.errors {
        tracing::error!("Error loading certificates: {e}");
    }
    root_store.add_parsable_certificates(cert_result.certs);

    Arc::new(
        ClientConfig::builder()
            .with_root_certificates(root_store)
            .with_no_client_auth(),
    )
}
