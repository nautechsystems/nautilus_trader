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

use std::{collections::HashMap, hash::BuildHasher, time::Duration};

use nautilus_common::{
    cache::CacheConfig, enums::Environment, logging::logger::LoggerConfig,
    msgbus::MessageBusConfig, python::config_error_to_pyvalue_err,
};
use nautilus_core::{UUID4, python::to_pyvalue_err};
use nautilus_model::{
    enums::BarIntervalType,
    identifiers::{ClientId, TraderId, Venue},
};
use nautilus_portfolio::config::PortfolioConfig;
use nautilus_trading::ImportableControllerConfig;
use pyo3::{
    IntoPyObject, Py, PyAny, PyResult, Python, pymethods,
    types::{PyAnyMethods, PyDict, PyDictMethods},
};

use crate::config::{
    DataClientConfig, ExecutionClientConfig, InstrumentProviderConfig, LiveDataEngineConfig,
    LiveExecutionEngineConfig, LiveNodeConfig, LiveRiskEngineConfig, PluginConfig,
    QueueMonitorConfig, RoutingConfig, duration_from_secs_f64, parse_rate_limit,
    validate_max_notional_per_order,
};

// Coerces a PyO3 input into `BarIntervalType`, accepting both the enum (modern Rust
// surface) and the legacy Python v1 string form (`"left-open"` / `"right-open"`).
fn coerce_bar_interval_type(value: &Py<PyAny>) -> PyResult<BarIntervalType> {
    Python::attach(|py| {
        let bound = value.bind(py);
        if let Ok(variant) = bound.extract::<BarIntervalType>() {
            return Ok(variant);
        }

        let raw = bound.extract::<String>().map_err(|_| {
            to_pyvalue_err("`time_bars_interval_type` must be a string or BarIntervalType")
        })?;

        match raw.to_ascii_uppercase().replace('-', "_").as_str() {
            "LEFT_OPEN" => Ok(BarIntervalType::LeftOpen),
            "RIGHT_OPEN" => Ok(BarIntervalType::RightOpen),
            _ => Err(to_pyvalue_err(format!(
                "invalid `time_bars_interval_type`: {raw:?} (expected 'left-open' or 'right-open')"
            ))),
        }
    })
}

/// Converts a Python value into a [`serde_json::Value`].
fn py_to_json_value(bound: &pyo3::Bound<'_, PyAny>) -> PyResult<serde_json::Value> {
    // Check bool before int since Python `bool` is a subclass of `int`
    if let Ok(b) = bound.extract::<bool>() {
        Ok(serde_json::Value::Bool(b))
    } else if let Ok(s) = bound.extract::<String>() {
        Ok(serde_json::Value::String(s))
    } else if let Ok(i) = bound.extract::<i64>() {
        Ok(serde_json::Value::Number(serde_json::Number::from(i)))
    } else if let Ok(f) = bound.extract::<f64>() {
        Ok(serde_json::Number::from_f64(f)
            .map_or(serde_json::Value::Null, serde_json::Value::Number))
    } else if let Ok(dict) = bound.cast::<PyDict>() {
        let mut obj = serde_json::Map::with_capacity(dict.len());
        for (key, value) in dict.iter() {
            obj.insert(key.extract::<String>()?, py_to_json_value(&value)?);
        }
        Ok(serde_json::Value::Object(obj))
    } else if let Ok(items) = bound.extract::<Vec<Py<PyAny>>>() {
        // Handle list/tuple/set
        let py = bound.py();
        let arr: Vec<serde_json::Value> = items
            .iter()
            .map(|item| py_to_json_value(item.bind(py)))
            .collect::<PyResult<_>>()?;
        Ok(serde_json::Value::Array(arr))
    } else {
        // Fall back to string representation
        let s: String = bound.str()?.extract()?;
        Ok(serde_json::Value::String(s))
    }
}

/// Converts a JSON configuration value into a Python object.
///
/// # Errors
///
/// Returns an error if Python object construction fails.
pub fn json_value_to_py(py: Python<'_>, value: &serde_json::Value) -> PyResult<Py<PyAny>> {
    match value {
        serde_json::Value::Null => Ok(py.None()),
        serde_json::Value::Bool(b) => Ok((*b).into_pyobject(py)?.to_owned().into_any().unbind()),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(i.into_pyobject(py)?.into_any().unbind())
            } else if let Some(u) = n.as_u64() {
                Ok(u.into_pyobject(py)?.into_any().unbind())
            } else if let Some(f) = n.as_f64() {
                Ok(f.into_pyobject(py)?.into_any().unbind())
            } else {
                Ok(n.to_string().into_pyobject(py)?.into_any().unbind())
            }
        }
        serde_json::Value::String(s) => Ok(s.into_pyobject(py)?.into_any().unbind()),
        serde_json::Value::Array(arr) => {
            let items: Vec<Py<PyAny>> = arr
                .iter()
                .map(|v| json_value_to_py(py, v))
                .collect::<PyResult<_>>()?;
            Ok(pyo3::types::PyList::new(py, items)?.into_any().unbind())
        }
        serde_json::Value::Object(obj) => {
            let dict = pyo3::types::PyDict::new(py);
            for (k, v) in obj {
                dict.set_item(k, json_value_to_py(py, v)?)?;
            }
            Ok(dict.into_any().unbind())
        }
    }
}

