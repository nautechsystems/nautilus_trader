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

use crate::config::{AxDataClientConfig, AxExecClientConfig};

#[pymethods]
impl AxDataClientConfig {
    #[new]
    #[allow(clippy::too_many_arguments)]
    #[pyo3(signature = (api_key=None, api_secret=None, is_sandbox=None, base_url_http=None, base_url_ws_public=None, base_url_ws_private=None, http_proxy_url=None, ws_proxy_url=None, http_timeout_secs=None, max_retries=None, retry_delay_initial_ms=None, retry_delay_max_ms=None, heartbeat_interval_secs=None, recv_window_ms=None, update_instruments_interval_mins=None, funding_rate_poll_interval_mins=None))]
    fn py_new(
        api_key: Option<String>,
        api_secret: Option<String>,
        is_sandbox: Option<bool>,
        base_url_http: Option<String>,
        base_url_ws_public: Option<String>,
        base_url_ws_private: Option<String>,
        http_proxy_url: Option<String>,
        ws_proxy_url: Option<String>,
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
            is_sandbox: is_sandbox.unwrap_or(default.is_sandbox),
            base_url_http,
            base_url_ws_public,
            base_url_ws_private,
            http_proxy_url,
            ws_proxy_url,
            http_timeout_secs: http_timeout_secs.or(default.http_timeout_secs),
            max_retries: max_retries.or(default.max_retries),
            retry_delay_initial_ms: retry_delay_initial_ms.or(default.retry_delay_initial_ms),
            retry_delay_max_ms: retry_delay_max_ms.or(default.retry_delay_max_ms),
            heartbeat_interval_secs: heartbeat_interval_secs.or(default.heartbeat_interval_secs),
            recv_window_ms: recv_window_ms.or(default.recv_window_ms),
            update_instruments_interval_mins: update_instruments_interval_mins
                .or(default.update_instruments_interval_mins),
            funding_rate_poll_interval_mins: funding_rate_poll_interval_mins
                .or(default.funding_rate_poll_interval_mins),
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
impl AxExecClientConfig {
    #[new]
    #[allow(clippy::too_many_arguments)]
    #[pyo3(signature = (trader_id=None, account_id=None, api_key=None, api_secret=None, is_sandbox=None, base_url_http=None, base_url_orders=None, base_url_ws_private=None, http_proxy_url=None, ws_proxy_url=None, http_timeout_secs=None, max_retries=None, retry_delay_initial_ms=None, retry_delay_max_ms=None, heartbeat_interval_secs=None, recv_window_ms=None))]
    fn py_new(
        trader_id: Option<TraderId>,
        account_id: Option<AccountId>,
        api_key: Option<String>,
        api_secret: Option<String>,
        is_sandbox: Option<bool>,
        base_url_http: Option<String>,
        base_url_orders: Option<String>,
        base_url_ws_private: Option<String>,
        http_proxy_url: Option<String>,
        ws_proxy_url: Option<String>,
        http_timeout_secs: Option<u64>,
        max_retries: Option<u32>,
        retry_delay_initial_ms: Option<u64>,
        retry_delay_max_ms: Option<u64>,
        heartbeat_interval_secs: Option<u64>,
        recv_window_ms: Option<u64>,
    ) -> Self {
        let default = Self::default();
        Self {
            trader_id: trader_id.unwrap_or(default.trader_id),
            account_id: account_id.unwrap_or(default.account_id),
            api_key,
            api_secret,
            is_sandbox: is_sandbox.unwrap_or(default.is_sandbox),
            base_url_http,
            base_url_orders,
            base_url_ws_private,
            http_proxy_url,
            ws_proxy_url,
            http_timeout_secs: http_timeout_secs.or(default.http_timeout_secs),
            max_retries: max_retries.or(default.max_retries),
            retry_delay_initial_ms: retry_delay_initial_ms.or(default.retry_delay_initial_ms),
            retry_delay_max_ms: retry_delay_max_ms.or(default.retry_delay_max_ms),
            heartbeat_interval_secs: heartbeat_interval_secs.or(default.heartbeat_interval_secs),
            recv_window_ms: recv_window_ms.or(default.recv_window_ms),
        }
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }

    fn __str__(&self) -> String {
        format!("{self:?}")
    }
}
