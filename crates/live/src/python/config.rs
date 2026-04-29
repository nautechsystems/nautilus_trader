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

use std::{collections::HashMap, str::FromStr, time::Duration};

use nautilus_common::{
    cache::CacheConfig, enums::Environment, logging::logger::LoggerConfig,
    msgbus::database::MessageBusConfig,
};
use nautilus_core::{UUID4, python::to_pyvalue_err};
use nautilus_model::{
    enums::BarIntervalType,
    identifiers::{ClientId, ClientOrderId, InstrumentId, TraderId},
};
use nautilus_portfolio::config::PortfolioConfig;
use pyo3::{IntoPyObject, Py, PyAny, PyResult, Python, pymethods, types::PyAnyMethods};
use rust_decimal::Decimal;

use crate::config::{
    InstrumentProviderConfig, LiveDataClientConfig, LiveDataEngineConfig, LiveExecClientConfig,
    LiveExecEngineConfig, LiveNodeConfig, LiveRiskEngineConfig, RoutingConfig,
};

fn validate_rate_limit(value: &str, name: &str) -> PyResult<()> {
    let (limit, interval) = value
        .split_once('/')
        .ok_or_else(|| to_pyvalue_err(format!("invalid `{name}`: expected 'limit/HH:MM:SS'")))?;

    let limit = limit
        .parse::<usize>()
        .map_err(|e| to_pyvalue_err(format!("invalid `{name}` limit: {e}")))?;

    if limit == 0 {
        return Err(to_pyvalue_err(format!(
            "invalid `{name}`: limit must be greater than zero"
        )));
    }

    let mut total_secs: u64 = 0;
    let mut parts = interval.split(':');
    for label in ["hours", "minutes", "seconds"] {
        let value = parts
            .next()
            .ok_or_else(|| {
                to_pyvalue_err(format!(
                    "invalid `{name}`: expected 'limit/HH:MM:SS' interval"
                ))
            })?
            .parse::<u64>()
            .map_err(|e| to_pyvalue_err(format!("invalid `{name}` {label}: {e}")))?;

        let multiplier: u64 = match label {
            "hours" => 3_600,
            "minutes" => 60,
            "seconds" => 1,
            _ => unreachable!(),
        };
        total_secs = total_secs.saturating_add(value.saturating_mul(multiplier));
    }

    if parts.next().is_some() {
        return Err(to_pyvalue_err(format!(
            "invalid `{name}`: expected 'limit/HH:MM:SS'"
        )));
    }

    if total_secs == 0 {
        return Err(to_pyvalue_err(format!(
            "invalid `{name}`: interval must be greater than zero"
        )));
    }

    Ok(())
}

fn validate_max_notional_per_order(
    max_notional_per_order: &HashMap<String, String>,
) -> PyResult<()> {
    for (instrument_id, notional) in max_notional_per_order {
        InstrumentId::from_str(instrument_id).map_err(|e| {
            to_pyvalue_err(format!(
                "invalid `max_notional_per_order` instrument ID {instrument_id:?}: {e}"
            ))
        })?;

        Decimal::from_str(notional).map_err(|e| {
            to_pyvalue_err(format!(
                "invalid `max_notional_per_order` notional {notional:?}: {e}"
            ))
        })?;
    }

    Ok(())
}

fn validate_instrument_id_strings(values: &[String], name: &str) -> PyResult<()> {
    for value in values {
        InstrumentId::from_str(value).map_err(|e| {
            to_pyvalue_err(format!("invalid `{name}` instrument ID {value:?}: {e}"))
        })?;
    }
    Ok(())
}

fn validate_client_order_id_strings(values: &[String], name: &str) -> PyResult<()> {
    for value in values {
        ClientOrderId::new_checked(value).map_err(|e| {
            to_pyvalue_err(format!("invalid `{name}` client order ID {value:?}: {e}"))
        })?;
    }
    Ok(())
}

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

// Normalizes a Python `max_notional_per_order` dict (values can be `int`, `float`,
// `str`, or `Decimal`, matching the legacy Python v1 config contract) into the
// string-keyed map stored on `LiveRiskEngineConfig`.
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

