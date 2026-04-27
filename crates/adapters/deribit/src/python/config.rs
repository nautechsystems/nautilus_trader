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

//! Python bindings for Deribit configuration.

use nautilus_model::identifiers::{AccountId, TraderId};
use pyo3::prelude::*;

use crate::{
    common::enums::DeribitEnvironment,
    config::{DeribitDataClientConfig, DeribitExecClientConfig},
    http::models::DeribitProductType,
};

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl DeribitDataClientConfig {
    /// Configuration for the Deribit data client.
    #[new]
    #[pyo3(signature = (
        product_types = None,
        environment = None,
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
        update_instruments_interval_mins = None,
    ))]
    #[expect(clippy::too_many_arguments)]
    fn py_new(
        product_types: Option<Vec<DeribitProductType>>,
        environment: Option<DeribitEnvironment>,
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
        update_instruments_interval_mins: Option<u64>,
    ) -> Self {
        let defaults = Self::default();
        Self {
            api_key,
            api_secret,
            product_types: product_types.unwrap_or(defaults.product_types),
            base_url_http,
            base_url_ws,
            proxy_url,
            environment: environment.unwrap_or(defaults.environment),
            http_timeout_secs: http_timeout_secs.unwrap_or(defaults.http_timeout_secs),
            max_retries: max_retries.unwrap_or(defaults.max_retries),
            retry_delay_initial_ms: retry_delay_initial_ms
                .unwrap_or(defaults.retry_delay_initial_ms),
            retry_delay_max_ms: retry_delay_max_ms.unwrap_or(defaults.retry_delay_max_ms),
            heartbeat_interval_secs: heartbeat_interval_secs
                .unwrap_or(defaults.heartbeat_interval_secs),
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
impl DeribitExecClientConfig {
    /// Configuration for the Deribit execution client.
    #[new]
    #[pyo3(signature = (
        trader_id,
        account_id,
        product_types = None,
        environment = None,
        api_key = None,
        api_secret = None,
        base_url_http = None,
        base_url_ws = None,
        proxy_url = None,
        http_timeout_secs = None,
        max_retries = None,
        retry_delay_initial_ms = None,
        retry_delay_max_ms = None,
    ))]
    #[expect(clippy::too_many_arguments)]
    fn py_new(
        trader_id: TraderId,
        account_id: AccountId,
        product_types: Option<Vec<DeribitProductType>>,
        environment: Option<DeribitEnvironment>,
        api_key: Option<String>,
        api_secret: Option<String>,
        base_url_http: Option<String>,
        base_url_ws: Option<String>,
        proxy_url: Option<String>,
        http_timeout_secs: Option<u64>,
        max_retries: Option<u32>,
        retry_delay_initial_ms: Option<u64>,
        retry_delay_max_ms: Option<u64>,
    ) -> Self {
        let defaults = Self::default();
        Self {
            trader_id,
            account_id,
            api_key,
            api_secret,
            product_types: product_types.unwrap_or(defaults.product_types),
            base_url_http,
            base_url_ws,
            proxy_url,
            environment: environment.unwrap_or(defaults.environment),
            http_timeout_secs: http_timeout_secs.unwrap_or(defaults.http_timeout_secs),
            max_retries: max_retries.unwrap_or(defaults.max_retries),
            retry_delay_initial_ms: retry_delay_initial_ms
                .unwrap_or(defaults.retry_delay_initial_ms),
            retry_delay_max_ms: retry_delay_max_ms.unwrap_or(defaults.retry_delay_max_ms),
            transport_backend: defaults.transport_backend,
        }
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }
}
