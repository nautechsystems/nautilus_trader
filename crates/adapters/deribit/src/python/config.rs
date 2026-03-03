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
    config::{DeribitDataClientConfig, DeribitExecClientConfig},
    http::models::DeribitProductType,
};

#[pymethods]
impl DeribitDataClientConfig {
    #[new]
    #[pyo3(signature = (
        product_types = None,
        use_testnet = None,
        api_key = None,
        api_secret = None,
        base_url_http = None,
        base_url_ws = None,
        http_timeout_secs = None,
        max_retries = None,
        retry_delay_initial_ms = None,
        retry_delay_max_ms = None,
        heartbeat_interval_secs = None,
        update_instruments_interval_mins = None,
    ))]
    #[allow(clippy::too_many_arguments)]
    fn py_new(
        product_types: Option<Vec<DeribitProductType>>,
        use_testnet: Option<bool>,
        api_key: Option<String>,
        api_secret: Option<String>,
        base_url_http: Option<String>,
        base_url_ws: Option<String>,
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
            use_testnet: use_testnet.unwrap_or(defaults.use_testnet),
            http_timeout_secs: http_timeout_secs.or(defaults.http_timeout_secs),
            max_retries: max_retries.or(defaults.max_retries),
            retry_delay_initial_ms: retry_delay_initial_ms.or(defaults.retry_delay_initial_ms),
            retry_delay_max_ms: retry_delay_max_ms.or(defaults.retry_delay_max_ms),
            heartbeat_interval_secs: heartbeat_interval_secs.or(defaults.heartbeat_interval_secs),
            update_instruments_interval_mins: update_instruments_interval_mins
                .or(defaults.update_instruments_interval_mins),
        }
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }
}

#[pymethods]
impl DeribitExecClientConfig {
    #[new]
    #[pyo3(signature = (
        trader_id,
        account_id,
        product_types = None,
        use_testnet = None,
        api_key = None,
        api_secret = None,
        base_url_http = None,
        base_url_ws = None,
        http_timeout_secs = None,
        max_retries = None,
        retry_delay_initial_ms = None,
        retry_delay_max_ms = None,
    ))]
    #[allow(clippy::too_many_arguments)]
    fn py_new(
        trader_id: TraderId,
        account_id: AccountId,
        product_types: Option<Vec<DeribitProductType>>,
        use_testnet: Option<bool>,
        api_key: Option<String>,
        api_secret: Option<String>,
        base_url_http: Option<String>,
        base_url_ws: Option<String>,
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
            use_testnet: use_testnet.unwrap_or(defaults.use_testnet),
            http_timeout_secs: http_timeout_secs.or(defaults.http_timeout_secs),
            max_retries: max_retries.or(defaults.max_retries),
            retry_delay_initial_ms: retry_delay_initial_ms.or(defaults.retry_delay_initial_ms),
            retry_delay_max_ms: retry_delay_max_ms.or(defaults.retry_delay_max_ms),
        }
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }
}