/// Converts Python mapping values into JSON values.
///
/// # Errors
///
/// Returns an error if a Python value cannot be converted.
pub fn coerce_json_config<S: BuildHasher>(
    raw: HashMap<String, Py<PyAny>, S>,
) -> PyResult<HashMap<String, serde_json::Value>> {
    Python::attach(|py| -> PyResult<HashMap<String, serde_json::Value>> {
        let mut result = HashMap::with_capacity(raw.len());
        for (key, value) in raw {
            let json_value = py_to_json_value(value.bind(py))?;
            result.insert(key, json_value);
        }
        Ok(result)
    })
}

// Normalizes a Python `max_notional_per_order` dict (values can be `int`, `float`,
// `str`, or `Decimal`, matching the legacy Python v1 config contract) into the
// string-keyed map stored on `LiveRiskEngineConfig`.
fn coerce_max_notional_per_order(
    raw: HashMap<String, Py<PyAny>>,
) -> PyResult<HashMap<String, String>> {
    Python::attach(|py| -> PyResult<HashMap<String, String>> {
        let mut result = HashMap::with_capacity(raw.len());
        for (instrument_id, value) in raw {
            let value_str: String = value.bind(py).str()?.extract()?;
            result.insert(instrument_id, value_str);
        }
        Ok(result)
    })
}

#[pyo3_stub_gen::derive::gen_stub_pymethods]
#[pymethods]
impl LiveDataEngineConfig {
    /// Configuration for live data engines.
    #[new]
    #[expect(clippy::too_many_arguments)]
    #[allow(
        clippy::needless_pass_by_value,
        reason = "PyO3 #[new] requires owned params"
    )]
    #[pyo3(signature = (time_bars_build_with_no_updates=None, time_bars_timestamp_on_close=None, time_bars_skip_first_non_full_bar=None, time_bars_interval_type=None, time_bars_build_delay=None, time_bars_origin_offset=None, validate_data_sequence=None, buffer_deltas=None, emit_quotes_from_book=None, emit_quotes_from_book_depths=None, external_clients=None, debug=None))]
    fn py_new(
        time_bars_build_with_no_updates: Option<bool>,
        time_bars_timestamp_on_close: Option<bool>,
        time_bars_skip_first_non_full_bar: Option<bool>,
        time_bars_interval_type: Option<Py<PyAny>>,
        time_bars_build_delay: Option<u64>,
        time_bars_origin_offset: Option<HashMap<String, u64>>,
        validate_data_sequence: Option<bool>,
        buffer_deltas: Option<bool>,
        emit_quotes_from_book: Option<bool>,
        emit_quotes_from_book_depths: Option<bool>,
        external_clients: Option<Vec<ClientId>>,
        debug: Option<bool>,
    ) -> PyResult<Self> {
        let default = Self::default();
        let time_bars_interval_type = match time_bars_interval_type {
            Some(ref obj) => coerce_bar_interval_type(obj)?,
            None => default.time_bars_interval_type,
        };
        Ok(Self {
            time_bars_build_with_no_updates: time_bars_build_with_no_updates
                .unwrap_or(default.time_bars_build_with_no_updates),
            time_bars_timestamp_on_close: time_bars_timestamp_on_close
                .unwrap_or(default.time_bars_timestamp_on_close),
            time_bars_skip_first_non_full_bar: time_bars_skip_first_non_full_bar
                .unwrap_or(default.time_bars_skip_first_non_full_bar),
            time_bars_interval_type,
            time_bars_build_delay: time_bars_build_delay.unwrap_or(default.time_bars_build_delay),
            time_bars_origin_offset: time_bars_origin_offset.unwrap_or_default(),
            validate_data_sequence: validate_data_sequence
                .unwrap_or(default.validate_data_sequence),
            buffer_deltas: buffer_deltas.unwrap_or(default.buffer_deltas),
            emit_quotes_from_book: emit_quotes_from_book.unwrap_or(default.emit_quotes_from_book),
            emit_quotes_from_book_depths: emit_quotes_from_book_depths
                .unwrap_or(default.emit_quotes_from_book_depths),
            external_clients,
            debug: debug.unwrap_or(default.debug),
            qsize: default.qsize,
        })
    }

    #[getter]
    #[pyo3(name = "time_bars_build_with_no_updates")]
    const fn py_time_bars_build_with_no_updates(&self) -> bool {
        self.time_bars_build_with_no_updates
    }

    #[getter]
    #[pyo3(name = "time_bars_timestamp_on_close")]
    const fn py_time_bars_timestamp_on_close(&self) -> bool {
        self.time_bars_timestamp_on_close
    }

    #[getter]
    #[pyo3(name = "time_bars_skip_first_non_full_bar")]
    const fn py_time_bars_skip_first_non_full_bar(&self) -> bool {
        self.time_bars_skip_first_non_full_bar
    }

    #[getter]
    #[pyo3(name = "time_bars_interval_type")]
    const fn py_time_bars_interval_type(&self) -> BarIntervalType {
        self.time_bars_interval_type
    }

    #[getter]
    #[pyo3(name = "time_bars_build_delay")]
    const fn py_time_bars_build_delay(&self) -> u64 {
        self.time_bars_build_delay
    }

    #[getter]
    #[pyo3(name = "time_bars_origin_offset")]
    fn py_time_bars_origin_offset(&self) -> HashMap<String, u64> {
        self.time_bars_origin_offset.clone()
    }

    #[getter]
    #[pyo3(name = "validate_data_sequence")]
    const fn py_validate_data_sequence(&self) -> bool {
        self.validate_data_sequence
    }

    #[getter]
    #[pyo3(name = "buffer_deltas")]
    const fn py_buffer_deltas(&self) -> bool {
        self.buffer_deltas
    }

    #[getter]
    #[pyo3(name = "emit_quotes_from_book")]
    const fn py_emit_quotes_from_book(&self) -> bool {
        self.emit_quotes_from_book
    }

    #[getter]
    #[pyo3(name = "emit_quotes_from_book_depths")]
    const fn py_emit_quotes_from_book_depths(&self) -> bool {
        self.emit_quotes_from_book_depths
    }

    #[getter]
    #[pyo3(name = "external_clients")]
    fn py_external_clients(&self) -> Option<Vec<ClientId>> {
        self.external_clients.clone()
    }

    #[getter]
    #[pyo3(name = "debug")]
    const fn py_debug(&self) -> bool {
        self.debug
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }

    fn __str__(&self) -> String {
        format!("{self:?}")
    }
}

