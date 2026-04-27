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

use nautilus_model::identifiers::{AccountId, TraderId};
use pyo3::pymethods;

use crate::{
    common::enums::AxEnvironment,
    config::{AxDataClientConfig, AxExecClientConfig},
};

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl AxDataClientConfig {
    /// Configuration for the AX Exchange live data client.
    #[new]
    #[expect(clippy::too_many_arguments)]
    #[pyo3(signature = (api_key=None, api_secret=None, environment=None, base_url_http=None, base_url_ws_public=None, base_url_ws_private=None, proxy_url=None, http_timeout_secs=None, max_retries=None, retry_delay_initial_ms=None, retry_delay_max_ms=None, heartbeat_interval_secs=None, recv_window_ms=None, update_instruments_interval_mins=None, funding_rate_poll_interval_mins=None))]
    fn py_new(
        api_key: Option<String>,
        api_secret: Option<String>,
        environment: Option<AxEnvironment>,
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
        funding_rate_poll_interval_mins: Option<u64>,
    ) -> Self {
        let default = Self::default();
        Self {
            api_key,
            api_secret,
            environment: environment.unwrap_or(default.environment),
            base_url_http,
            base_url_ws_public,
            base_url_ws_private,
            proxy_url,
            http_timeout_secs: http_timeout_secs.unwrap_or(default.http_timeout_secs),
            max_retries: max_retries.unwrap_or(default.max_retries),
            retry_delay_initial_ms: retry_delay_initial_ms
                .unwrap_or(default.retry_delay_initial_ms),
            retry_delay_max_ms: retry_delay_max_ms.unwrap_or(default.retry_delay_max_ms),
            heartbeat_interval_secs: heartbeat_interval_secs
                .unwrap_or(default.heartbeat_interval_secs),
            recv_window_ms: recv_window_ms.unwrap_or(default.recv_window_ms),
            update_instruments_interval_mins: update_instruments_interval_mins
                .unwrap_or(default.update_instruments_interval_mins),
            funding_rate_poll_interval_mins: funding_rate_poll_interval_mins
                .unwrap_or(default.funding_rate_poll_interval_mins),
            transport_backend: default.transport_backend,
        }
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }

    fn __str__(&self) -> String {
        format!("{self:?}")
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl AxExecClientConfig {
    /// Configuration for the AX Exchange live execution client.
    #[new]
    #[expect(clippy::too_many_arguments)]
    #[pyo3(signature = (trader_id=None, account_id=None, api_key=None, api_secret=None, environment=None, base_url_http=None, base_url_orders=None, base_url_ws_private=None, proxy_url=None, http_timeout_secs=None, max_retries=None, retry_delay_initial_ms=None, retry_delay_max_ms=None, heartbeat_interval_secs=None, recv_window_ms=None, cancel_on_disconnect=None))]
    fn py_new(
        trader_id: Option<TraderId>,
        account_id: Option<AccountId>,
        api_key: Option<String>,
        api_secret: Option<String>,
        environment: Option<AxEnvironment>,
        base_url_http: Option<String>,
        base_url_orders: Option<String>,
        base_url_ws_private: Option<String>,
        proxy_url: Option<String>,
        http_timeout_secs: Option<u64>,
        max_retries: Option<u32>,
        retry_delay_initial_ms: Option<u64>,
        retry_delay_max_ms: Option<u64>,
        heartbeat_interval_secs: Option<u64>,
        recv_window_ms: Option<u64>,
        cancel_on_disconnect: Option<bool>,
    ) -> Self {
        let default = Self::default();
        Self {
            trader_id: trader_id.unwrap_or(default.trader_id),
            account_id: account_id.unwrap_or(default.account_id),
            api_key,
            api_secret,
            environment: environment.unwrap_or(default.environment),
            base_url_http,
            base_url_orders,
            base_url_ws_private,
            proxy_url,
            http_timeout_secs: http_timeout_secs.unwrap_or(default.http_timeout_secs),
            max_retries: max_retries.unwrap_or(default.max_retries),
            retry_delay_initial_ms: retry_delay_initial_ms
                .unwrap_or(default.retry_delay_initial_ms),
            retry_delay_max_ms: retry_delay_max_ms.unwrap_or(default.retry_delay_max_ms),
            heartbeat_interval_secs: heartbeat_interval_secs
                .unwrap_or(default.heartbeat_interval_secs),
            recv_window_ms: recv_window_ms.unwrap_or(default.recv_window_ms),
            cancel_on_disconnect: cancel_on_disconnect.unwrap_or(default.cancel_on_disconnect),
            transport_backend: default.transport_backend,
        }
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }

    fn __str__(&self) -> String {
        format!("{self:?}")
    }
}
