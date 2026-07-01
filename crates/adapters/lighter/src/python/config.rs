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

//! Python bindings for Lighter configuration.

use nautilus_model::identifiers::{AccountId, TraderId};
use nautilus_network::websocket::TransportBackend;
use pyo3::pymethods;

use crate::{
    common::enums::LighterEnvironment,
    config::{LighterDataClientConfig, LighterExecClientConfig},
};

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl LighterDataClientConfig {
    /// Configuration for the Lighter live data client.
    #[new]
    #[pyo3(signature = (
        base_url_http = None,
        base_url_ws = None,
        proxy_url = None,
        environment = None,
        account_index = None,
        api_key_index = None,
        private_key = None,
        http_timeout_secs = None,
        ws_timeout_secs = None,
        update_instruments_interval_mins = None,
        rest_quota_per_min = None,
        transport_backend = None,
    ))]
    #[expect(clippy::too_many_arguments)]
    fn py_new(
        base_url_http: Option<String>,
        base_url_ws: Option<String>,
        proxy_url: Option<String>,
        environment: Option<LighterEnvironment>,
        account_index: Option<u64>,
        api_key_index: Option<u8>,
        private_key: Option<String>,
        http_timeout_secs: Option<u64>,
        ws_timeout_secs: Option<u64>,
        update_instruments_interval_mins: Option<u64>,
        rest_quota_per_min: Option<u32>,
        transport_backend: Option<TransportBackend>,
    ) -> Self {
        let defaults = Self::default();
        Self {
            base_url_http,
            base_url_ws,
            proxy_url,
            environment: environment.unwrap_or(defaults.environment),
            account_index,
            api_key_index,
            private_key,
            http_timeout_secs: http_timeout_secs.unwrap_or(defaults.http_timeout_secs),
            ws_timeout_secs: ws_timeout_secs.unwrap_or(defaults.ws_timeout_secs),
            update_instruments_interval_mins: update_instruments_interval_mins
                .unwrap_or(defaults.update_instruments_interval_mins),
            rest_quota_per_min,
            transport_backend: transport_backend.unwrap_or(defaults.transport_backend),
        }
    }

    #[getter]
    fn proxy_url(&self) -> Option<String> {
        self.proxy_url.clone()
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl LighterExecClientConfig {
    /// Configuration for the Lighter live execution client.
    #[new]
    #[pyo3(signature = (
        trader_id,
        account_id,
        account_index = None,
        api_key_index = None,
        private_key = None,
        base_url_http = None,
        base_url_ws = None,
        proxy_url = None,
        environment = None,
        http_timeout_secs = None,
        ws_timeout_secs = None,
        market_order_slippage_bps = None,
        rest_quota_per_min = None,
        sendtx_quota_per_min = None,
        transport_backend = None,
    ))]
    #[expect(clippy::too_many_arguments)]
    fn py_new(
        trader_id: TraderId,
        account_id: AccountId,
        account_index: Option<u64>,
        api_key_index: Option<u8>,
        private_key: Option<String>,
        base_url_http: Option<String>,
        base_url_ws: Option<String>,
        proxy_url: Option<String>,
        environment: Option<LighterEnvironment>,
        http_timeout_secs: Option<u64>,
        ws_timeout_secs: Option<u64>,
        market_order_slippage_bps: Option<u32>,
        rest_quota_per_min: Option<u32>,
        sendtx_quota_per_min: Option<u32>,
        transport_backend: Option<TransportBackend>,
    ) -> Self {
        let defaults = Self::builder()
            .trader_id(trader_id)
            .account_id(account_id)
            .build();
        Self {
            trader_id,
            account_id,
            account_index,
            api_key_index,
            private_key,
            base_url_http,
            base_url_ws,
            proxy_url,
            environment: environment.unwrap_or(defaults.environment),
            http_timeout_secs: http_timeout_secs.unwrap_or(defaults.http_timeout_secs),
            ws_timeout_secs: ws_timeout_secs.unwrap_or(defaults.ws_timeout_secs),
            market_order_slippage_bps: market_order_slippage_bps
                .unwrap_or(defaults.market_order_slippage_bps),
            rest_quota_per_min,
            sendtx_quota_per_min,
            transport_backend: transport_backend.unwrap_or(defaults.transport_backend),
        }
    }

    #[getter]
    fn proxy_url(&self) -> Option<String> {
        self.proxy_url.clone()
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }
}
