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

//! Python bindings for Interactive Brokers configuration types.

use nautilus_model::identifiers::InstrumentId;
use pyo3::prelude::*;

use crate::config::{
    DockerizedIBGatewayConfig, InteractiveBrokersDataClientConfig,
    InteractiveBrokersExecClientConfig, InteractiveBrokersInstrumentProviderConfig, MarketDataType,
    TradingMode,
};

#[pymethods]
impl MarketDataType {
    #[classattr]
    const REALTIME: i32 = 1;

    #[classattr]
    const FROZEN: i32 = 2;

    #[classattr]
    const DELAYED: i32 = 3;

    #[classattr]
    const DELAYED_FROZEN: i32 = 4;
}

#[pymethods]
impl InteractiveBrokersDataClientConfig {
    /// Creates a new `InteractiveBrokersDataClientConfig` instance.
    #[new]
    #[pyo3(signature = (host=None, port=None, client_id=None, use_regular_trading_hours=None, market_data_type=None, ignore_quote_tick_size_updates=None, connection_timeout=None, request_timeout=None, handle_revised_bars=None, batch_quotes=None))]
    #[allow(clippy::too_many_arguments)]
    fn py_new(
        host: Option<String>,
        port: Option<u16>,
        client_id: Option<i32>,
        use_regular_trading_hours: Option<bool>,
        market_data_type: Option<MarketDataType>,
        ignore_quote_tick_size_updates: Option<bool>,
        connection_timeout: Option<u64>,
        request_timeout: Option<u64>,
        handle_revised_bars: Option<bool>,
        batch_quotes: Option<bool>,
    ) -> Self {
        Self {
            host: host.unwrap_or_else(|| crate::common::consts::DEFAULT_HOST.to_string()),
            port: port.unwrap_or(crate::common::consts::DEFAULT_PORT),
            client_id: client_id.unwrap_or(crate::common::consts::DEFAULT_CLIENT_ID),
            use_regular_trading_hours: use_regular_trading_hours.unwrap_or(true),
            market_data_type: market_data_type.unwrap_or_default(),
            ignore_quote_tick_size_updates: ignore_quote_tick_size_updates.unwrap_or(false),
            connection_timeout: connection_timeout.unwrap_or(300),
            request_timeout: request_timeout.unwrap_or(60),
            handle_revised_bars: handle_revised_bars.unwrap_or(false),
            batch_quotes: batch_quotes.unwrap_or(true),
        }
    }

    /// Returns the host.
    #[getter]
    fn host(&self) -> &str {
        &self.host
    }

    /// Returns the port.
    #[getter]
    fn port(&self) -> u16 {
        self.port
    }

    /// Returns the client ID.
    #[getter]
    fn client_id(&self) -> i32 {
        self.client_id
    }

    /// Returns whether to use regular trading hours only.
    #[getter]
    fn use_regular_trading_hours(&self) -> bool {
        self.use_regular_trading_hours
    }

    /// Returns the market data type.
    #[getter]
    fn market_data_type(&self) -> MarketDataType {
        self.market_data_type
    }

    /// Returns whether to ignore quote tick size updates.
    #[getter]
    fn ignore_quote_tick_size_updates(&self) -> bool {
        self.ignore_quote_tick_size_updates
    }

    /// Returns the connection timeout in seconds.
    #[getter]
    fn connection_timeout(&self) -> u64 {
        self.connection_timeout
    }

    /// Returns the request timeout in seconds.
    #[getter]
    fn request_timeout(&self) -> u64 {
        self.request_timeout
    }

    /// Returns whether to handle revised bars.
    #[getter]
    fn handle_revised_bars(&self) -> bool {
        self.handle_revised_bars
    }

    /// Returns whether to use batch quotes.
    #[getter]
    fn batch_quotes(&self) -> bool {
        self.batch_quotes
    }
}