#[pyo3_stub_gen::derive::gen_stub_pymethods]
#[pymethods]
impl LiveRiskEngineConfig {
    /// Configuration for live risk engines.
    #[new]
    #[pyo3(signature = (bypass=None, max_order_submit_rate=None, max_order_modify_rate=None, max_notional_per_order=None, full_position_exit_venues=None, debug=None))]
    fn py_new(
        bypass: Option<bool>,
        max_order_submit_rate: Option<String>,
        max_order_modify_rate: Option<String>,
        max_notional_per_order: Option<HashMap<String, Py<PyAny>>>,
        full_position_exit_venues: Option<Vec<Venue>>,
        debug: Option<bool>,
    ) -> PyResult<Self> {
        let default = Self::default();
        let max_order_submit_rate =
            max_order_submit_rate.unwrap_or_else(|| default.max_order_submit_rate.clone());
        let max_order_modify_rate =
            max_order_modify_rate.unwrap_or_else(|| default.max_order_modify_rate.clone());
        let max_notional_per_order = match max_notional_per_order {
            Some(raw) => coerce_max_notional_per_order(raw)?,
            None => HashMap::new(),
        };
        let full_position_exit_venues = full_position_exit_venues.unwrap_or_default();

        parse_rate_limit(
            "LiveRiskEngineConfig.max_order_submit_rate",
            &max_order_submit_rate,
        )
        .map_err(config_error_to_pyvalue_err)?;
        parse_rate_limit(
            "LiveRiskEngineConfig.max_order_modify_rate",
            &max_order_modify_rate,
        )
        .map_err(config_error_to_pyvalue_err)?;
        validate_max_notional_per_order(
            "LiveRiskEngineConfig.max_notional_per_order",
            &max_notional_per_order,
        )
        .map_err(config_error_to_pyvalue_err)?;

        Ok(Self {
            bypass: bypass.unwrap_or(default.bypass),
            max_order_submit_rate,
            max_order_modify_rate,
            max_notional_per_order,
            full_position_exit_venues,
            debug: debug.unwrap_or(default.debug),
            qsize: default.qsize,
        })
    }

    #[getter]
    #[pyo3(name = "bypass")]
    const fn py_bypass(&self) -> bool {
        self.bypass
    }

    #[getter]
    #[pyo3(name = "max_order_submit_rate")]
    fn py_max_order_submit_rate(&self) -> &str {
        &self.max_order_submit_rate
    }

    #[getter]
    #[pyo3(name = "max_order_modify_rate")]
    fn py_max_order_modify_rate(&self) -> &str {
        &self.max_order_modify_rate
    }

    #[getter]
    #[pyo3(name = "max_notional_per_order")]
    fn py_max_notional_per_order(&self) -> HashMap<String, String> {
        self.max_notional_per_order.clone()
    }

    #[getter]
    #[pyo3(name = "full_position_exit_venues")]
    fn py_full_position_exit_venues(&self) -> Vec<Venue> {
        self.full_position_exit_venues.clone()
    }

    #[getter]
    #[pyo3(name = "debug")]
    const fn py_debug(&self) -> bool {
        self.debug
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }

    fn __str__(&self) -> String {
        format!("{self:?}")
    }
}

