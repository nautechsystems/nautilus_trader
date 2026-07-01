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

//! Python bindings for Derive configuration.

use nautilus_network::websocket::TransportBackend;
use pyo3::pymethods;
use rust_decimal::Decimal;

use crate::{
    common::enums::DeriveEnvironment,
    config::{DeriveDataClientConfig, DeriveExecClientConfig},
};

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl DeriveDataClientConfig {
    /// Configuration for the Derive live data client.
    #[new]
    #[pyo3(signature = (
        base_url_rest = None,
        base_url_ws = None,
        proxy_url = None,
        environment = None,
        http_timeout_secs = None,
        ws_timeout_secs = None,
        update_instruments_interval_mins = None,
        currencies = None,
        include_expired = None,
        auto_load_missing_instruments = None,
        transport_backend = None,
    ))]
    #[expect(clippy::too_many_arguments)]
    fn py_new(
        base_url_rest: Option<String>,
        base_url_ws: Option<String>,
        proxy_url: Option<String>,
        environment: Option<DeriveEnvironment>,
        http_timeout_secs: Option<u64>,
        ws_timeout_secs: Option<u64>,
        update_instruments_interval_mins: Option<u64>,
        currencies: Option<Vec<String>>,
        include_expired: Option<bool>,
        auto_load_missing_instruments: Option<bool>,
        transport_backend: Option<TransportBackend>,
    ) -> Self {
        let defaults = Self::default();
        Self {
            base_url_rest,
            base_url_ws,
            proxy_url,
            environment: environment.unwrap_or(defaults.environment),
            http_timeout_secs: http_timeout_secs.unwrap_or(defaults.http_timeout_secs),
            ws_timeout_secs: ws_timeout_secs.unwrap_or(defaults.ws_timeout_secs),
            update_instruments_interval_mins: update_instruments_interval_mins
                .unwrap_or(defaults.update_instruments_interval_mins),
            currencies: currencies.unwrap_or(defaults.currencies),
            include_expired: include_expired.unwrap_or(defaults.include_expired),
            auto_load_missing_instruments: auto_load_missing_instruments
                .unwrap_or(defaults.auto_load_missing_instruments),
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
impl DeriveExecClientConfig {
    /// Configuration for the Derive live execution client.
    #[new]
    #[pyo3(signature = (
        wallet_address = None,
        session_key = None,
        subaccount_id = None,
        base_url_rest = None,
        base_url_ws = None,
        proxy_url = None,
        environment = None,
        http_timeout_secs = None,
        max_retries = None,
        retry_delay_initial_ms = None,
        retry_delay_max_ms = None,
        max_fee_per_contract = None,
        domain_separator = None,
        action_typehash = None,
        trade_module_address = None,
        signature_expiry_secs = None,
        market_order_slippage_bps = None,
        max_matching_requests_per_second = None,
        transport_backend = None,
    ))]
    #[expect(clippy::too_many_arguments)]
    fn py_new(
        wallet_address: Option<String>,
        session_key: Option<String>,
        subaccount_id: Option<u64>,
        base_url_rest: Option<String>,
        base_url_ws: Option<String>,
        proxy_url: Option<String>,
        environment: Option<DeriveEnvironment>,
        http_timeout_secs: Option<u64>,
        max_retries: Option<u32>,
        retry_delay_initial_ms: Option<u64>,
        retry_delay_max_ms: Option<u64>,
        max_fee_per_contract: Option<Decimal>,
        domain_separator: Option<String>,
        action_typehash: Option<String>,
        trade_module_address: Option<String>,
        signature_expiry_secs: Option<u64>,
        market_order_slippage_bps: Option<u32>,
        max_matching_requests_per_second: Option<u32>,
        transport_backend: Option<TransportBackend>,
    ) -> Self {
        let defaults = Self::default();
        Self {
            wallet_address,
            session_key,
            subaccount_id,
            base_url_rest,
            base_url_ws,
            proxy_url,
            environment: environment.unwrap_or(defaults.environment),
            http_timeout_secs: http_timeout_secs.unwrap_or(defaults.http_timeout_secs),
            max_retries: max_retries.unwrap_or(defaults.max_retries),
            retry_delay_initial_ms: retry_delay_initial_ms
                .unwrap_or(defaults.retry_delay_initial_ms),
            retry_delay_max_ms: retry_delay_max_ms.unwrap_or(defaults.retry_delay_max_ms),
            max_fee_per_contract,
            transport_backend: transport_backend.unwrap_or(defaults.transport_backend),
            domain_separator,
            action_typehash,
            trade_module_address,
            signature_expiry_secs: signature_expiry_secs.unwrap_or(defaults.signature_expiry_secs),
            market_order_slippage_bps: market_order_slippage_bps
                .unwrap_or(defaults.market_order_slippage_bps),
            max_matching_requests_per_second,
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
