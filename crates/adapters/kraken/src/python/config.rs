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

//! Python bindings for Kraken configuration.

use nautilus_model::identifiers::{AccountId, TraderId};
use pyo3::prelude::*;

use crate::{
    common::enums::{KrakenEnvironment, KrakenProductType},
    config::{KrakenDataClientConfig, KrakenExecClientConfig},
};

#[pymethods]
impl KrakenDataClientConfig {
    #[new]
    #[pyo3(signature = (
        product_type = None,
        environment = None,
        api_key = None,
        api_secret = None,
        base_url = None,
        ws_public_url = None,
        ws_private_url = None,
        http_proxy = None,
        timeout_secs = None,
        heartbeat_interval_secs = None,
        max_requests_per_second = None,
    ))]
    #[allow(clippy::too_many_arguments)]
    fn py_new(
        product_type: Option<KrakenProductType>,
        environment: Option<KrakenEnvironment>,
        api_key: Option<String>,
        api_secret: Option<String>,
        base_url: Option<String>,
        ws_public_url: Option<String>,
        ws_private_url: Option<String>,
        http_proxy: Option<String>,
        timeout_secs: Option<u64>,
        heartbeat_interval_secs: Option<u64>,
        max_requests_per_second: Option<u32>,
    ) -> Self {
        let defaults = Self::default();
        Self {
            api_key,
            api_secret,
            product_type: product_type.unwrap_or(defaults.product_type),
            environment: environment.unwrap_or(defaults.environment),
            base_url,
            ws_public_url,
            ws_private_url,
            http_proxy,
            ws_proxy: None,
            timeout_secs: timeout_secs.or(defaults.timeout_secs),
            heartbeat_interval_secs: heartbeat_interval_secs.or(defaults.heartbeat_interval_secs),
            max_requests_per_second,
        }
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }
}

#[pymethods]
impl KrakenExecClientConfig {
    #[new]
    #[pyo3(signature = (
        trader_id,
        account_id,
        api_key,
        api_secret,
        product_type = None,
        environment = None,
        base_url = None,
        ws_url = None,
        http_proxy = None,
        timeout_secs = None,
        heartbeat_interval_secs = None,
        max_requests_per_second = None,
    ))]
    #[allow(clippy::too_many_arguments)]
    fn py_new(
        trader_id: TraderId,
        account_id: AccountId,
        api_key: String,
        api_secret: String,
        product_type: Option<KrakenProductType>,
        environment: Option<KrakenEnvironment>,
        base_url: Option<String>,
        ws_url: Option<String>,
        http_proxy: Option<String>,
        timeout_secs: Option<u64>,
        heartbeat_interval_secs: Option<u64>,
        max_requests_per_second: Option<u32>,
    ) -> Self {
        let defaults = Self::default();
        Self {
            trader_id,
            account_id,
            api_key,
            api_secret,
            product_type: product_type.unwrap_or(defaults.product_type),
            environment: environment.unwrap_or(defaults.environment),
            base_url,
            ws_url,
            http_proxy,
            ws_proxy: None,
            timeout_secs: timeout_secs.or(defaults.timeout_secs),
            heartbeat_interval_secs: heartbeat_interval_secs.or(defaults.heartbeat_interval_secs),
            max_requests_per_second,
        }
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }
}