#[pyo3_stub_gen::derive::gen_stub_pymethods]
#[pymethods]
impl LiveExecutionEngineConfig {
    /// Configuration for live execution engines.
    #[new]
    #[expect(clippy::too_many_arguments)]
    #[pyo3(signature = (load_cache=None, manage_own_order_books=None, snapshot_positions_interval_secs=None, external_clients=None, allow_overfills=None, reconciliation=None, reconciliation_startup_delay_secs=None, reconciliation_lookback_mins=None, reconciliation_instrument_ids=None, filter_unclaimed_external_orders=None, filter_position_reports=None, filtered_client_order_ids=None, generate_missing_orders=None, inflight_check_interval_ms=None, inflight_check_threshold_ms=None, inflight_check_retries=None, open_check_interval_secs=None, open_check_lookback_mins=None, open_check_threshold_ms=None, open_check_missing_retries=None, open_check_open_only=None, max_single_order_queries_per_cycle=None, single_order_query_delay_ms=None, position_check_interval_secs=None, position_check_lookback_mins=None, position_check_threshold_ms=None, position_check_retries=None, purge_closed_orders_interval_mins=None, purge_closed_orders_buffer_mins=None, purge_closed_positions_interval_mins=None, purge_closed_positions_buffer_mins=None, purge_account_events_interval_mins=None, purge_account_events_lookback_mins=None, own_books_audit_interval_secs=None, debug=None, snapshot_orders=None, snapshot_positions=None))]
    fn py_new(
        load_cache: Option<bool>,
        manage_own_order_books: Option<bool>,
        snapshot_positions_interval_secs: Option<f64>,
        external_clients: Option<Vec<ClientId>>,
        allow_overfills: Option<bool>,
        reconciliation: Option<bool>,
        reconciliation_startup_delay_secs: Option<f64>,
        reconciliation_lookback_mins: Option<u32>,
        reconciliation_instrument_ids: Option<Vec<String>>,
        filter_unclaimed_external_orders: Option<bool>,
        filter_position_reports: Option<bool>,
        filtered_client_order_ids: Option<Vec<String>>,
        generate_missing_orders: Option<bool>,
        inflight_check_interval_ms: Option<u32>,
        inflight_check_threshold_ms: Option<u32>,
        inflight_check_retries: Option<u32>,
        open_check_interval_secs: Option<f64>,
        open_check_lookback_mins: Option<u32>,
        open_check_threshold_ms: Option<u32>,
        open_check_missing_retries: Option<u32>,
        open_check_open_only: Option<bool>,
        max_single_order_queries_per_cycle: Option<u32>,
        single_order_query_delay_ms: Option<u32>,
        position_check_interval_secs: Option<f64>,
        position_check_lookback_mins: Option<u32>,
        position_check_threshold_ms: Option<u32>,
        position_check_retries: Option<u32>,
        purge_closed_orders_interval_mins: Option<u32>,
        purge_closed_orders_buffer_mins: Option<u32>,
        purge_closed_positions_interval_mins: Option<u32>,
        purge_closed_positions_buffer_mins: Option<u32>,
        purge_account_events_interval_mins: Option<u32>,
        purge_account_events_lookback_mins: Option<u32>,
        own_books_audit_interval_secs: Option<f64>,
        debug: Option<bool>,
        snapshot_orders: Option<bool>,
        snapshot_positions: Option<bool>,
    ) -> PyResult<Self> {
        let default = Self::default();

        let config = Self {
            load_cache: load_cache.unwrap_or(default.load_cache),
            manage_own_order_books: manage_own_order_books
                .unwrap_or(default.manage_own_order_books),
            snapshot_orders: snapshot_orders.unwrap_or(default.snapshot_orders),
            snapshot_positions: snapshot_positions.unwrap_or(default.snapshot_positions),
            snapshot_positions_interval_secs,
            external_clients,
            allow_overfills: allow_overfills.unwrap_or(default.allow_overfills),
            reconciliation: reconciliation.unwrap_or(default.reconciliation),
            reconciliation_startup_delay_secs: reconciliation_startup_delay_secs
                .unwrap_or(default.reconciliation_startup_delay_secs),
            reconciliation_lookback_mins,
            reconciliation_instrument_ids,
            filter_unclaimed_external_orders: filter_unclaimed_external_orders
                .unwrap_or(default.filter_unclaimed_external_orders),
            filter_position_reports: filter_position_reports
                .unwrap_or(default.filter_position_reports),
            filtered_client_order_ids,
            generate_missing_orders: generate_missing_orders
                .unwrap_or(default.generate_missing_orders),
            inflight_check_interval_ms: inflight_check_interval_ms
                .unwrap_or(default.inflight_check_interval_ms),
            inflight_check_threshold_ms: inflight_check_threshold_ms
                .unwrap_or(default.inflight_check_threshold_ms),
            inflight_check_retries: inflight_check_retries
                .unwrap_or(default.inflight_check_retries),
            open_check_interval_secs,
            open_check_lookback_mins: open_check_lookback_mins.or(default.open_check_lookback_mins),
            open_check_threshold_ms: open_check_threshold_ms
                .unwrap_or(default.open_check_threshold_ms),
            open_check_missing_retries: open_check_missing_retries
                .unwrap_or(default.open_check_missing_retries),
            open_check_open_only: open_check_open_only.unwrap_or(default.open_check_open_only),
            max_single_order_queries_per_cycle: max_single_order_queries_per_cycle
                .unwrap_or(default.max_single_order_queries_per_cycle),
            single_order_query_delay_ms: single_order_query_delay_ms
                .unwrap_or(default.single_order_query_delay_ms),
            position_check_interval_secs,
            position_check_lookback_mins: position_check_lookback_mins
                .unwrap_or(default.position_check_lookback_mins),
            position_check_threshold_ms: position_check_threshold_ms
                .unwrap_or(default.position_check_threshold_ms),
            position_check_retries: position_check_retries
                .unwrap_or(default.position_check_retries),
            purge_closed_orders_interval_mins,
            purge_closed_orders_buffer_mins,
            purge_closed_positions_interval_mins,
            purge_closed_positions_buffer_mins,
            purge_account_events_interval_mins,
            purge_account_events_lookback_mins,
            purge_from_database: default.purge_from_database,
            debug: debug.unwrap_or(default.debug),
            own_books_audit_interval_secs,
            qsize: default.qsize,
        };
        config
            .validate_runtime_support()
            .map_err(config_error_to_pyvalue_err)?;
        Ok(config)
    }

    #[getter]
    #[pyo3(name = "load_cache")]
    const fn py_load_cache(&self) -> bool {
        self.load_cache
    }