#[pymethods]
impl InteractiveBrokersExecClientConfig {
    /// Creates a new `InteractiveBrokersExecClientConfig` instance.
    #[new]
    #[pyo3(signature = (host=None, port=None, client_id=None, account_id=None, connection_timeout=None, request_timeout=None, fetch_all_open_orders=None, track_option_exercise_from_position_update=None))]
    fn py_new(
        host: Option<String>,
        port: Option<u16>,
        client_id: Option<i32>,
        account_id: Option<String>,
        connection_timeout: Option<u64>,
        request_timeout: Option<u64>,
        fetch_all_open_orders: Option<bool>,
        track_option_exercise_from_position_update: Option<bool>,
    ) -> Self {
        Self {
            host: host.unwrap_or_else(|| crate::common::consts::DEFAULT_HOST.to_string()),
            port: port.unwrap_or(crate::common::consts::DEFAULT_PORT),
            client_id: client_id.unwrap_or(crate::common::consts::DEFAULT_CLIENT_ID),
            account_id,
            connection_timeout: connection_timeout.unwrap_or(300),
            request_timeout: request_timeout.unwrap_or(60),
            fetch_all_open_orders: fetch_all_open_orders.unwrap_or(false),
            track_option_exercise_from_position_update: track_option_exercise_from_position_update
                .unwrap_or(false),
        }
    }

    /// Returns the host.
    #[getter]
    fn host(&self) -> &str {
        &self.host
    }

    /// Returns the port.
    #[getter]
    fn port(&self) -> u16 {
        self.port
    }

    /// Returns the client ID.
    #[getter]
    fn client_id(&self) -> i32 {
        self.client_id
    }

    /// Returns the account ID.
    #[getter]
    fn account_id(&self) -> Option<String> {
        self.account_id.clone()
    }

    /// Returns the connection timeout in seconds.
    #[getter]
    fn connection_timeout(&self) -> u64 {
        self.connection_timeout
    }

    /// Returns the request timeout in seconds.
    #[getter]
    fn request_timeout(&self) -> u64 {
        self.request_timeout
    }

    /// Returns whether to fetch all open orders.
    #[getter]
    fn fetch_all_open_orders(&self) -> bool {
        self.fetch_all_open_orders
    }

    /// Returns whether to track option exercise from position updates.
    #[getter]
    fn track_option_exercise_from_position_update(&self) -> bool {
        self.track_option_exercise_from_position_update
    }
}

#[pymethods]
impl InteractiveBrokersInstrumentProviderConfig {
    /// Creates a new `InteractiveBrokersInstrumentProviderConfig` instance.
    #[new]
    #[pyo3(signature = (symbology_method=None, load_ids=None, load_contracts=None, min_expiry_days=None, max_expiry_days=None, build_options_chain=None, build_futures_chain=None, cache_validity_days=None, convert_exchange_to_mic_venue=None, symbol_to_mic_venue=None, filter_sec_types=None, cache_path=None))]
    #[allow(clippy::too_many_arguments)]
    fn py_new(
        py: Python<'_>,
        symbology_method: Option<crate::config::SymbologyMethod>,
        load_ids: Option<std::collections::HashSet<InstrumentId>>,
        load_contracts: Option<Py<pyo3::types::PyList>>,
        min_expiry_days: Option<u32>,
        max_expiry_days: Option<u32>,
        build_options_chain: Option<bool>,
        build_futures_chain: Option<bool>,
        cache_validity_days: Option<u32>,
        convert_exchange_to_mic_venue: Option<bool>,
        symbol_to_mic_venue: Option<std::collections::HashMap<String, String>>,
        filter_sec_types: Option<std::collections::HashSet<String>>,
        cache_path: Option<String>,
    ) -> PyResult<Self> {
        Ok(Self {
            symbology_method: symbology_method.unwrap_or_default(),
            load_ids: load_ids.unwrap_or_default(),
            load_contracts: if let Some(c) = load_contracts {
                crate::python::conversion::py_list_to_json_values(c.bind(py))?
            } else {
                Vec::new()
            },
            min_expiry_days,
            max_expiry_days,
            build_options_chain,
            build_futures_chain,
            cache_validity_days,
            convert_exchange_to_mic_venue: convert_exchange_to_mic_venue.unwrap_or(false),
            symbol_to_mic_venue: symbol_to_mic_venue.unwrap_or_default(),
            filter_sec_types: filter_sec_types.unwrap_or_default(),
            cache_path,
        })
    }

    /// Returns the symbology method.
    #[getter]
    fn symbology_method(&self) -> crate::config::SymbologyMethod {
        self.symbology_method
    }