/// Converts a [`serde_json::Value`] into a Python object.
fn json_value_to_py(py: Python<'_>, value: &serde_json::Value) -> PyResult<Py<PyAny>> {
    match value {
        serde_json::Value::Null => Ok(py.None()),
        serde_json::Value::Bool(b) => Ok((*b).into_pyobject(py)?.to_owned().into_any().unbind()),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(i.into_pyobject(py)?.into_any().unbind())
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

/// Converts Python filter values into JSON values.
fn coerce_filters_to_json(
    raw: HashMap<String, Py<PyAny>>,
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
    #[pyo3(signature = (time_bars_build_with_no_updates=None, time_bars_timestamp_on_close=None, time_bars_skip_first_non_full_bar=None, time_bars_interval_type=None, time_bars_build_delay=None, time_bars_origins=None, validate_data_sequence=None, buffer_deltas=None, emit_quotes_from_book=None, emit_quotes_from_book_depths=None, external_clients=None, debug=None, graceful_shutdown_on_error=None))]
    fn py_new(
        time_bars_build_with_no_updates: Option<bool>,
        time_bars_timestamp_on_close: Option<bool>,
        time_bars_skip_first_non_full_bar: Option<bool>,
        time_bars_interval_type: Option<Py<PyAny>>,
        time_bars_build_delay: Option<u64>,
        time_bars_origins: Option<HashMap<String, u64>>,
        validate_data_sequence: Option<bool>,
        buffer_deltas: Option<bool>,
        emit_quotes_from_book: Option<bool>,
        emit_quotes_from_book_depths: Option<bool>,
        external_clients: Option<Vec<ClientId>>,
        debug: Option<bool>,
        graceful_shutdown_on_error: Option<bool>,
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
            time_bars_origins: time_bars_origins.unwrap_or_default(),
            validate_data_sequence: validate_data_sequence
                .unwrap_or(default.validate_data_sequence),
            buffer_deltas: buffer_deltas.unwrap_or(default.buffer_deltas),
            emit_quotes_from_book: emit_quotes_from_book.unwrap_or(default.emit_quotes_from_book),
            emit_quotes_from_book_depths: emit_quotes_from_book_depths
                .unwrap_or(default.emit_quotes_from_book_depths),
            external_clients,
            debug: debug.unwrap_or(default.debug),
            graceful_shutdown_on_error: graceful_shutdown_on_error
                .unwrap_or(default.graceful_shutdown_on_error),
            qsize: default.qsize,
        })
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
    #[pyo3(signature = (bypass=None, max_order_submit_rate=None, max_order_modify_rate=None, max_notional_per_order=None, debug=None, graceful_shutdown_on_error=None))]
    fn py_new(
        bypass: Option<bool>,
        max_order_submit_rate: Option<String>,
        max_order_modify_rate: Option<String>,
        max_notional_per_order: Option<HashMap<String, Py<PyAny>>>,
        debug: Option<bool>,
        graceful_shutdown_on_error: Option<bool>,
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

        validate_rate_limit(&max_order_submit_rate, "max_order_submit_rate")?;
        validate_rate_limit(&max_order_modify_rate, "max_order_modify_rate")?;
        validate_max_notional_per_order(&max_notional_per_order)?;

        Ok(Self {
            bypass: bypass.unwrap_or(default.bypass),
            max_order_submit_rate,
            max_order_modify_rate,
            max_notional_per_order,
            debug: debug.unwrap_or(default.debug),
            graceful_shutdown_on_error: graceful_shutdown_on_error
                .unwrap_or(default.graceful_shutdown_on_error),
            qsize: default.qsize,
        })
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
impl LiveExecEngineConfig {
    /// Configuration for live execution engines.
    #[new]
    #[expect(clippy::too_many_arguments)]
    #[pyo3(signature = (load_cache=None, manage_own_order_books=None, snapshot_positions_interval_secs=None, external_clients=None, allow_overfills=None, reconciliation=None, reconciliation_startup_delay_secs=None, reconciliation_lookback_mins=None, reconciliation_instrument_ids=None, filter_unclaimed_external_orders=None, filter_position_reports=None, filtered_client_order_ids=None, generate_missing_orders=None, inflight_check_interval_ms=None, inflight_check_threshold_ms=None, inflight_check_retries=None, open_check_interval_secs=None, open_check_lookback_mins=None, open_check_threshold_ms=None, open_check_missing_retries=None, open_check_open_only=None, max_single_order_queries_per_cycle=None, single_order_query_delay_ms=None, position_check_interval_secs=None, position_check_lookback_mins=None, position_check_threshold_ms=None, position_check_retries=None, purge_closed_orders_interval_mins=None, purge_closed_orders_buffer_mins=None, purge_closed_positions_interval_mins=None, purge_closed_positions_buffer_mins=None, purge_account_events_interval_mins=None, purge_account_events_lookback_mins=None, own_books_audit_interval_secs=None, debug=None))]
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
    ) -> PyResult<Self> {
        let default = Self::default();

        if let Some(delay) = reconciliation_startup_delay_secs
            && (!delay.is_finite() || delay < 0.0)
        {
            return Err(to_pyvalue_err(format!(
                "invalid `reconciliation_startup_delay_secs`: {delay} (must be a non-negative finite number)"
            )));
        }

        if let Some(ids) = reconciliation_instrument_ids.as_ref() {
            validate_instrument_id_strings(ids, "reconciliation_instrument_ids")?;
        }

        if let Some(ids) = filtered_client_order_ids.as_ref() {
            validate_client_order_id_strings(ids, "filtered_client_order_ids")?;
        }

        Ok(Self {
            load_cache: load_cache.unwrap_or(default.load_cache),
            manage_own_order_books: manage_own_order_books
                .unwrap_or(default.manage_own_order_books),
            snapshot_orders: default.snapshot_orders,
            snapshot_positions: default.snapshot_positions,
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
            graceful_shutdown_on_error: default.graceful_shutdown_on_error,
            qsize: default.qsize,
        })
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
            Some(raw) => coerce_filters_to_json(raw)?,
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
impl LiveDataClientConfig {
    /// Configuration for live data clients.
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
impl LiveExecClientConfig {
    /// Configuration for live execution clients.
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
impl LiveNodeConfig {
    /// Configuration for live Nautilus system nodes.
    #[new]
    #[expect(clippy::too_many_arguments)]
    #[pyo3(signature = (environment=None, trader_id=None, load_state=None, save_state=None, logging=None, instance_id=None, timeout_connection_secs=None, timeout_reconciliation_secs=None, timeout_portfolio_secs=None, timeout_disconnection_secs=None, delay_post_stop_secs=None, timeout_shutdown_secs=None, cache=None, msgbus=None, portfolio=None, loop_debug=None, data_engine=None, risk_engine=None, exec_engine=None))]
    fn py_new(
        environment: Option<Environment>,
        trader_id: Option<TraderId>,
        load_state: Option<bool>,
        save_state: Option<bool>,
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
        loop_debug: Option<bool>,
        data_engine: Option<LiveDataEngineConfig>,
        risk_engine: Option<LiveRiskEngineConfig>,
        exec_engine: Option<LiveExecEngineConfig>,
    ) -> PyResult<Self> {
        let default = Self::default();

        let to_duration = |value: f64, name: &str| -> PyResult<Duration> {
            if !value.is_finite() || !(0.0..=86_400.0).contains(&value) {
                return Err(to_pyvalue_err(format!(
                    "invalid {name}: {value} (must be finite, non-negative, and <= 86400)"
                )));
            }
            Ok(Duration::from_secs_f64(value))
        };

        Ok(Self {
            environment: environment.unwrap_or(default.environment),
            trader_id: trader_id.unwrap_or(default.trader_id),
            load_state: load_state.unwrap_or(default.load_state),
            save_state: save_state.unwrap_or(default.save_state),
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
            loop_debug: loop_debug.unwrap_or(false),
            data_engine: data_engine.unwrap_or_default(),
            risk_engine: risk_engine.unwrap_or_default(),
            exec_engine: exec_engine.unwrap_or_default(),
            data_clients: HashMap::new(),
            exec_clients: HashMap::new(),
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
}