    #[getter]
    #[pyo3(name = "manage_own_order_books")]
    const fn py_manage_own_order_books(&self) -> bool {
        self.manage_own_order_books
    }

    #[getter]
    #[pyo3(name = "snapshot_orders")]
    const fn py_snapshot_orders(&self) -> bool {
        self.snapshot_orders
    }

    #[getter]
    #[pyo3(name = "snapshot_positions")]
    const fn py_snapshot_positions(&self) -> bool {
        self.snapshot_positions
    }

    #[getter]
    #[pyo3(name = "snapshot_positions_interval_secs")]
    const fn py_snapshot_positions_interval_secs(&self) -> Option<f64> {
        self.snapshot_positions_interval_secs
    }

    #[getter]
    #[pyo3(name = "external_clients")]
    fn py_external_clients(&self) -> Option<Vec<ClientId>> {
        self.external_clients.clone()
    }

    #[getter]
    #[pyo3(name = "allow_overfills")]
    const fn py_allow_overfills(&self) -> bool {
        self.allow_overfills
    }

    #[getter]
    #[pyo3(name = "reconciliation")]
    const fn py_reconciliation(&self) -> bool {
        self.reconciliation
    }

    #[getter]
    #[pyo3(name = "reconciliation_startup_delay_secs")]
    const fn py_reconciliation_startup_delay_secs(&self) -> f64 {
        self.reconciliation_startup_delay_secs
    }

    #[getter]
    #[pyo3(name = "reconciliation_lookback_mins")]
    const fn py_reconciliation_lookback_mins(&self) -> Option<u32> {
        self.reconciliation_lookback_mins
    }

    #[getter]
    #[pyo3(name = "reconciliation_instrument_ids")]
    fn py_reconciliation_instrument_ids(&self) -> Option<Vec<String>> {
        self.reconciliation_instrument_ids.clone()
    }

    #[getter]
    #[pyo3(name = "filter_unclaimed_external_orders")]
    const fn py_filter_unclaimed_external_orders(&self) -> bool {
        self.filter_unclaimed_external_orders
    }

    #[getter]
    #[pyo3(name = "filter_position_reports")]
    const fn py_filter_position_reports(&self) -> bool {
        self.filter_position_reports
    }

    #[getter]
    #[pyo3(name = "filtered_client_order_ids")]
    fn py_filtered_client_order_ids(&self) -> Option<Vec<String>> {
        self.filtered_client_order_ids.clone()
    }

    #[getter]
    #[pyo3(name = "generate_missing_orders")]
    const fn py_generate_missing_orders(&self) -> bool {
        self.generate_missing_orders
    }

    #[getter]
    #[pyo3(name = "inflight_check_interval_ms")]
    const fn py_inflight_check_interval_ms(&self) -> u32 {
        self.inflight_check_interval_ms
    }

    #[getter]
    #[pyo3(name = "inflight_check_threshold_ms")]
    const fn py_inflight_check_threshold_ms(&self) -> u32 {
        self.inflight_check_threshold_ms
    }

    #[getter]
    #[pyo3(name = "inflight_check_retries")]
    const fn py_inflight_check_retries(&self) -> u32 {
        self.inflight_check_retries
    }

    #[getter]
    #[pyo3(name = "open_check_interval_secs")]
    const fn py_open_check_interval_secs(&self) -> Option<f64> {
        self.open_check_interval_secs
    }

    #[getter]
    #[pyo3(name = "open_check_lookback_mins")]
    const fn py_open_check_lookback_mins(&self) -> Option<u32> {
        self.open_check_lookback_mins
    }

    #[getter]
    #[pyo3(name = "open_check_threshold_ms")]
    const fn py_open_check_threshold_ms(&self) -> u32 {
        self.open_check_threshold_ms
    }

    #[getter]
    #[pyo3(name = "open_check_missing_retries")]
    const fn py_open_check_missing_retries(&self) -> u32 {
        self.open_check_missing_retries
    }

    #[getter]
    #[pyo3(name = "open_check_open_only")]
    const fn py_open_check_open_only(&self) -> bool {
        self.open_check_open_only
    }

    #[getter]
    #[pyo3(name = "max_single_order_queries_per_cycle")]
    const fn py_max_single_order_queries_per_cycle(&self) -> u32 {
        self.max_single_order_queries_per_cycle
    }

    #[getter]
    #[pyo3(name = "single_order_query_delay_ms")]
    const fn py_single_order_query_delay_ms(&self) -> u32 {
        self.single_order_query_delay_ms
    }

    #[getter]
    #[pyo3(name = "position_check_interval_secs")]
    const fn py_position_check_interval_secs(&self) -> Option<f64> {
        self.position_check_interval_secs
    }

    #[getter]
    #[pyo3(name = "position_check_lookback_mins")]
    const fn py_position_check_lookback_mins(&self) -> u32 {
        self.position_check_lookback_mins
    }

    #[getter]
    #[pyo3(name = "position_check_threshold_ms")]
    const fn py_position_check_threshold_ms(&self) -> u32 {
        self.position_check_threshold_ms
    }

    #[getter]
    #[pyo3(name = "position_check_retries")]
    const fn py_position_check_retries(&self) -> u32 {
        self.position_check_retries
    }

    #[getter]
    #[pyo3(name = "purge_closed_orders_interval_mins")]
    const fn py_purge_closed_orders_interval_mins(&self) -> Option<u32> {
        self.purge_closed_orders_interval_mins
    }

