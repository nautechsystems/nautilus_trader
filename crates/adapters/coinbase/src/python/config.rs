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

//! Python bindings for Coinbase configuration.

use nautilus_core::string::secret::SecretString;
use nautilus_model::{enums::AccountType, identifiers::AccountId};
use nautilus_network::websocket::TransportBackend;
use pyo3::pymethods;
use rust_decimal::Decimal;

use crate::{
    common::enums::{CoinbaseEnvironment, CoinbaseMarginType},
    config::{CoinbaseDataClientConfig, CoinbaseExecutionClientConfig},
};

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl CoinbaseDataClientConfig {
    /// Configuration for the Coinbase live data client.
    #[new]
    #[pyo3(signature = (
        api_key = None,
        api_secret = None,
        base_url_rest = None,
        base_url_ws = None,
        proxy_url = None,
        environment = None,
        http_timeout_secs = None,
        ws_timeout_secs = None,
        update_instruments_interval_mins = None,
        derivatives_poll_interval_secs = None,
        transport_backend = None,
    ))]
    #[expect(clippy::too_many_arguments)]
    fn py_new(
        api_key: Option<String>,
        api_secret: Option<String>,
        base_url_rest: Option<String>,
        base_url_ws: Option<String>,
        proxy_url: Option<String>,
        environment: Option<CoinbaseEnvironment>,
        http_timeout_secs: Option<u64>,
        ws_timeout_secs: Option<u64>,
        update_instruments_interval_mins: Option<u64>,
        derivatives_poll_interval_secs: Option<u64>,
        transport_backend: Option<TransportBackend>,
    ) -> Self {
        let defaults = Self::default();
        Self {
            api_key: api_key.map(SecretString::from),
            api_secret: api_secret.map(SecretString::from),
            base_url_rest,
            base_url_ws,
            proxy_url: proxy_url.map(SecretString::from),
            environment: environment.unwrap_or(defaults.environment),
            http_timeout_secs: http_timeout_secs.unwrap_or(defaults.http_timeout_secs),
            ws_timeout_secs: ws_timeout_secs.unwrap_or(defaults.ws_timeout_secs),
            update_instruments_interval_mins: update_instruments_interval_mins
                .unwrap_or(defaults.update_instruments_interval_mins),
            derivatives_poll_interval_secs: derivatives_poll_interval_secs
                .unwrap_or(defaults.derivatives_poll_interval_secs),
            transport_backend: transport_backend.unwrap_or(defaults.transport_backend),
        }
    }

    #[getter]
    const fn has_proxy_url(&self) -> bool {
        self.proxy_url.is_some()
    }

    fn __repr__(&self) -> String {
        stringify!(CoinbaseDataClientConfig).to_string()
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl CoinbaseExecutionClientConfig {
    /// Configuration for the Coinbase live execution client.
    #[new]
    #[pyo3(signature = (
        account_id = None,
        api_key = None,
        api_secret = None,
        base_url_rest = None,
        base_url_ws = None,
        proxy_url = None,
        environment = None,
        http_timeout_secs = None,
        max_retries = None,
        retry_delay_initial_ms = None,
        retry_delay_max_ms = None,
        account_type = None,
        default_margin_type = None,
        default_leverage = None,
        retail_portfolio_id = None,
        transport_backend = None,
    ))]
    #[expect(clippy::too_many_arguments)]
    fn py_new(
        account_id: Option<AccountId>,
        api_key: Option<String>,
        api_secret: Option<String>,
        base_url_rest: Option<String>,
        base_url_ws: Option<String>,
        proxy_url: Option<String>,
        environment: Option<CoinbaseEnvironment>,
        http_timeout_secs: Option<u64>,
        max_retries: Option<u32>,
        retry_delay_initial_ms: Option<u64>,
        retry_delay_max_ms: Option<u64>,
        account_type: Option<AccountType>,
        default_margin_type: Option<CoinbaseMarginType>,
        default_leverage: Option<Decimal>,
        retail_portfolio_id: Option<String>,
        transport_backend: Option<TransportBackend>,
    ) -> Self {
        let defaults = Self::default();
        Self {
            account_id: account_id.unwrap_or(defaults.account_id),
            api_key: api_key.map(SecretString::from),
            api_secret: api_secret.map(SecretString::from),
            base_url_rest,
            base_url_ws,
            proxy_url: proxy_url.map(SecretString::from),
            environment: environment.unwrap_or(defaults.environment),
            http_timeout_secs: http_timeout_secs.unwrap_or(defaults.http_timeout_secs),
            max_retries: max_retries.unwrap_or(defaults.max_retries),
            retry_delay_initial_ms: retry_delay_initial_ms
                .unwrap_or(defaults.retry_delay_initial_ms),
            retry_delay_max_ms: retry_delay_max_ms.unwrap_or(defaults.retry_delay_max_ms),
            account_type: account_type.unwrap_or(defaults.account_type),
            default_margin_type,
            default_leverage,
            retail_portfolio_id,
            transport_backend: transport_backend.unwrap_or(defaults.transport_backend),
        }
    }

    #[getter]
    const fn has_proxy_url(&self) -> bool {
        self.proxy_url.is_some()
    }

    fn __repr__(&self) -> String {
        stringify!(CoinbaseExecutionClientConfig).to_string()
    }
}
