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

//! Configuration structures for the Bullet adapter.

use crate::common::{
    consts::{http_url, ws_url},
    enums::BulletEnvironment,
};

/// Configuration for the Bullet data client.
#[derive(Clone, Debug, bon::Builder)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.bullet",
        from_py_object
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.bullet")
)]
pub struct BulletDataClientConfig {
    /// Override for the HTTP base URL.
    pub base_url_http: Option<String>,
    /// Override for the WebSocket URL.
    pub base_url_ws: Option<String>,
    /// Optional proxy URL.
    pub proxy_url: Option<String>,
    /// The target environment.
    #[builder(default)]
    pub environment: BulletEnvironment,
    /// HTTP timeout in seconds.
    #[builder(default = 60)]
    pub http_timeout_secs: u64,
    /// Interval for refreshing instruments in minutes.
    #[builder(default = 60)]
    pub update_instruments_interval_mins: u64,
}

impl Default for BulletDataClientConfig {
    fn default() -> Self {
        Self::builder().build()
    }
}

impl BulletDataClientConfig {
    /// Returns the HTTP base URL.
    #[must_use]
    pub fn http_url(&self) -> String {
        self.base_url_http
            .clone()
            .unwrap_or_else(|| http_url(self.environment).to_string())
    }

    /// Returns the WebSocket URL.
    #[must_use]
    pub fn ws_url(&self) -> String {
        self.base_url_ws
            .clone()
            .unwrap_or_else(|| ws_url(self.environment).to_string())
    }
}

/// Configuration for the Bullet execution client.
#[derive(Clone, Debug, bon::Builder)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.bullet",
        from_py_object
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.bullet")
)]
pub struct BulletExecClientConfig {
    /// Ed25519 delegate private key as hex string.
    ///
    /// Falls back to `BULLET_PRIVATE_KEY` environment variable when absent.
    pub private_key: Option<String>,
    /// Path to Solana-compatible JSON keystore.
    ///
    /// Falls back to `BULLET_KEY_FILE` environment variable when absent.
    pub key_file: Option<String>,
    /// Main account address (base58).
    ///
    /// When absent, derive from the signing key (no-delegate mode).
    /// Falls back to `BULLET_ACCOUNT_ADDRESS` environment variable when absent.
    pub account_address: Option<String>,
    /// Override for the HTTP base URL.
    pub base_url_http: Option<String>,
    /// Override for the WebSocket URL.
    pub base_url_ws: Option<String>,
    /// Optional proxy URL.
    pub proxy_url: Option<String>,
    /// The target environment.
    #[builder(default)]
    pub environment: BulletEnvironment,
    /// HTTP timeout in seconds.
    #[builder(default = 60)]
    pub http_timeout_secs: u64,
    /// Maximum number of retry attempts.
    #[builder(default = 3)]
    pub max_retries: u32,
    /// Initial retry delay in milliseconds.
    #[builder(default = 1000)]
    pub retry_delay_initial_ms: u64,
    /// Maximum retry delay in milliseconds.
    #[builder(default = 10_000)]
    pub retry_delay_max_ms: u64,
}

impl Default for BulletExecClientConfig {
    fn default() -> Self {
        Self::builder().build()
    }
}

impl BulletExecClientConfig {
    /// Returns `true` when credentials (key or key_file) are available.
    #[must_use]
    pub fn has_credentials(&self) -> bool {
        self.private_key
            .as_deref()
            .is_some_and(|s| !s.trim().is_empty())
            || self
                .key_file
                .as_deref()
                .is_some_and(|s| !s.trim().is_empty())
    }

    /// Returns the HTTP base URL.
    #[must_use]
    pub fn http_url(&self) -> String {
        self.base_url_http
            .clone()
            .unwrap_or_else(|| http_url(self.environment).to_string())
    }

    /// Returns the WebSocket URL.
    #[must_use]
    pub fn ws_url(&self) -> String {
        self.base_url_ws
            .clone()
            .unwrap_or_else(|| ws_url(self.environment).to_string())
    }
}