    #[getter]
    #[pyo3(name = "purge_closed_orders_buffer_mins")]
    const fn py_purge_closed_orders_buffer_mins(&self) -> Option<u32> {
        self.purge_closed_orders_buffer_mins
    }

    #[getter]
    #[pyo3(name = "purge_closed_positions_interval_mins")]
    const fn py_purge_closed_positions_interval_mins(&self) -> Option<u32> {
        self.purge_closed_positions_interval_mins
    }

    #[getter]
    #[pyo3(name = "purge_closed_positions_buffer_mins")]
    const fn py_purge_closed_positions_buffer_mins(&self) -> Option<u32> {
        self.purge_closed_positions_buffer_mins
    }

    #[getter]
    #[pyo3(name = "purge_account_events_interval_mins")]
    const fn py_purge_account_events_interval_mins(&self) -> Option<u32> {
        self.purge_account_events_interval_mins
    }

    #[getter]
    #[pyo3(name = "purge_account_events_lookback_mins")]
    const fn py_purge_account_events_lookback_mins(&self) -> Option<u32> {
        self.purge_account_events_lookback_mins
    }

    #[getter]
    #[pyo3(name = "own_books_audit_interval_secs")]
    const fn py_own_books_audit_interval_secs(&self) -> Option<f64> {
        self.own_books_audit_interval_secs
    }

    #[getter]
    #[pyo3(name = "debug")]
    const fn py_debug(&self) -> bool {
        self.debug
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }

    fn __str__(&self) -> String {
        format!("{self:?}")
    }
}

#[pyo3_stub_gen::derive::gen_stub_pymethods]
#[pymethods]
impl RoutingConfig {
    /// Configuration for live client message routing.
    #[new]
    #[pyo3(signature = (default=None, venues=None))]
    fn py_new(default: Option<bool>, venues: Option<Vec<String>>) -> Self {
        Self {
            default: default.unwrap_or(false),
            venues,
        }
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }

    fn __str__(&self) -> String {
        format!("{self:?}")
    }

    #[getter]
    fn default(&self) -> bool {
        self.default
    }

    #[getter]
    fn venues(&self) -> Option<Vec<String>> {
        self.venues.clone()
    }
}

#[pyo3_stub_gen::derive::gen_stub_pymethods]
#[pymethods]
impl InstrumentProviderConfig {
    /// Configuration for instrument providers.
    #[new]
    #[allow(
        clippy::needless_pass_by_value,
        reason = "PyO3 #[new] requires owned params"
    )]
    #[pyo3(signature = (load_all=None, load_ids=None, filters=None, filter_callable=None, log_warnings=None))]
    fn py_new(
        load_all: Option<bool>,
        load_ids: Option<Vec<String>>,
        filters: Option<HashMap<String, Py<PyAny>>>,
        filter_callable: Option<String>,
        log_warnings: Option<bool>,
    ) -> PyResult<Self> {
        let default = Self::default();
        let filters = match filters {
            Some(raw) => coerce_json_config(raw)?,
            None => HashMap::new(),
        };
        Ok(Self {
            load_all: load_all.unwrap_or(default.load_all),
            load_ids,
            filters,
            filter_callable,
            log_warnings: log_warnings.unwrap_or(default.log_warnings),
        })
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }

    fn __str__(&self) -> String {
        format!("{self:?}")
    }

    #[getter]
    fn load_all(&self) -> bool {
        self.load_all
    }

    #[getter]
    fn load_ids(&self) -> Option<Vec<String>> {
        self.load_ids.clone()
    }

    #[getter]
    fn filters(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let dict = pyo3::types::PyDict::new(py);
        for (k, v) in &self.filters {
            let py_val = json_value_to_py(py, v)?;
            dict.set_item(k, py_val)?;
        }
        Ok(dict.into_any().unbind())
    }

    #[getter]
    fn filter_callable(&self) -> Option<String> {
        self.filter_callable.clone()
    }

    #[getter]
    fn log_warnings(&self) -> bool {
        self.log_warnings
    }
}

#[pyo3_stub_gen::derive::gen_stub_pymethods]
#[pymethods]
impl DataClientConfig {
    /// Shared configuration for data clients registered with a live node.
    #[new]
    #[pyo3(signature = (handle_revised_bars=None, instrument_provider=None, routing=None))]
    fn py_new(
        handle_revised_bars: Option<bool>,
        instrument_provider: Option<InstrumentProviderConfig>,
        routing: Option<RoutingConfig>,
    ) -> Self {
        Self {
            handle_revised_bars: handle_revised_bars.unwrap_or(false),
            instrument_provider: instrument_provider.unwrap_or_default(),
            routing: routing.unwrap_or_default(),
        }
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }

    fn __str__(&self) -> String {
        format!("{self:?}")
    }

    #[getter]
    fn handle_revised_bars(&self) -> bool {
        self.handle_revised_bars
    }

    #[getter]
    fn instrument_provider(&self) -> InstrumentProviderConfig {
        self.instrument_provider.clone()
    }

    #[getter]
    fn routing(&self) -> RoutingConfig {
        self.routing.clone()
    }
}

