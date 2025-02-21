// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

use std::sync::Once;

use rustls::crypto::{CryptoProvider, aws_lc_rs};

// Static flag to ensure the provider is installed only once
static INSTALL_PROVIDER: Once = Once::new();

pub fn install_cryptographic_provider() {
    INSTALL_PROVIDER.call_once(|| {
        if CryptoProvider::get_default().is_none() {
            tracing::debug!("Installing aws_lc_rs cryptographic provider");

            match aws_lc_rs::default_provider().install_default() {
                Ok(()) => tracing::debug!("Cryptographic provider installed successfully"),
                Err(e) => tracing::debug!("Error installing cryptographic provider: {e:?}"),
            }
        }
    });
}
