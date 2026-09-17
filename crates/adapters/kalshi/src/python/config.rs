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

//! Python bindings for Kalshi configuration.

use nautilus_core::string::secret::SecretString;
use nautilus_model::identifiers::AccountId;
use pyo3::pymethods;

use crate::{
    common::enums::{KalshiEnvironment, KalshiSelfTradePrevention},
    config::{KalshiDataClientConfig, KalshiExecClientConfig},
};

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl KalshiDataClientConfig {
    /// Configuration for the Kalshi live data client.
    #[new]
    #[pyo3(signature = (
        environment = None,
        base_url = None,
        api_key_id = None,
        api_key_pem = None,
        http_timeout_secs = None,
        proxy_url = None,
        event_tickers = None,
        series_ticker = None,
        poll_interval_millis = None,
    ))]
    #[expect(clippy::too_many_arguments)]
    fn py_new(
        environment: Option<KalshiEnvironment>,
        base_url: Option<String>,
        api_key_id: Option<String>,
        api_key_pem: Option<String>,
        http_timeout_secs: Option<u64>,
        proxy_url: Option<String>,
        event_tickers: Option<Vec<String>>,
        series_ticker: Option<String>,
        poll_interval_millis: Option<u64>,
    ) -> Self {
        let defaults = Self::default();
        Self {
            environment: environment.unwrap_or(defaults.environment),
            base_url,
            api_key_id,
            api_key_pem: api_key_pem.map(SecretString::from),
            http_timeout_secs,
            proxy_url: proxy_url.map(SecretString::from),
            event_tickers: event_tickers.unwrap_or(defaults.event_tickers),
            series_ticker,
            poll_interval_millis,
        }
    }

    #[getter]
    const fn has_proxy_url(&self) -> bool {
        self.proxy_url.is_some()
    }

    fn __repr__(&self) -> String {
        stringify!(KalshiDataClientConfig).to_string()
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl KalshiExecClientConfig {
    /// Configuration for the Kalshi live execution client.
    #[new]
    #[pyo3(signature = (
        environment = None,
        base_url = None,
        api_key_id = None,
        api_key_pem = None,
        http_timeout_secs = None,
        proxy_url = None,
        reconciliation = None,
        account_id = None,
        self_trade_prevention = None,
        cancel_order_on_pause = None,
        poll_interval_millis = None,
    ))]
    #[expect(clippy::too_many_arguments)]
    fn py_new(
        environment: Option<KalshiEnvironment>,
        base_url: Option<String>,
        api_key_id: Option<String>,
        api_key_pem: Option<String>,
        http_timeout_secs: Option<u64>,
        proxy_url: Option<String>,
        reconciliation: Option<bool>,
        account_id: Option<AccountId>,
        self_trade_prevention: Option<KalshiSelfTradePrevention>,
        cancel_order_on_pause: Option<bool>,
        poll_interval_millis: Option<u64>,
    ) -> Self {
        let defaults = Self::default();
        Self {
            environment: environment.unwrap_or(defaults.environment),
            base_url,
            api_key_id,
            api_key_pem: api_key_pem.map(SecretString::from),
            http_timeout_secs,
            proxy_url: proxy_url.map(SecretString::from),
            reconciliation: reconciliation.unwrap_or(defaults.reconciliation),
            account_id: account_id.unwrap_or(defaults.account_id),
            self_trade_prevention: self_trade_prevention.unwrap_or(defaults.self_trade_prevention),
            cancel_order_on_pause,
            poll_interval_millis,
        }
    }

    #[getter]
    const fn has_proxy_url(&self) -> bool {
        self.proxy_url.is_some()
    }

    fn __repr__(&self) -> String {
        "KalshiExecutionClientConfig".to_string()
    }
}