#[pyo3_stub_gen::derive::gen_stub_pymethods]
#[pymethods]
impl ExecutionClientConfig {
    /// Shared configuration for execution clients registered with a live node.
    #[new]
    #[pyo3(signature = (instrument_provider=None, routing=None))]
    fn py_new(
        instrument_provider: Option<InstrumentProviderConfig>,
        routing: Option<RoutingConfig>,
    ) -> Self {
        Self {
            instrument_provider: instrument_provider.unwrap_or_default(),
            routing: routing.unwrap_or_default(),
        }
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }

    fn __str__(&self) -> String {
        format!("{self:?}")
    }

    #[getter]
    fn instrument_provider(&self) -> InstrumentProviderConfig {
        self.instrument_provider.clone()
    }

    #[getter]
    fn routing(&self) -> RoutingConfig {
        self.routing.clone()
    }
}

#[pyo3_stub_gen::derive::gen_stub_pymethods]
#[pymethods]
impl PluginConfig {
    /// Configuration for one Rust-native plug-in instance loaded by a live node.
    #[new]
    #[pyo3(signature = (path, type_name, config=None, sha256=None))]
    fn py_new(
        path: String,
        type_name: String,
        config: Option<HashMap<String, Py<PyAny>>>,
        sha256: Option<String>,
    ) -> PyResult<Self> {
        let config = match config {
            Some(config) => coerce_json_config(config)?,
            None => HashMap::new(),
        };

        Ok(Self {
            path,
            type_name,
            config,
            sha256,
        })
    }

    #[getter]
    fn path(&self) -> &str {
        &self.path
    }

    #[getter]
    fn type_name(&self) -> &str {
        &self.type_name
    }

    #[getter]
    fn config(&self, py: Python<'_>) -> PyResult<Py<PyDict>> {
        let dict = PyDict::new(py);
        for (key, value) in &self.config {
            dict.set_item(key, json_value_to_py(py, value)?)?;
        }
        Ok(dict.unbind())
    }

    #[getter]
    fn sha256(&self) -> Option<&str> {
        self.sha256.as_deref()
    }
}

#[pyo3_stub_gen::derive::gen_stub_pymethods]
#[pymethods]
impl QueueMonitorConfig {
    /// Configuration for runner queue pressure monitoring.
    #[new]
    const fn py_new(
        queue_depth_trigger: usize,
        queue_depth_clear: usize,
        mean_dispatch_ns_trigger: u64,
        mean_dispatch_ns_clear: u64,
    ) -> Self {
        Self {
            queue_depth_trigger,
            queue_depth_clear,
            mean_dispatch_ns_trigger,
            mean_dispatch_ns_clear,
        }
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }

    fn __str__(&self) -> String {
        format!("{self:?}")
    }

    #[getter]
    const fn queue_depth_trigger(&self) -> usize {
        self.queue_depth_trigger
    }

    #[getter]
    const fn queue_depth_clear(&self) -> usize {
        self.queue_depth_clear
    }

    #[getter]
    const fn mean_dispatch_ns_trigger(&self) -> u64 {
        self.mean_dispatch_ns_trigger
    }

    #[getter]
    const fn mean_dispatch_ns_clear(&self) -> u64 {
        self.mean_dispatch_ns_clear
    }
}

#[pyo3_stub_gen::derive::gen_stub_pymethods]
#[pymethods]
impl LiveNodeConfig {
    /// Configuration for live Nautilus system nodes.
    #[new]
    #[expect(clippy::too_many_arguments)]
    #[pyo3(signature = (environment=None, trader_id=None, load_state=None, save_state=None, shutdown_on_error=None, logging=None, instance_id=None, timeout_connection_secs=None, timeout_reconciliation_secs=None, timeout_portfolio_secs=None, timeout_disconnection_secs=None, delay_post_stop_secs=None, timeout_shutdown_secs=None, cache=None, msgbus=None, portfolio=None, queue_monitor=None, loop_debug=None, data_engine=None, risk_engine=None, exec_engine=None, controller=None, plugins=None))]
    fn py_new(
        environment: Option<Environment>,
        trader_id: Option<TraderId>,
        load_state: Option<bool>,
        save_state: Option<bool>,
        shutdown_on_error: Option<bool>,
        logging: Option<LoggerConfig>,
        instance_id: Option<UUID4>,
        timeout_connection_secs: Option<f64>,
        timeout_reconciliation_secs: Option<f64>,
        timeout_portfolio_secs: Option<f64>,
        timeout_disconnection_secs: Option<f64>,
        delay_post_stop_secs: Option<f64>,
        timeout_shutdown_secs: Option<f64>,
        cache: Option<CacheConfig>,
        msgbus: Option<MessageBusConfig>,
        portfolio: Option<PortfolioConfig>,
        queue_monitor: Option<QueueMonitorConfig>,
        loop_debug: Option<bool>,
        data_engine: Option<LiveDataEngineConfig>,
        risk_engine: Option<LiveRiskEngineConfig>,
        exec_engine: Option<LiveExecutionEngineConfig>,
        controller: Option<ImportableControllerConfig>,
        plugins: Option<Vec<PluginConfig>>,
    ) -> PyResult<Self> {
        let default = Self::default();

        let to_duration = |value: f64, name: &str| -> PyResult<Duration> {
            duration_from_secs_f64(name, value).map_err(config_error_to_pyvalue_err)
        };

        Ok(Self {
            environment: environment.unwrap_or(default.environment),
            trader_id: trader_id.unwrap_or(default.trader_id),
            load_state: load_state.unwrap_or(default.load_state),
            save_state: save_state.unwrap_or(default.save_state),
            shutdown_on_error: shutdown_on_error.unwrap_or(default.shutdown_on_error),
            logging: logging.unwrap_or(default.logging),
            instance_id,
            timeout_connection: to_duration(
                timeout_connection_secs.unwrap_or(default.timeout_connection.as_secs_f64()),
                "timeout_connection_secs",
            )?,
            timeout_reconciliation: to_duration(
                timeout_reconciliation_secs.unwrap_or(default.timeout_reconciliation.as_secs_f64()),
                "timeout_reconciliation_secs",
            )?,
            timeout_portfolio: to_duration(
                timeout_portfolio_secs.unwrap_or(default.timeout_portfolio.as_secs_f64()),
                "timeout_portfolio_secs",
            )?,
            timeout_disconnection: to_duration(
                timeout_disconnection_secs.unwrap_or(default.timeout_disconnection.as_secs_f64()),
                "timeout_disconnection_secs",
            )?,
            delay_post_stop: to_duration(
                delay_post_stop_secs.unwrap_or(default.delay_post_stop.as_secs_f64()),
                "delay_post_stop_secs",
            )?,
            timeout_shutdown: to_duration(
                timeout_shutdown_secs.unwrap_or(default.timeout_shutdown.as_secs_f64()),
                "timeout_shutdown_secs",
            )?,
            cache,
            msgbus,
            portfolio,
            emulator: None,
            streaming: None,
            queue_monitor,
            event_store: None,
            loop_debug: loop_debug.unwrap_or(false),
            data_engine: data_engine.unwrap_or_default(),
            risk_engine: risk_engine.unwrap_or_default(),
            exec_engine: exec_engine.unwrap_or_default(),
            data_clients: HashMap::new(),
            exec_clients: HashMap::new(),
            controller,
            plugins: plugins.unwrap_or_default(),
        })
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }

