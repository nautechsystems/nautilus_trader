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

//! Python bindings for Bullet configuration types.

use pyo3::prelude::*;

use crate::{
    common::enums::BulletEnvironment,
    config::{BulletDataClientConfig, BulletExecClientConfig},
    factories::BulletExecFactoryConfig,
};
use nautilus_model::identifiers::{AccountId, TraderId};

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl BulletDataClientConfig {
    /// Configuration for the Bullet data client.
    #[new]
    #[pyo3(signature = (
        environment = None,
        base_url_http = None,
        base_url_ws = None,
        proxy_url = None,
        http_timeout_secs = None,
        update_instruments_interval_mins = None,
    ))]
    fn py_new(
        environment: Option<BulletEnvironment>,
        base_url_http: Option<String>,
        base_url_ws: Option<String>,
        proxy_url: Option<String>,
        http_timeout_secs: Option<u64>,
        update_instruments_interval_mins: Option<u64>,
    ) -> Self {
        let defaults = Self::default();
        Self {
            environment: environment.unwrap_or(defaults.environment),
            base_url_http,
            base_url_ws,
            proxy_url,
            http_timeout_secs: http_timeout_secs.unwrap_or(defaults.http_timeout_secs),
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
impl BulletExecClientConfig {
    /// Configuration for the Bullet execution client.
    #[new]
    #[pyo3(signature = (
        private_key = None,
        key_file = None,
        account_address = None,
        environment = None,
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
        private_key: Option<String>,
        key_file: Option<String>,
        account_address: Option<String>,
        environment: Option<BulletEnvironment>,
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
            private_key,
            key_file,
            account_address,
            environment: environment.unwrap_or(defaults.environment),
            base_url_http,
            base_url_ws,
            proxy_url,
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

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl BulletExecFactoryConfig {
    /// Configuration bundle passed to `BulletExecutionClientFactory`.
    #[new]
    #[pyo3(signature = (
        trader_id,
        account_id,
        config,
    ))]
    fn py_new(
        trader_id: TraderId,
        account_id: AccountId,
        config: BulletExecClientConfig,
    ) -> Self {
        Self { trader_id, account_id, config }
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }
}
