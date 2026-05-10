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

//! Python bindings for Betfair configuration.

use nautilus_model::identifiers::{AccountId, TraderId};
use pyo3::prelude::*;

use crate::config::{BetfairDataConfig, BetfairExecConfig};

fn stringify_ids(values: Option<Vec<u64>>) -> Option<Vec<String>> {
    values.map(|values| values.into_iter().map(|value| value.to_string()).collect())
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl BetfairDataConfig {
    /// Configuration for the Betfair live data client.
    #[new]
    #[pyo3(signature = (
        account_currency = None,
        username = None,
        password = None,
        app_key = None,
        proxy_url = None,
        request_rate_per_second = 5,
        default_min_notional = None,
        event_type_ids = None,
        event_type_names = None,
        event_ids = None,
        country_codes = None,
        market_types = None,
        market_ids = None,
        min_market_start_time = None,
        max_market_start_time = None,
        stream_host = None,
        stream_port = None,
        stream_heartbeat_ms = 5_000,
        stream_idle_timeout_ms = 60_000,
        stream_reconnect_delay_initial_ms = 2_000,
        stream_reconnect_delay_max_ms = 30_000,
        stream_use_tls = true,
        stream_conflate_ms = None,
        subscription_delay_secs = None,
        subscribe_race_data = false,
    ))]
    #[expect(clippy::too_many_arguments)]
    fn py_new(
        account_currency: Option<String>,
        username: Option<String>,
        password: Option<String>,
        app_key: Option<String>,
        proxy_url: Option<String>,
        request_rate_per_second: u32,
        default_min_notional: Option<f64>,
        event_type_ids: Option<Vec<u64>>,
        event_type_names: Option<Vec<String>>,
        event_ids: Option<Vec<u64>>,
        country_codes: Option<Vec<String>>,
        market_types: Option<Vec<String>>,
        market_ids: Option<Vec<String>>,
        min_market_start_time: Option<String>,
        max_market_start_time: Option<String>,
        stream_host: Option<String>,
        stream_port: Option<u16>,
        stream_heartbeat_ms: u64,
        stream_idle_timeout_ms: u64,
        stream_reconnect_delay_initial_ms: u64,
        stream_reconnect_delay_max_ms: u64,
        stream_use_tls: bool,
        stream_conflate_ms: Option<u64>,
        subscription_delay_secs: Option<u64>,
        subscribe_race_data: bool,
    ) -> Self {
        Self {
            account_currency: account_currency.unwrap_or_else(|| "GBP".to_string()),
            username,
            password,
            app_key,
            proxy_url,
            request_rate_per_second,
            default_min_notional,
            event_type_ids: stringify_ids(event_type_ids),
            event_type_names,
            event_ids: stringify_ids(event_ids),
            country_codes,
            market_types,
            market_ids,
            min_market_start_time,
            max_market_start_time,
            stream_host,
            stream_port,
            stream_heartbeat_ms,
            stream_idle_timeout_ms,
            stream_reconnect_delay_initial_ms,
            stream_reconnect_delay_max_ms,
            stream_use_tls,
            stream_conflate_ms,
            subscription_delay_secs: subscription_delay_secs.unwrap_or(3),
            subscribe_race_data,
        }
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl BetfairExecConfig {
    /// Configuration for the Betfair live execution client.
    #[new]
    #[pyo3(signature = (
        trader_id = None,
        account_id = None,
        account_currency = None,
        username = None,
        password = None,
        app_key = None,
        proxy_url = None,
        request_rate_per_second = 5,
        order_request_rate_per_second = 20,
        stream_host = None,
        stream_port = None,
        stream_heartbeat_ms = 5_000,
        stream_idle_timeout_ms = 60_000,
        stream_reconnect_delay_initial_ms = 2_000,
        stream_reconnect_delay_max_ms = 30_000,
        stream_use_tls = true,
        stream_market_ids_filter = None,
        ignore_external_orders = false,
        calculate_account_state = true,
        request_account_state_secs = 300,
        reconcile_market_ids_only = false,
        reconcile_market_ids = None,
        use_market_version = false,
    ))]
    #[expect(clippy::too_many_arguments)]
    fn py_new(
        trader_id: Option<TraderId>,
        account_id: Option<AccountId>,
        account_currency: Option<String>,
        username: Option<String>,
        password: Option<String>,
        app_key: Option<String>,
        proxy_url: Option<String>,
        request_rate_per_second: u32,
        order_request_rate_per_second: u32,
        stream_host: Option<String>,
        stream_port: Option<u16>,
        stream_heartbeat_ms: u64,
        stream_idle_timeout_ms: u64,
        stream_reconnect_delay_initial_ms: u64,
        stream_reconnect_delay_max_ms: u64,
        stream_use_tls: bool,
        stream_market_ids_filter: Option<Vec<String>>,
        ignore_external_orders: bool,
        calculate_account_state: bool,
        request_account_state_secs: u64,
        reconcile_market_ids_only: bool,
        reconcile_market_ids: Option<Vec<String>>,
        use_market_version: bool,
    ) -> Self {
        Self {
            trader_id: trader_id.unwrap_or_else(|| TraderId::from("TRADER-001")),
            account_id: account_id.unwrap_or_else(|| AccountId::from("BETFAIR-001")),
            account_currency: account_currency.unwrap_or_else(|| "GBP".to_string()),
            username,
            password,
            app_key,
            proxy_url,
            request_rate_per_second,
            order_request_rate_per_second,
            stream_host,
            stream_port,
            stream_heartbeat_ms,
            stream_idle_timeout_ms,
            stream_reconnect_delay_initial_ms,
            stream_reconnect_delay_max_ms,
            stream_use_tls,
            stream_market_ids_filter,
            ignore_external_orders,
            calculate_account_state,
            request_account_state_secs,
            reconcile_market_ids_only,
            reconcile_market_ids,
            use_market_version,
        }
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }
}