    fn __str__(&self) -> String {
        format!("{self:?}")
    }

    #[getter]
    fn environment(&self) -> Environment {
        self.environment
    }

    #[getter]
    fn trader_id(&self) -> TraderId {
        self.trader_id
    }

    #[getter]
    fn load_state(&self) -> bool {
        self.load_state
    }

    #[getter]
    fn save_state(&self) -> bool {
        self.save_state
    }

    #[getter]
    fn shutdown_on_error(&self) -> bool {
        self.shutdown_on_error
    }

    #[getter]
    fn timeout_connection_secs(&self) -> f64 {
        self.timeout_connection.as_secs_f64()
    }

    #[getter]
    fn timeout_reconciliation_secs(&self) -> f64 {
        self.timeout_reconciliation.as_secs_f64()
    }

    #[getter]
    fn timeout_portfolio_secs(&self) -> f64 {
        self.timeout_portfolio.as_secs_f64()
    }

    #[getter]
    fn timeout_disconnection_secs(&self) -> f64 {
        self.timeout_disconnection.as_secs_f64()
    }

    #[getter]
    fn delay_post_stop_secs(&self) -> f64 {
        self.delay_post_stop.as_secs_f64()
    }

    #[getter]
    fn timeout_shutdown_secs(&self) -> f64 {
        self.timeout_shutdown.as_secs_f64()
    }

    #[getter]
    #[pyo3(name = "logging")]
    fn py_logging(&self) -> LoggerConfig {
        self.logging.clone()
    }

    #[getter]
    #[pyo3(name = "instance_id")]
    const fn py_instance_id(&self) -> Option<UUID4> {
        self.instance_id
    }

    #[getter]
    #[pyo3(name = "cache")]
    fn py_cache(&self) -> Option<CacheConfig> {
        self.cache.clone()
    }

    #[getter]
    #[pyo3(name = "msgbus")]
    fn py_msgbus(&self) -> Option<MessageBusConfig> {
        self.msgbus.clone()
    }

    #[getter]
    #[pyo3(name = "portfolio")]
    fn py_portfolio(&self) -> Option<PortfolioConfig> {
        self.portfolio
    }

    #[getter]
    #[pyo3(name = "queue_monitor")]
    fn py_queue_monitor(&self) -> Option<QueueMonitorConfig> {
        self.queue_monitor.clone()
    }

    #[getter]
    #[pyo3(name = "loop_debug")]
    const fn py_loop_debug(&self) -> bool {
        self.loop_debug
    }

    #[getter]
    #[pyo3(name = "data_engine")]
    fn py_data_engine(&self) -> LiveDataEngineConfig {
        self.data_engine.clone()
    }

    #[getter]
    #[pyo3(name = "risk_engine")]
    fn py_risk_engine(&self) -> LiveRiskEngineConfig {
        self.risk_engine.clone()
    }

    #[getter]
    #[pyo3(name = "exec_engine")]
    fn py_exec_engine(&self) -> LiveExecutionEngineConfig {
        self.exec_engine.clone()
    }

    #[getter]
    fn plugins(&self) -> Vec<PluginConfig> {
        self.plugins.clone()
    }

    #[getter]
    fn controller(&self) -> Option<ImportableControllerConfig> {
        self.controller.clone()
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn json_value_to_py_preserves_unsigned_integer() {
        Python::initialize();
        Python::attach(|py| {
            let expected = u64::MAX;
            let value = serde_json::Value::from(expected);
            let result = json_value_to_py(py, &value).expect("JSON value must convert to Python");

            assert_eq!(
                result
                    .extract::<u64>(py)
                    .expect("Python value must remain an unsigned integer"),
                expected
            );
        });
    }
}