    /// Returns the instrument IDs to load on startup.
    #[getter]
    fn load_ids(&self) -> std::collections::HashSet<InstrumentId> {
        self.load_ids.clone()
    }

    /// Returns the IB contracts to load on startup.
    #[getter]
    fn load_contracts(&self, py: Python<'_>) -> PyResult<Py<pyo3::types::PyList>> {
        let json_mod = py.import("json")?;
        let list = pyo3::types::PyList::empty(py);

        for value in &self.load_contracts {
            let json_str = value.to_string();
            let dict = json_mod.call_method1("loads", (json_str,))?;
            list.append(dict)?;
        }
        Ok(list.unbind())
    }

    /// Returns the minimum expiry days.
    #[getter]
    fn min_expiry_days(&self) -> Option<u32> {
        self.min_expiry_days
    }

    /// Returns the maximum expiry days.
    #[getter]
    fn max_expiry_days(&self) -> Option<u32> {
        self.max_expiry_days
    }

    /// Returns whether to build full options chain.
    #[getter]
    fn build_options_chain(&self) -> Option<bool> {
        self.build_options_chain
    }

    /// Returns whether to build full futures chain.
    #[getter]
    fn build_futures_chain(&self) -> Option<bool> {
        self.build_futures_chain
    }

    /// Returns the cache validity in days.
    #[getter]
    fn cache_validity_days(&self) -> Option<u32> {
        self.cache_validity_days
    }

    /// Returns whether to convert IB exchanges to MIC venues.
    #[getter]
    fn convert_exchange_to_mic_venue(&self) -> bool {
        self.convert_exchange_to_mic_venue
    }

    /// Returns the symbol to MIC venue mapping.
    #[getter]
    fn symbol_to_mic_venue(&self) -> std::collections::HashMap<String, String> {
        self.symbol_to_mic_venue.clone()
    }

    /// Returns the filter security types.
    #[getter]
    fn filter_sec_types(&self) -> Vec<String> {
        self.filter_sec_types.iter().cloned().collect()
    }

    /// Returns the cache path for persistent instrument caching.
    #[getter]
    fn cache_path(&self) -> Option<String> {
        self.cache_path.clone()
    }

    /// Sets the cache path for persistent instrument caching.
    #[setter]
    fn set_cache_path(&mut self, cache_path: Option<String>) {
        self.cache_path = cache_path;
    }
}

#[pymethods]
impl DockerizedIBGatewayConfig {
    /// Creates a new `DockerizedIBGatewayConfig` instance.
    #[new]
    #[pyo3(signature = (username=None, password=None, trading_mode=None, read_only_api=None, timeout=None, container_image=None, vnc_port=None))]
    fn py_new(
        username: Option<String>,
        password: Option<String>,
        trading_mode: Option<TradingMode>,
        read_only_api: Option<bool>,
        timeout: Option<u64>,
        container_image: Option<String>,
        vnc_port: Option<u16>,
    ) -> Self {
        Self {
            username,
            password,
            trading_mode: trading_mode.unwrap_or_default(),
            read_only_api: read_only_api.unwrap_or(true),
            timeout: timeout.unwrap_or(300),
            container_image: container_image
                .unwrap_or_else(|| "ghcr.io/gnzsnz/ib-gateway:stable".to_string()),
            vnc_port,
        }
    }

    /// Returns the username.
    #[getter]
    fn username(&self) -> Option<String> {
        self.username.clone()
    }

    /// Returns the password (masked for security).
    #[getter]
    fn password(&self) -> Option<String> {
        self.password.as_ref().map(|_| "********".to_string())
    }

    /// Returns the trading mode.
    #[getter]
    fn trading_mode(&self) -> TradingMode {
        self.trading_mode
    }

    /// Returns whether read-only API is enabled.
    #[getter]
    fn read_only_api(&self) -> bool {
        self.read_only_api
    }

    /// Returns the timeout in seconds.
    #[getter]
    fn timeout(&self) -> u64 {
        self.timeout
    }

    /// Returns the container image.
    #[getter]
    fn container_image(&self) -> &str {
        &self.container_image
    }

    /// Returns the VNC port.
    #[getter]
    fn vnc_port(&self) -> Option<u16> {
        self.vnc_port
    }
}
