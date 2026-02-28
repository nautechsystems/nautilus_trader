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

//! Python bindings for Hyperliquid configuration.

use pyo3::prelude::*;

use crate::config::{HyperliquidDataClientConfig, HyperliquidExecClientConfig};

#[pymethods]
impl HyperliquidDataClientConfig {
    #[new]
    #[pyo3(signature = (
        is_testnet = None,
        private_key = None,
        base_url_ws = None,
        base_url_http = None,
        http_proxy_url = None,
        http_timeout_secs = None,
        ws_timeout_secs = None,
        update_instruments_interval_mins = None,
    ))]
    #[allow(clippy::too_many_arguments)]
    fn py_new(
        is_testnet: Option<bool>,
        private_key: Option<String>,
        base_url_ws: Option<String>,
        base_url_http: Option<String>,
        http_proxy_url: Option<String>,
        http_timeout_secs: Option<u64>,
        ws_timeout_secs: Option<u64>,
        update_instruments_interval_mins: Option<u64>,
    ) -> Self {
        let defaults = Self::default();
        Self {
            private_key,
            base_url_ws,
            base_url_http,
            http_proxy_url,
            ws_proxy_url: None,
            is_testnet: is_testnet.unwrap_or(defaults.is_testnet),
            http_timeout_secs: http_timeout_secs.or(defaults.http_timeout_secs),
            ws_timeout_secs: ws_timeout_secs.or(defaults.ws_timeout_secs),
            update_instruments_interval_mins: update_instruments_interval_mins
                .or(defaults.update_instruments_interval_mins),
        }
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }
}

#[pymethods]
impl HyperliquidExecClientConfig {
    #[new]
    #[pyo3(signature = (
        private_key = None,
        vault_address = None,
        is_testnet = None,
        base_url_ws = None,
        base_url_http = None,
        base_url_exchange = None,
        http_proxy_url = None,
        http_timeout_secs = None,
        max_retries = None,
        retry_delay_initial_ms = None,
        retry_delay_max_ms = None,
        normalize_prices = None,
    ))]
    #[allow(clippy::too_many_arguments)]
    fn py_new(
        private_key: Option<String>,
        vault_address: Option<String>,
        is_testnet: Option<bool>,
        base_url_ws: Option<String>,
        base_url_http: Option<String>,
        base_url_exchange: Option<String>,
        http_proxy_url: Option<String>,
        http_timeout_secs: Option<u64>,
        max_retries: Option<u32>,
        retry_delay_initial_ms: Option<u64>,
        retry_delay_max_ms: Option<u64>,
        normalize_prices: Option<bool>,
    ) -> Self {
        let defaults = Self::default();
        Self {
            private_key,
            vault_address,
            base_url_ws,
            base_url_http,
            base_url_exchange,
            http_proxy_url,
            ws_proxy_url: None,
            is_testnet: is_testnet.unwrap_or(defaults.is_testnet),
            http_timeout_secs: http_timeout_secs.unwrap_or(defaults.http_timeout_secs),
            max_retries: max_retries.unwrap_or(defaults.max_retries),
            retry_delay_initial_ms: retry_delay_initial_ms
                .unwrap_or(defaults.retry_delay_initial_ms),
            retry_delay_max_ms: retry_delay_max_ms.unwrap_or(defaults.retry_delay_max_ms),
            normalize_prices: normalize_prices.unwrap_or(defaults.normalize_prices),
        }
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }
}
