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

//! Python bindings for OKX configuration.

use nautilus_model::identifiers::{AccountId, TraderId};
use pyo3::prelude::*;

use crate::{
    common::enums::{OKXEnvironment, OKXInstrumentType, OKXMarginMode, OKXVipLevel},
    config::{OKXDataClientConfig, OKXExecClientConfig},
};

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl OKXDataClientConfig {
    /// Configuration for the OKX data client.
    #[new]
    #[pyo3(signature = (
        instrument_types = None,
        environment = None,
        api_key = None,
        api_secret = None,
        api_passphrase = None,
        base_url_http = None,
        base_url_ws_public = None,
        base_url_ws_business = None,
        proxy_url = None,
        http_timeout_secs = None,
        max_retries = None,
        retry_delay_initial_ms = None,
        retry_delay_max_ms = None,
        update_instruments_interval_mins = None,
        vip_level = None,
    ))]
    #[expect(clippy::too_many_arguments)]
    fn py_new(
        instrument_types: Option<Vec<OKXInstrumentType>>,
        environment: Option<OKXEnvironment>,
        api_key: Option<String>,
        api_secret: Option<String>,
        api_passphrase: Option<String>,
        base_url_http: Option<String>,
        base_url_ws_public: Option<String>,
        base_url_ws_business: Option<String>,
        proxy_url: Option<String>,
        http_timeout_secs: Option<u64>,
        max_retries: Option<u32>,
        retry_delay_initial_ms: Option<u64>,
        retry_delay_max_ms: Option<u64>,
        update_instruments_interval_mins: Option<u64>,
        vip_level: Option<OKXVipLevel>,
    ) -> Self {
        let defaults = Self::default();
        Self {
            api_key,
            api_secret,
            api_passphrase,
            instrument_types: instrument_types.unwrap_or(defaults.instrument_types),
            contract_types: None,
            instrument_families: None,
            base_url_http,
            base_url_ws_public,
            base_url_ws_business,
            proxy_url,
            environment: environment.unwrap_or(defaults.environment),
            http_timeout_secs: http_timeout_secs.unwrap_or(defaults.http_timeout_secs),
            max_retries: max_retries.unwrap_or(defaults.max_retries),
            retry_delay_initial_ms: retry_delay_initial_ms
                .unwrap_or(defaults.retry_delay_initial_ms),
            retry_delay_max_ms: retry_delay_max_ms.unwrap_or(defaults.retry_delay_max_ms),
            update_instruments_interval_mins: update_instruments_interval_mins
                .unwrap_or(defaults.update_instruments_interval_mins),
            vip_level,
            transport_backend: defaults.transport_backend,
        }
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl OKXExecClientConfig {
    /// Configuration for the OKX execution client.
    #[new]
    #[pyo3(signature = (
        trader_id,
        account_id,
        instrument_types = None,
        environment = None,
        api_key = None,
        api_secret = None,
        api_passphrase = None,
        base_url_http = None,
        base_url_ws_private = None,
        base_url_ws_business = None,
        proxy_url = None,
        http_timeout_secs = None,
        max_retries = None,
        retry_delay_initial_ms = None,
        retry_delay_max_ms = None,
        margin_mode = None,
    ))]
    #[expect(clippy::too_many_arguments)]
    fn py_new(
        trader_id: TraderId,
        account_id: AccountId,
        instrument_types: Option<Vec<OKXInstrumentType>>,
        environment: Option<OKXEnvironment>,
        api_key: Option<String>,
        api_secret: Option<String>,
        api_passphrase: Option<String>,
        base_url_http: Option<String>,
        base_url_ws_private: Option<String>,
        base_url_ws_business: Option<String>,
        proxy_url: Option<String>,
        http_timeout_secs: Option<u64>,
        max_retries: Option<u32>,
        retry_delay_initial_ms: Option<u64>,
        retry_delay_max_ms: Option<u64>,
        margin_mode: Option<OKXMarginMode>,
    ) -> Self {
        let defaults = Self::default();
        Self {
            trader_id,
            account_id,
            api_key,
            api_secret,
            api_passphrase,
            instrument_types: instrument_types.unwrap_or(defaults.instrument_types),
            contract_types: None,
            instrument_families: None,
            base_url_http,
            base_url_ws_private,
            base_url_ws_business,
            proxy_url,
            environment: environment.unwrap_or(defaults.environment),
            http_timeout_secs: http_timeout_secs.unwrap_or(defaults.http_timeout_secs),
            use_fills_channel: defaults.use_fills_channel,
            use_mm_mass_cancel: defaults.use_mm_mass_cancel,
            max_retries: max_retries.unwrap_or(defaults.max_retries),
            retry_delay_initial_ms: retry_delay_initial_ms
                .unwrap_or(defaults.retry_delay_initial_ms),
            retry_delay_max_ms: retry_delay_max_ms.unwrap_or(defaults.retry_delay_max_ms),
            margin_mode,
            use_spot_margin: defaults.use_spot_margin,
            transport_backend: defaults.transport_backend,
        }
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }
}
