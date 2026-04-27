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

//! Python bindings for BitMEX configuration.

use nautilus_model::identifiers::AccountId;
use pyo3::prelude::*;

use crate::{
    common::enums::BitmexEnvironment,
    config::{BitmexDataClientConfig, BitmexExecClientConfig},
};

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl BitmexDataClientConfig {
    /// Configuration for the BitMEX live data client.
    #[new]
    #[pyo3(signature = (
        api_key = None,
        api_secret = None,
        base_url_http = None,
        base_url_ws = None,
        proxy_url = None,
        http_timeout_secs = None,
        max_retries = None,
        retry_delay_initial_ms = None,
        retry_delay_max_ms = None,
        heartbeat_interval_secs = None,
        recv_window_ms = None,
        active_only = None,
        update_instruments_interval_mins = None,
        environment = None,
        max_requests_per_second = None,
        max_requests_per_minute = None,
    ))]
    #[expect(clippy::too_many_arguments)]
    fn py_new(
        api_key: Option<String>,
        api_secret: Option<String>,
        base_url_http: Option<String>,
        base_url_ws: Option<String>,
        proxy_url: Option<String>,
        http_timeout_secs: Option<u64>,
        max_retries: Option<u32>,
        retry_delay_initial_ms: Option<u64>,
        retry_delay_max_ms: Option<u64>,
        heartbeat_interval_secs: Option<u64>,
        recv_window_ms: Option<u64>,
        active_only: Option<bool>,
        update_instruments_interval_mins: Option<u64>,
        environment: Option<BitmexEnvironment>,
        max_requests_per_second: Option<u32>,
        max_requests_per_minute: Option<u32>,
    ) -> Self {
        let defaults = Self::default();
        Self {
            api_key,
            api_secret,
            base_url_http,
            base_url_ws,
            proxy_url,
            http_timeout_secs: http_timeout_secs.unwrap_or(defaults.http_timeout_secs),
            max_retries: max_retries.unwrap_or(defaults.max_retries),
            retry_delay_initial_ms: retry_delay_initial_ms
                .unwrap_or(defaults.retry_delay_initial_ms),
            retry_delay_max_ms: retry_delay_max_ms.unwrap_or(defaults.retry_delay_max_ms),
            heartbeat_interval_secs,
            recv_window_ms: recv_window_ms.unwrap_or(defaults.recv_window_ms),
            active_only: active_only.unwrap_or(defaults.active_only),
            update_instruments_interval_mins,
            environment: environment.unwrap_or(defaults.environment),
            max_requests_per_second: max_requests_per_second
                .unwrap_or(defaults.max_requests_per_second),
            max_requests_per_minute: max_requests_per_minute
                .unwrap_or(defaults.max_requests_per_minute),
            transport_backend: defaults.transport_backend,
        }
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl BitmexExecClientConfig {
    /// Configuration for the BitMEX live execution client.
    #[new]
    #[pyo3(signature = (
        api_key = None,
        api_secret = None,
        base_url_http = None,
        base_url_ws = None,
        proxy_url = None,
        http_timeout_secs = None,
        max_retries = None,
        retry_delay_initial_ms = None,
        retry_delay_max_ms = None,
        heartbeat_interval_secs = None,
        recv_window_ms = None,
        active_only = None,
        environment = None,
        account_id = None,
        max_requests_per_second = None,
        max_requests_per_minute = None,
        submitter_pool_size = None,
        canceller_pool_size = None,
        submitter_proxy_urls = None,
        canceller_proxy_urls = None,
        deadmans_switch_timeout_secs = None,
    ))]
    #[expect(clippy::too_many_arguments)]
    fn py_new(
        api_key: Option<String>,
        api_secret: Option<String>,
        base_url_http: Option<String>,
        base_url_ws: Option<String>,
        proxy_url: Option<String>,
        http_timeout_secs: Option<u64>,
        max_retries: Option<u32>,
        retry_delay_initial_ms: Option<u64>,
        retry_delay_max_ms: Option<u64>,
        heartbeat_interval_secs: Option<u64>,
        recv_window_ms: Option<u64>,
        active_only: Option<bool>,
        environment: Option<BitmexEnvironment>,
        account_id: Option<AccountId>,
        max_requests_per_second: Option<u32>,
        max_requests_per_minute: Option<u32>,
        submitter_pool_size: Option<usize>,
        canceller_pool_size: Option<usize>,
        submitter_proxy_urls: Option<Vec<String>>,
        canceller_proxy_urls: Option<Vec<String>>,
        deadmans_switch_timeout_secs: Option<u64>,
    ) -> Self {
        let defaults = Self::default();
        Self {
            api_key,
            api_secret,
            base_url_http,
            base_url_ws,
            proxy_url,
            http_timeout_secs: http_timeout_secs.unwrap_or(defaults.http_timeout_secs),
            max_retries: max_retries.unwrap_or(defaults.max_retries),
            retry_delay_initial_ms: retry_delay_initial_ms
                .unwrap_or(defaults.retry_delay_initial_ms),
            retry_delay_max_ms: retry_delay_max_ms.unwrap_or(defaults.retry_delay_max_ms),
            heartbeat_interval_secs: heartbeat_interval_secs
                .unwrap_or(defaults.heartbeat_interval_secs),
            recv_window_ms: recv_window_ms.unwrap_or(defaults.recv_window_ms),
            active_only: active_only.unwrap_or(defaults.active_only),
            environment: environment.unwrap_or(defaults.environment),
            account_id,
            max_requests_per_second: max_requests_per_second
                .unwrap_or(defaults.max_requests_per_second),
            max_requests_per_minute: max_requests_per_minute
                .unwrap_or(defaults.max_requests_per_minute),
            submitter_pool_size,
            canceller_pool_size,
            submitter_proxy_urls,
            canceller_proxy_urls,
            deadmans_switch_timeout_secs,
            transport_backend: defaults.transport_backend,
        }
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }
}
