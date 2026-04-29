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

use crate::{
    common::enums::HyperliquidEnvironment,
    config::{HyperliquidDataClientConfig, HyperliquidExecClientConfig},
};

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl HyperliquidDataClientConfig {
    /// Configuration for the Hyperliquid data client.
    #[new]
    #[pyo3(signature = (
        environment = None,
        private_key = None,
        base_url_ws = None,
        base_url_http = None,
        proxy_url = None,
        http_timeout_secs = None,
        ws_timeout_secs = None,
        update_instruments_interval_mins = None,
    ))]
    #[expect(clippy::too_many_arguments)]
    fn py_new(
        environment: Option<HyperliquidEnvironment>,
        private_key: Option<String>,
        base_url_ws: Option<String>,
        base_url_http: Option<String>,
        proxy_url: Option<String>,
        http_timeout_secs: Option<u64>,
        ws_timeout_secs: Option<u64>,
        update_instruments_interval_mins: Option<u64>,
    ) -> Self {
        let defaults = Self::default();
        Self {
            private_key,
            base_url_ws,
            base_url_http,
            proxy_url,
            environment: environment.unwrap_or(defaults.environment),
            http_timeout_secs: http_timeout_secs.unwrap_or(defaults.http_timeout_secs),
            ws_timeout_secs: ws_timeout_secs.unwrap_or(defaults.ws_timeout_secs),
            update_instruments_interval_mins: update_instruments_interval_mins
                .unwrap_or(defaults.update_instruments_interval_mins),
            transport_backend: defaults.transport_backend,
        }
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl HyperliquidExecClientConfig {
    /// Configuration for the Hyperliquid execution client.
    #[new]
    #[pyo3(signature = (
        private_key = None,
        vault_address = None,
        account_address = None,
        environment = None,
        base_url_ws = None,
        base_url_http = None,
        base_url_exchange = None,
        proxy_url = None,
        http_timeout_secs = None,
        max_retries = None,
        retry_delay_initial_ms = None,
        retry_delay_max_ms = None,
        normalize_prices = None,
        market_order_slippage_bps = None,
    ))]
    #[expect(clippy::too_many_arguments)]
    fn py_new(
        private_key: Option<String>,
        vault_address: Option<String>,
        account_address: Option<String>,
        environment: Option<HyperliquidEnvironment>,
        base_url_ws: Option<String>,
        base_url_http: Option<String>,
        base_url_exchange: Option<String>,
        proxy_url: Option<String>,
        http_timeout_secs: Option<u64>,
        max_retries: Option<u32>,
        retry_delay_initial_ms: Option<u64>,
        retry_delay_max_ms: Option<u64>,
        normalize_prices: Option<bool>,
        market_order_slippage_bps: Option<u32>,
    ) -> Self {
        let defaults = Self::default();
        Self {
            private_key,
            vault_address,
            account_address,
            base_url_ws,
            base_url_http,
            base_url_exchange,
            proxy_url,
            environment: environment.unwrap_or(defaults.environment),
            http_timeout_secs: http_timeout_secs.unwrap_or(defaults.http_timeout_secs),
            max_retries: max_retries.unwrap_or(defaults.max_retries),
            retry_delay_initial_ms: retry_delay_initial_ms
                .unwrap_or(defaults.retry_delay_initial_ms),
            retry_delay_max_ms: retry_delay_max_ms.unwrap_or(defaults.retry_delay_max_ms),
            normalize_prices: normalize_prices.unwrap_or(defaults.normalize_prices),
            market_order_slippage_bps: market_order_slippage_bps
                .unwrap_or(defaults.market_order_slippage_bps),
            transport_backend: defaults.transport_backend,
        }
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }
}
