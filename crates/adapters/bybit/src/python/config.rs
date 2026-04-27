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

//! Python bindings for Bybit configuration.

use nautilus_model::identifiers::AccountId;
use pyo3::pymethods;

use crate::{
    common::enums::{BybitEnvironment, BybitMarginMode, BybitProductType},
    config::{BybitDataClientConfig, BybitExecClientConfig},
};

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl BybitDataClientConfig {
    /// Configuration for the Bybit live data client.
    #[new]
    #[pyo3(signature = (
        product_types = None,
        environment = None,
        api_key = None,
        api_secret = None,
        base_url_http = None,
        base_url_ws_public = None,
        base_url_ws_private = None,
        proxy_url = None,
        http_timeout_secs = None,
        max_retries = None,
        retry_delay_initial_ms = None,
        retry_delay_max_ms = None,
        heartbeat_interval_secs = None,
        recv_window_ms = None,
        update_instruments_interval_mins = None,
        instrument_status_poll_secs = None,
    ))]
    #[expect(clippy::too_many_arguments)]
    fn py_new(
        product_types: Option<Vec<BybitProductType>>,
        environment: Option<BybitEnvironment>,
        api_key: Option<String>,
        api_secret: Option<String>,
        base_url_http: Option<String>,
        base_url_ws_public: Option<String>,
        base_url_ws_private: Option<String>,
        proxy_url: Option<String>,
        http_timeout_secs: Option<u64>,
        max_retries: Option<u32>,
        retry_delay_initial_ms: Option<u64>,
        retry_delay_max_ms: Option<u64>,
        heartbeat_interval_secs: Option<u64>,
        recv_window_ms: Option<u64>,
        update_instruments_interval_mins: Option<u64>,
        instrument_status_poll_secs: Option<u64>,
    ) -> Self {
        let defaults = Self::default();
        Self {
            api_key,
            api_secret,
            product_types: product_types.unwrap_or(defaults.product_types),
            environment: environment.unwrap_or(defaults.environment),
            base_url_http,
            base_url_ws_public,
            base_url_ws_private,
            proxy_url,
            http_timeout_secs: http_timeout_secs.unwrap_or(defaults.http_timeout_secs),
            max_retries: max_retries.unwrap_or(defaults.max_retries),
            retry_delay_initial_ms: retry_delay_initial_ms
                .unwrap_or(defaults.retry_delay_initial_ms),
            retry_delay_max_ms: retry_delay_max_ms.unwrap_or(defaults.retry_delay_max_ms),
            heartbeat_interval_secs: heartbeat_interval_secs
                .unwrap_or(defaults.heartbeat_interval_secs),
            recv_window_ms: recv_window_ms.unwrap_or(defaults.recv_window_ms),
            update_instruments_interval_mins: update_instruments_interval_mins
                .or(defaults.update_instruments_interval_mins),
            instrument_status_poll_secs: instrument_status_poll_secs
                .or(defaults.instrument_status_poll_secs),
            transport_backend: defaults.transport_backend,
        }
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl BybitExecClientConfig {
    /// Configuration for the Bybit live execution client.
    #[new]
    #[pyo3(signature = (
        product_types = None,
        environment = None,
        api_key = None,
        api_secret = None,
        base_url_http = None,
        base_url_ws_private = None,
        base_url_ws_trade = None,
        proxy_url = None,
        http_timeout_secs = None,
        max_retries = None,
        retry_delay_initial_ms = None,
        retry_delay_max_ms = None,
        heartbeat_interval_secs = None,
        recv_window_ms = None,
        account_id = None,
        use_spot_position_reports = None,
        margin_mode = None,
    ))]
    #[expect(clippy::too_many_arguments)]
    fn py_new(
        product_types: Option<Vec<BybitProductType>>,
        environment: Option<BybitEnvironment>,
        api_key: Option<String>,
        api_secret: Option<String>,
        base_url_http: Option<String>,
        base_url_ws_private: Option<String>,
        base_url_ws_trade: Option<String>,
        proxy_url: Option<String>,
        http_timeout_secs: Option<u64>,
        max_retries: Option<u32>,
        retry_delay_initial_ms: Option<u64>,
        retry_delay_max_ms: Option<u64>,
        heartbeat_interval_secs: Option<u64>,
        recv_window_ms: Option<u64>,
        account_id: Option<AccountId>,
        use_spot_position_reports: Option<bool>,
        margin_mode: Option<BybitMarginMode>,
    ) -> Self {
        let defaults = Self::default();
        Self {
            api_key,
            api_secret,
            product_types: product_types.unwrap_or(defaults.product_types),
            environment: environment.unwrap_or(defaults.environment),
            base_url_http,
            base_url_ws_private,
            base_url_ws_trade,
            proxy_url,
            http_timeout_secs: http_timeout_secs.unwrap_or(defaults.http_timeout_secs),
            max_retries: max_retries.unwrap_or(defaults.max_retries),
            retry_delay_initial_ms: retry_delay_initial_ms
                .unwrap_or(defaults.retry_delay_initial_ms),
            retry_delay_max_ms: retry_delay_max_ms.unwrap_or(defaults.retry_delay_max_ms),
            heartbeat_interval_secs: heartbeat_interval_secs
                .unwrap_or(defaults.heartbeat_interval_secs),
            recv_window_ms: recv_window_ms.unwrap_or(defaults.recv_window_ms),
            account_id,
            use_spot_position_reports: use_spot_position_reports
                .unwrap_or(defaults.use_spot_position_reports),
            futures_leverages: None,
            position_mode: None,
            margin_mode,
            transport_backend: defaults.transport_backend,
        }
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }
}
