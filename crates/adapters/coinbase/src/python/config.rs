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

use pyo3::pymethods;

use crate::{
    common::enums::CoinbaseEnvironment,
    config::{CoinbaseDataClientConfig, CoinbaseExecClientConfig},
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
        http_proxy_url = None,
        ws_proxy_url = None,
        environment = None,
        http_timeout_secs = None,
        ws_timeout_secs = None,
        update_instruments_interval_mins = None,
    ))]
    #[expect(clippy::too_many_arguments)]
    fn py_new(
        api_key: Option<String>,
        api_secret: Option<String>,
        base_url_rest: Option<String>,
        base_url_ws: Option<String>,
        http_proxy_url: Option<String>,
        ws_proxy_url: Option<String>,
        environment: Option<CoinbaseEnvironment>,
        http_timeout_secs: Option<u64>,
        ws_timeout_secs: Option<u64>,
        update_instruments_interval_mins: Option<u64>,
    ) -> Self {
        let defaults = Self::default();
        Self {
            api_key,
            api_secret,
            base_url_rest,
            base_url_ws,
            http_proxy_url,
            ws_proxy_url,
            environment: environment.unwrap_or(defaults.environment),
            http_timeout_secs: http_timeout_secs.unwrap_or(defaults.http_timeout_secs),
            ws_timeout_secs: ws_timeout_secs.unwrap_or(defaults.ws_timeout_secs),
            update_instruments_interval_mins: update_instruments_interval_mins
                .unwrap_or(defaults.update_instruments_interval_mins),
        }
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl CoinbaseExecClientConfig {
    /// Configuration for the Coinbase live execution client.
    #[new]
    #[pyo3(signature = (
        api_key = None,
        api_secret = None,
        base_url_rest = None,
        base_url_ws = None,
        http_proxy_url = None,
        ws_proxy_url = None,
        environment = None,
        http_timeout_secs = None,
        max_retries = None,
        retry_delay_initial_ms = None,
        retry_delay_max_ms = None,
    ))]
    #[expect(clippy::too_many_arguments)]
    fn py_new(
        api_key: Option<String>,
        api_secret: Option<String>,
        base_url_rest: Option<String>,
        base_url_ws: Option<String>,
        http_proxy_url: Option<String>,
        ws_proxy_url: Option<String>,
        environment: Option<CoinbaseEnvironment>,
        http_timeout_secs: Option<u64>,
        max_retries: Option<u32>,
        retry_delay_initial_ms: Option<u64>,
        retry_delay_max_ms: Option<u64>,
    ) -> Self {
        let defaults = Self::default();
        Self {
            api_key,
            api_secret,
            base_url_rest,
            base_url_ws,
            http_proxy_url,
            ws_proxy_url,
            environment: environment.unwrap_or(defaults.environment),
            http_timeout_secs: http_timeout_secs.unwrap_or(defaults.http_timeout_secs),
            max_retries: max_retries.unwrap_or(defaults.max_retries),
            retry_delay_initial_ms: retry_delay_initial_ms
                .unwrap_or(defaults.retry_delay_initial_ms),
            retry_delay_max_ms: retry_delay_max_ms.unwrap_or(defaults.retry_delay_max_ms),
        }
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }
}
